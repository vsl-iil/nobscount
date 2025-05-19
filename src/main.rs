use std::{collections::HashMap, fs::File, io::{BufRead, BufReader, BufWriter, Read, Write}, net::{IpAddr, TcpListener, TcpStream}, process::exit, str::from_utf8, time::Instant};
use regex::Regex;
use toml::Table;
use util::{kill_old_counter, remove_pid_file};

#[macro_use] mod util;
mod single;

const COUNTER_FILE: &str = "count.bin";

const BIND_ADDR:    &str = "0.0.0.0:1234";
const IMAGE_DIR:    &str = "img";
const IMG_FORMAT:   &str = "jpg";
const CONTENT_TYPE: &str = "image/jpeg";
const TIMEOUT:      u64  = 3600;

struct Config {
    counterfile:    String,
    bind_addr:      String,
    image_dir:      String,
    img_format:     String,
    content_type:   String,
    count_unique:   bool,
    timeout:        u64,
    blacklist:      Vec<IpAddr>,
    ua_list:        Vec<Regex>,
    allow_empty_ua: bool
} 

impl Default for Config {
    fn default() -> Self {
        Config { 
            counterfile: COUNTER_FILE.to_owned(), 
            bind_addr: BIND_ADDR.to_owned(), 
            image_dir: IMAGE_DIR.to_owned(), 
            img_format: IMG_FORMAT.to_owned(), 
            content_type: CONTENT_TYPE.to_owned(),
            count_unique: false,
            timeout: TIMEOUT,
            blacklist: Vec::new(),
            ua_list: Vec::new(),
            allow_empty_ua: false
        }
    }
}

const OK: &str = "200 OK";
const BAD_REQUEST: &str = "400 Bad Request";
// const FORBIDDEN: &str = "403 Forbidden";
// const TEAPOT: &str = "418 I'm a teapot";
const INTERNAL_ERROR: &str = "500 Internal Server Error";

const INSTANCE_UUID: &str = "1e5319b4-73ca-447d-a05d-eca92225ebb9";

fn main() {
    let single = single::SingleInstance::new(INSTANCE_UUID);
    let mut single = single.ok();
    
    if single.as_ref().is_some_and(|inst| !inst.is_single()) {
        loop {
            eprint!("Another counter instance is running. Do you want to stop that old instance? (y/N) > ");

            let mut choice = String::new();
            match std::io::stdin().read_line(&mut choice) {
                Err(e) => {
                    eprintln!("Error reading user input: {e}. Exiting...");
                    exit(1);
                }
                Ok(_) => {},
            }

            match choice.trim() {
                "y" | "Y" => {
                    kill_old_counter().expect("Unable to kill an old instance. Maybe it's running as root?");

                    let mut retries = 20;
                    while single.as_ref().is_none_or(|inst| !inst.is_single()) && retries > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        single = single::SingleInstance::new(INSTANCE_UUID).ok();
                        retries -= 1;
                    }
                    if single.as_ref().is_none_or(|inst| !inst.is_single()) {
                        debugprint!("Wasn't able to reaquire a socket. Proceeding anyway...");
                    }
                    break;
                },
                "n" | "N" | "" => break,
                _ => eprintln!("Please, choose `y` or `n`."),
            }
        }
    }

    if single.as_ref().is_some_and(|inst| inst.is_single()) {
        let pid = nix::unistd::getpid().as_raw().to_string();
        if let Err(e) = std::fs::write(".counter.pid", pid.clone()) {
            debugprint!("Unable to write PID in current directory. Trying temp directory... ", e);
            
            let mut tmp = std::env::temp_dir();
            tmp.set_file_name(".counter.pid");
            if let Err(e) = std::fs::write(tmp, pid) {
                debugprint!("Unable to write PID to a file. Another instance won't be able to kill this one.", e);
            }
        }
    }

    if let Err(e) = ctrlc::set_handler(|| {
        remove_pid_file();
        exit(0);
    }) {
        debugprint!("Unable to set SIGINT handler. PID file won't be removed; ", e);
    }

    let mut config: Config = Config::default();
    load_config_from_file(&mut config, "config.toml");

    println!("Listening on {}", config.bind_addr);

    let mut counter = Counter::new(
                            read_number(&config.counterfile).unwrap_or_default(), 
                            config.counterfile.to_string(),
                            config.image_dir,
                            config.img_format,
                            config.content_type,
                            config.count_unique,
                            config.timeout,
                            config.blacklist,
                            config.ua_list,
                            config.allow_empty_ua
                            );

    let listener = match TcpListener::bind(config.bind_addr) {
        Err(err) => {
            eprintln!("Unable to bind address! Error: {}", err);
            exit(1);
        },
        Ok(lst) => lst,
    };

    for stream in listener.incoming() {
        let stream = match stream {
            Err(e) => {
                eprintln!("Incoming connection error: {e}");
                continue;
            },
            Ok(stm) => stm,
        };

        counter.clear_timedout();
        counter.handle_connection(stream);
    }

    remove_pid_file();
}

