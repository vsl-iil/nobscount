use nix::unistd::Pid;

#[cfg(debug_assertions)]
#[macro_export]
macro_rules! debugprint {
    ($x: expr) => (eprintln!("{}", $x));
    ($x: expr, $($y: expr)+) => (eprint!("{} ", $x); debugprint!($($y),+));
}

#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! debugprint {
    ($x: expr) => ();
    ($x: expr, $($y: expr)+) => ();
}

pub fn kill_old_counter() -> nix::Result<()> {
    let localfile = std::fs::read_to_string(".counter.pid");
    if let Ok(pid) = localfile {
        match pid.parse::<i32>() {
            Ok(pid) => {
                kill(pid)?;
                return Ok(());
            },
            Err(e) => {
                debugprint!("Error parsing PID: ", e);
                return Err(nix::errno::Errno::UnknownErrno);
            },
        }
    } else {
        debugprint!("Error reading local PID. Seeking in tmp...");
    }

    let mut tmpfilepath = std::env::temp_dir();
    tmpfilepath.set_file_name(".counter.pid");
    let tmpfile = std::fs::read_to_string(tmpfilepath);
    if let Ok(pid) = tmpfile {
        match pid.parse::<i32>() {
            Ok(pid) => {
                kill(pid)?;
                return Ok(());
            },
            Err(e) => {
                debugprint!("Error parsing PID: ", e);
                return Err(nix::errno::Errno::UnknownErrno);
            },
        }
    } else {
        debugprint!("Error reading temp PID. Giving up...");
        return Err(nix::errno::Errno::EIO);
    }
}

fn kill(pid: i32) -> Result<(), nix::errno::Errno> {
    nix::sys::signal::kill(Pid::from_raw(pid), Some(nix::sys::signal::SIGINT))
}

pub fn remove_pid_file() {
    let rm = std::fs::remove_file(".counter.pid");
    #[cfg(debug_assertions)]
    if let Err(e) = rm {
        debugprint!("Unable to remove PID file from current directory:", e);
    }
}