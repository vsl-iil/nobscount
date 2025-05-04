use std::{collections::HashMap, fs::File, io::{BufRead, BufReader, BufWriter, Read, Write}, net::{IpAddr, TcpListener, TcpStream}, process::exit, str::from_utf8, time::Instant};
use serde::Deserialize;
use toml::Table;

#[macro_use] mod util;

const COUNTER_FILE: &str = "count.bin";

const BIND_ADDR:    &str = "0.0.0.0:1234";
const IMAGE_DIR:    &str = "img";
const IMG_FORMAT:   &str = "jpg";
const CONTENT_TYPE: &str = "image/jpeg";
const TIMEOUT:      u64  = 3600;

#[derive(Deserialize)]
struct Config {
    counterfile: String,
    bind_addr:   String,
    image_dir:   String,
    img_format:  String,
    content_type:String,
    count_unique:bool,
    timeout:     u64,
    blacklist:   Vec<IpAddr>,
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
            blacklist: Vec::new()
        }
    }
}

const OK: &str = "200 OK";
const BAD_REQUEST: &str = "400 Bad Request";
const INTERNAL_ERROR: &str = "500 Internal Server Error";

fn main() {
    let mut config: Config = Config::default();
    load_config_from_file(&mut config, "config.toml");

    println!("Listening on {}", config.bind_addr);

    let mut counter = Counter::new(
                            read_number(&config.counterfile).unwrap_or_default(), 
                            config.counterfile.to_string(),
                            config.count_unique,
                            config.timeout,
                            config.blacklist
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
}

struct Counter {
    count: usize,
    filepath: String,
    uniques: HashMap<IpAddr, Instant>,
    count_unique: bool,
    timeout: u64,
    blacklist: Vec<IpAddr>
}

impl Counter {
    pub fn new(count: usize, filepath: String, count_unique: bool, timeout: u64, blacklist: Vec<IpAddr>) -> Self {
        Counter { count, filepath, uniques: HashMap::new(), count_unique, timeout, blacklist }
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
                        if !self.blacklist.contains(&ip) {
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
                    respond(&mut stream, OK);
                },
                "/get" => {
                    match arg {
                        Some(arg) => match arg {
                            0 => {
                                eprintln!("Argument cannot be equal 0");
                                respond(&mut stream, BAD_REQUEST);
                            }
                            _ => {
                                self.send_counter_image(&mut stream, arg);
                            }
                        }
                        None => {
                            eprintln!("Unparsable argument or wrong argument name");
                            respond(&mut stream, BAD_REQUEST);
                        }
                    }
                }
                _ => {
                    eprintln!("Unknown method: {method}");
                    respond(&mut stream, BAD_REQUEST);
                }
            }
        } else {
            eprintln!("Malformed GET header: {http_request}");
            respond(&mut stream, BAD_REQUEST);
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

        let digit = counter % 10;
        assert!(digit < 10);

        let mut status = OK;
        let mut stream = BufWriter::new(stream);
        let filebuf = match std::fs::read(format!("{IMAGE_DIR}/{digit}.{IMG_FORMAT}")) {
            Err(e) => {
                eprintln!("Error reading file! {e}");
                status = INTERNAL_ERROR;

                Vec::new()
            },
            Ok(v) => v,
        };
        let length = filebuf.len();

        let headers = format!("HTTP/1.1 {status}\r\nContent-Type: {CONTENT_TYPE}\r\nContent-Length: {length}\r\n\r\n");
        
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

fn respond(stream: &mut TcpStream, code: &str) {
    let r = stream.write_all(format!("HTTP/1.1 {code}\r\n\r\n").as_bytes());
    if let Err(e) = r {
        eprintln!("Error sending a response {code}: {e}");
    }
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
    let file_contents = std::fs::read_to_string(filepath).unwrap_or_default();
    if let Ok(fileconf) = file_contents.parse::<Table>() {
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