struct Counter {
    count: usize,
    filepath: String,
    image_dir: String,
    img_format: String,
    content_type: String,
    uniques: HashMap<IpAddr, Instant>,
    count_unique: bool,
    timeout: u64,
    blacklist: Vec<IpAddr>,
    ua_list: Vec<Regex>,
    allow_empty_ua: bool
}

impl Counter {
    pub fn new(count: usize, filepath: String, image_dir: String, img_format: String, content_type: String, count_unique: bool, timeout: u64, blacklist: Vec<IpAddr>, ua_list: Vec<Regex>, allow_empty_ua: bool) -> Self {
        Counter { count, filepath, image_dir, img_format, content_type, uniques: HashMap::new(), count_unique, timeout, blacklist, ua_list, allow_empty_ua }
    }

    pub fn clear_timedout(&mut self) {
        let mut for_removal = Vec::new();
        for (ip, timestamp) in self.uniques.iter() {
            let elapsed = Instant::now().checked_duration_since(timestamp.clone());
            if elapsed.is_some_and(|e| e.as_secs() >= self.timeout) {
                debugprint!("Removed from uniques list: ", ip.to_string());
                for_removal.push(ip.clone());
            }
        }

        for ip in for_removal {
            self.uniques.remove(&ip);
        }
    }

    pub fn handle_connection(&mut self, mut stream: TcpStream) {
        let mut ip = stream.peer_addr().ok().map(|ip| ip.ip());
        let reader = BufReader::new(&stream);
        // let mut http_request = String::new(); 
        let mut http_request = reader.lines()
                                             .take_while(|line| line.as_ref().is_ok_and(|l| !l.is_empty()))
                                             .map(|l| l.unwrap())
                                             .collect::<Vec<String>>()
                                             .join("\n");

        debugprint!(http_request);
        if let Some(realip) = check_x_real_ip(&http_request) {
            ip = Some(realip);
        }
        debugprint!(format!("New connection from {}!", ip.unwrap()));

        let allowed_useragent = allow_useragent(&http_request, &self.ua_list, self.allow_empty_ua);
        if !allowed_useragent {
            let ua = http_request.lines()
                                 .find_map(|l| l.strip_prefix("User-Agent: "))
                                 .unwrap_or("[no user-agent]");
            eprintln!("Connection filtered based on user-agent: {ua}");
        }

        http_request = http_request.lines().take(1).collect();
        let http_request = http_request.trim();
        debugprint!(http_request);

        if !http_request.starts_with("GET ") {
            return;
        }

        if let Some(uri) = http_request.split_whitespace().nth(1) {
            let tuple = uri.split_once('?');
            let mut arg: Option<u8> = None;
            let method = if let Some((m, a)) = tuple {
                arg = parse_arg(a);
                m
            } else {
                uri
            };

            match method {
                "/increment" => {
                    if let Some(ip) = ip {
                        if !self.blacklist.contains(&ip) && allowed_useragent {
                            if self.count_unique {
                                if !self.uniques.contains_key(&ip) {
                                    self.increment_counter();
                                    self.uniques.insert(ip, Instant::now());
                                    debugprint!("Added to uniques list: ", ip.to_string());
                                }
                            } else {
                                self.increment_counter();
                            }
                        }
                    }
                    if let Err(e) = respond(&mut stream, OK, Some("text/javascript")) {
                        eprintln!("Error sending OK response to /increment request: {e}");
                    }
                },
                "/get" => {
                    match arg {
                        Some(arg) => match arg {
                            0 => {
                                eprintln!("Argument cannot be equal 0");
                                if let Err(e) = respond(&mut stream, BAD_REQUEST, None) {
                                    eprintln!("Error sending response: {e}");
                                }
                            }
                            _ => {
                                self.send_counter_image(&mut stream, arg);
                            }
                        }
                        None => {
                            eprintln!("Unparsable argument or wrong argument name");
                            if let Err(e) = respond(&mut stream, BAD_REQUEST, None) {
                                eprintln!("Error sending response: {e}");
                            }
                        }
                    }
                }
                _ => {
                    eprintln!("Unknown method: {method}");
                    if let Err(e) = respond(&mut stream, BAD_REQUEST, None) {
                        eprintln!("Error sending response: {e}");
                    }
                }
            }
        } else {
            eprintln!("Malformed GET header: {http_request}");
            if let Err(e) = respond(&mut stream, BAD_REQUEST, None) {
                eprintln!("Error sending response: {e}");
            }
        }
    }

