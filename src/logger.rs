use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use chrono::Local;
use lazy_static::lazy_static;

static PAINT_COUNTER: AtomicUsize = AtomicUsize::new(0);

lazy_static! {
    static ref LOG_MUTEX: Mutex<()> = Mutex::new(());
}

pub fn get_app_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("DesktopOverlay")
    } else {
        PathBuf::from("DesktopOverlay")
    }
}

pub fn get_log_file_path() -> PathBuf {
    get_app_dir().join("overlay.log")
}

pub fn log_message(msg: &str) {
    let _lock = LOG_MUTEX.lock().unwrap();
    let app_dir = get_app_dir();
    let _ = create_dir_all(&app_dir);

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("{} {}\n", timestamp, msg);

    let log_path = get_log_file_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = file.write_all(line.as_bytes());
    }
    eprintln!("{}", line.trim_end());
}

pub fn log_paint_event() {
    let count = PAINT_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    if count <= 8 {
        log_message(&format!("[paint #{}] перерисовка оверлея", count));
    }
}

pub fn reset_paint_counter() {
    PAINT_COUNTER.store(0, Ordering::SeqCst);
}
