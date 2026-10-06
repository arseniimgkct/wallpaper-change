use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
    PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};
use windows::Win32::System::Threading::CreateMutexW;

const MUTEX_NAME: PCWSTR = w!("Local\\DesktopOverlay.SingleInstance");
const PIPE_NAME: &str = r"\\.\pipe\DesktopOverlay.IPC";
const PIPE_NAME_W: PCWSTR = w!(r"\\.\pipe\DesktopOverlay.IPC");

#[derive(Debug, Clone)]
pub enum IpcCommand {
    Show,
    Revert,
    Set(PathBuf),
}

pub struct SingleInstanceLock {
    handle: HANDLE,
    is_first: bool,
}

impl SingleInstanceLock {
    pub fn try_acquire() -> Self {
        unsafe {
            let handle = match CreateMutexW(None, true, MUTEX_NAME) {
                Ok(h) => h,
                Err(_) => return Self { handle: HANDLE(std::ptr::null_mut()), is_first: false },
            };
            let is_first = GetLastError() != ERROR_ALREADY_EXISTS;
            Self { handle, is_first }
        }
    }

    pub fn is_first_instance(&self) -> bool {
        self.is_first
    }
}

impl Drop for SingleInstanceLock {
    fn drop(&mut self) {
        if !self.handle.0.is_null() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

pub fn send_ipc_command(cmd: &IpcCommand) -> Result<(), String> {
    let msg = match cmd {
        IpcCommand::Show => "SHOW".to_string(),
        IpcCommand::Revert => "REVERT".to_string(),
        IpcCommand::Set(path) => format!("SET {}", path.to_string_lossy()),
    };

    let mut attempts = 0;
    while attempts < 5 {
        attempts += 1;
        if let Ok(mut pipe) = OpenOptions::new().read(true).write(true).open(PIPE_NAME) {
            let _ = pipe.write_all(format!("{}\n", msg).as_bytes());
            let _ = pipe.flush();
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    Err(format!("Не удалось отправить IPC команду: {}", msg))
}

pub fn start_ipc_server(tx: Sender<IpcCommand>, is_running: Arc<AtomicBool>) {
    thread::Builder::new()
        .name("IpcServerThread".to_string())
        .spawn(move || {
            while is_running.load(Ordering::SeqCst) {
                unsafe {
                    let pipe_handle = CreateNamedPipeW(
                        PIPE_NAME_W,
                        PIPE_ACCESS_DUPLEX,
                        PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                        1,
                        512,
                        512,
                        1000,
                        None,
                    );

                    if pipe_handle.0.is_null() || pipe_handle == INVALID_HANDLE_VALUE {
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }

                    let connected = ConnectNamedPipe(pipe_handle, None).is_ok()
                        || GetLastError() == windows::Win32::Foundation::ERROR_PIPE_CONNECTED;

                    if connected {
                        let mut buffer = [0u8; 1024];
                        let mut bytes_read = 0u32;
                        let res = windows::Win32::Storage::FileSystem::ReadFile(
                            pipe_handle,
                            Some(&mut buffer),
                            Some(&mut bytes_read),
                            None,
                        );

                        if res.is_ok() && bytes_read > 0 {
                            let msg = String::from_utf8_lossy(&buffer[..bytes_read as usize]);
                            let line = msg.lines().next().unwrap_or("").trim();

                            if line == "SHOW" {
                                let _ = tx.send(IpcCommand::Show);
                            } else if line == "REVERT" {
                                let _ = tx.send(IpcCommand::Revert);
                            } else if let Some(path_str) = line.strip_prefix("SET ") {
                                let path = PathBuf::from(path_str.trim().trim_matches('"'));
                                let _ = tx.send(IpcCommand::Set(path));
                            }
                        }

                        let _ = DisconnectNamedPipe(pipe_handle);
                    }

                    let _ = CloseHandle(pipe_handle);
                }
            }
        })
        .expect("Failed to start IPC server thread");
}