    fn increment_counter(&mut self) {
        self.count += 1;

        let bytes = self.count.to_string();
        if let Err(e) = std::fs::write(&self.filepath, bytes.as_bytes()) {
            eprintln!("Error writing counter value to file! {e}");
        }
    }

    fn send_counter_image(&self, stream: &mut TcpStream, no: u8) {
        let mut counter = self.count;

        counter /= 10usize.pow((no-1).into());

        let digit;
        if counter == 0 {
            digit = None;
        } else {
            digit = Some(counter % 10);
        }
        assert!(digit.is_none_or(|d| d < 10));

        let mut status = OK;
        let mut stream = BufWriter::new(stream);
        let digit = digit.map(|d| d.to_string()).unwrap_or("empty".to_owned());
        let filebuf = match std::fs::read(format!("{}/{}.{}", self.image_dir, digit, self.img_format)) 
        {
            Err(e) => {
                eprintln!("Error reading file! {e}");
                status = INTERNAL_ERROR;

                Vec::new()
            },
            Ok(v) => v,
        };
        let length = filebuf.len();

        let headers = format!("HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n", status, self.content_type, length);
        
        if let Err(e) = stream.write(headers.as_bytes()) {
            eprintln!("Error writing bytes to stream buffer! {e}");
        } else {
            if let Err(e) = stream.write(&filebuf) {
                eprintln!("Error writing file to stream buffer! {e}");
            }
        }

        if let Err(e) = stream.flush() {
            eprintln!("Error flushing stream buffer! {e}");
        }
    }
}

fn respond(stream: &mut TcpStream, code: &str, content_type: Option<&str>) -> std::io::Result<()> {
    stream.write_all(format!("HTTP/1.1 {code}\r\n").as_bytes())?;
    if let Some(ctype) = content_type {
        stream.write_all(format!("Content-Type: {ctype}\r\n").as_bytes())?;
    }
    stream.write_all("\r\n".as_bytes())?;
    
    Ok(())
}

fn parse_arg(arg: &str) -> Option<u8> {
    // n=0
    match arg.split_once('=') {
        None => return None,
        Some((name, value)) => {
            if name != "n" {
                return None;
            }

            return value.parse::<u8>().ok();
        }
    }
}

fn read_number(filepath: &str) -> Option<usize> {
    let file = File::open(filepath);

    let mut file = match file {
        Err(_) => {
            eprintln!("Unable to open file {filepath}");
            return None;
        },
        Ok(f) => f,
    }; 

    let mut buf = Vec::new();
    match file.read_to_end(&mut buf) {
        Err(_) => {
            eprintln!("Unable to read file {filepath}");
            return None;
        },
        Ok(_) => {},
    }

    let snum = match from_utf8(&buf) {
        Err(_) => {
            eprintln!("Unable to parse file {filepath} to UTF-8");
            return None;
        },
        Ok(s) => s,
    };

    match snum.parse::<usize>() {
        Err(_) => {
            eprintln!("Unable to parse value from {filepath}");
            return None;
        },
        Ok(v) => return Some(v),
    }
}

