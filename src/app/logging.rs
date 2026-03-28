use std::backtrace::Backtrace;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::panic;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const PRIVATE_FILE_MODE: u32 = 0o600;

struct Logger {
    file: Mutex<File>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();
static CRASH_FD: AtomicI32 = AtomicI32::new(-1);
static ALT_SIGNAL_STACK: OnceLock<Box<[u8]>> = OnceLock::new();
static HOOKS_INSTALLED: OnceLock<()> = OnceLock::new();

pub fn init() {
    let log_path = standard_log_path();

    if let Some(path) = &log_path {
        match open_logger(path) {
            Ok((file, crash_fd)) => {
                let logger = Logger {
                    file: Mutex::new(file),
                };

                let _ = LOGGER.set(logger);
                CRASH_FD.store(crash_fd, Ordering::SeqCst);
            }
            Err(error) => {
                eprintln!("TerminalTiler logging init failed: {error}");
            }
        }
    }

    install_hooks();

    if let Some(path) = log_path {
        info(format!("logging initialized at {}", path.display()));
    } else {
        error("could not resolve an XDG state directory for logs");
    }
}

pub fn info(message: impl AsRef<str>) {
    write_log_line("INFO", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write_log_line("ERROR", message.as_ref());
}

fn standard_log_path() -> Option<PathBuf> {
    ProjectDirs::from("dev", "Zethrus", "TerminalTiler").and_then(|dirs| {
        dirs.state_dir()
            .map(|state_dir| state_dir.join("logs").join("terminaltiler.log"))
    })
}

fn open_logger(path: &PathBuf) -> std::io::Result<(File, i32)> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(PRIVATE_FILE_MODE);

    let file = options.open(path)?;
    let crash_fd = unsafe { libc::dup(file.as_raw_fd()) };

    if crash_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok((file, crash_fd))
}

fn write_log_line(level: &str, message: &str) {
    let line = format!("[{}] {} {}\n", unix_timestamp(), level, message);

    if let Some(logger) = LOGGER.get()
        && let Ok(mut file) = logger.file.lock()
    {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
        return;
    }

    eprint!("{line}");
}

fn unix_timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("{}.{:03}", duration.as_secs(), duration.subsec_millis()),
        Err(_) => "0.000".into(),
    }
}

fn install_hooks() {
    if HOOKS_INSTALLED.set(()).is_err() {
        return;
    }

    install_panic_hook();
    install_signal_handlers();
}

fn install_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".into());

        let payload = if let Some(message) = panic_info.payload().downcast_ref::<&str>() {
            (*message).to_string()
        } else if let Some(message) = panic_info.payload().downcast_ref::<String>() {
            message.clone()
        } else {
            "non-string panic payload".into()
        };

        error(format!(
            "panic at {}: {}\nbacktrace:\n{}",
            location,
            payload,
            Backtrace::force_capture()
        ));
    }));
}

fn install_signal_handlers() {
    let stack_size = libc::SIGSTKSZ.max(64 * 1024);
    let mut stack = vec![0u8; stack_size].into_boxed_slice();

    let alt_stack = libc::stack_t {
        ss_sp: stack.as_mut_ptr().cast(),
        ss_flags: 0,
        ss_size: stack.len(),
    };

    unsafe {
        if libc::sigaltstack(&alt_stack, ptr::null_mut()) != 0 {
            error(format!(
                "sigaltstack installation failed: {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    let _ = ALT_SIGNAL_STACK.set(stack);

    for signal in [
        libc::SIGABRT,
        libc::SIGBUS,
        libc::SIGFPE,
        libc::SIGILL,
        libc::SIGSEGV,
    ] {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK | libc::SA_RESETHAND;
            action.sa_sigaction = crash_signal_handler as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);

            if libc::sigaction(signal, &action, ptr::null_mut()) != 0 {
                error(format!(
                    "sigaction installation failed for signal {}: {}",
                    signal,
                    std::io::Error::last_os_error()
                ));
            }
        }
    }
}

unsafe extern "C" fn crash_signal_handler(
    signal: libc::c_int,
    _info: *mut libc::siginfo_t,
    _context: *mut libc::c_void,
) {
    let fd = CRASH_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        let message = match signal {
            libc::SIGABRT => b"fatal signal: SIGABRT\n".as_slice(),
            libc::SIGBUS => b"fatal signal: SIGBUS\n".as_slice(),
            libc::SIGFPE => b"fatal signal: SIGFPE\n".as_slice(),
            libc::SIGILL => b"fatal signal: SIGILL\n".as_slice(),
            libc::SIGSEGV => b"fatal signal: SIGSEGV\n".as_slice(),
            _ => b"fatal signal: UNKNOWN\n".as_slice(),
        };
        let prefix = b"TerminalTiler crash handler captured ";

        unsafe {
            libc::write(fd, prefix.as_ptr().cast(), prefix.len());
            libc::write(fd, message.as_ptr().cast(), message.len());
            libc::fsync(fd);
        }
    }

    unsafe {
        libc::_exit(128 + signal);
    }
}