fn load_config_from_file(config: &mut Config, filepath: &str) {
    let file_contents = match std::fs::read_to_string(filepath) {
        Ok(contents) => contents,
        Err(e) => {
            eprintln!("Error reading config: {e}; Using default settings");
            return;
        },
    };

    match file_contents.parse::<Table>() {
        Ok(fileconf) => {
            if fileconf.contains_key("counterfile") && fileconf["counterfile"].is_str() {
                config.counterfile = fileconf["counterfile"].as_str().unwrap().to_owned();
            }
            if fileconf.contains_key("bind_addr") && fileconf["bind_addr"].is_str() {
                config.bind_addr = fileconf["bind_addr"].as_str().unwrap().to_owned();
            }
            if fileconf.contains_key("image_dir") && fileconf["image_dir"].is_str() {
                config.image_dir = fileconf["image_dir"].as_str().unwrap().to_owned();
            }
            if fileconf.contains_key("img_format") && fileconf["img_format"].is_str() {
                config.img_format = fileconf["img_format"].as_str().unwrap().to_owned();
            }
            if fileconf.contains_key("content_type") && fileconf["content_type"].is_str() {
                config.content_type = fileconf["content_type"].as_str().unwrap().to_owned();
            }
            if fileconf.contains_key("count_unique") && fileconf["count_unique"].is_bool() {
                config.count_unique = fileconf["count_unique"].as_bool().unwrap().to_owned();
            }
            if fileconf.contains_key("timeout") && fileconf["timeout"].is_integer() {
                config.timeout = fileconf["timeout"].as_integer().unwrap().to_owned() as u64;
            }
            if fileconf.contains_key("blacklist") && 
            fileconf["blacklist"].is_array() &&
            fileconf["blacklist"].as_array().unwrap().iter().all(|v| v.is_str()) {
                for ip_str in fileconf["blacklist"].as_array()
                                                        .unwrap()
                                                        .iter() 
                {
                    let ip = ip_str.as_str().map(|ip| ip.parse::<IpAddr>().ok());
                    if let Some(ip_parsed) = ip {
                        if let Some(ip_valid) = ip_parsed {
                            debugprint!("Adding IP", ip_valid);
                            config.blacklist.push(ip_valid);
                        } else {
                            eprintln!("A blacklist IP {ip_str} isn't a valid IP; check config!");
                            continue;
                        }
                    } else {
                        eprintln!("A blacklist IP {ip_str} isn't a valid string; check config!");
                        continue;
                    }
                }
            }
            if fileconf.contains_key("useragent_regexes") && 
            fileconf["useragent_regexes"].is_array() 
            {
                for re in fileconf["useragent_regexes"].as_array().unwrap().iter() {
                    if re.is_str() {
                        let regex = re.as_str().unwrap();
                        if let Ok(regex) = Regex::new(&regex) {
                            config.ua_list.push(regex);
                        } else {
                            eprintln!("Not a valid regex: {regex}; check config!");
                        }
                    }
                }
            }
            if fileconf.contains_key("allow_empty_uas") && fileconf["allow_empty_uas"].is_bool() {
                config.allow_empty_ua = fileconf["allow_empty_uas"].as_bool().unwrap();
            }
        },
        Err(e) => eprintln!("Error parsing config: {e}; using default settings"),
    }
}

fn check_x_real_ip(http_request: &str) -> Option<IpAddr> {
    let mut realip: Option<&str> = None;
    for line in http_request.lines() {
        if line.starts_with("X-Real-IP: ") {
            realip = line.strip_prefix("X-Real-IP: ").map(|s| s.trim());
            break;
        } 
    }

    if let Some(ip) = realip {
        ip.parse().ok()
    } else {
        None
    }
}

fn allow_useragent(http_request: &str, re_list: &[Regex], allow_empty: bool) -> bool {
    let useragent = http_request.lines()
                                              .find(|line| line.starts_with("User-Agent: "));

    if useragent.is_none() {
        return allow_empty;
    }

    let useragent = useragent.unwrap().strip_prefix("User-Agent: ").unwrap();

    !re_list.iter()
            .any(|re| re.is_match(useragent))
}