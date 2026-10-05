//! Перезапуск проводника: после изменения реестра оболочка должна увидеть
//! новые настройки, иначе панель задач и значки остаются прежними.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::Duration;

use windows::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, OpenProcess, TerminateProcess, WaitForSingleObject,
    PROCESS_ACCESS_RIGHTS, PROCESS_TERMINATE,
};

use crate::native;
use crate::program;

const TASKBAR_CLASS: &str = "Shell_TrayWnd";

/// Убивает проводник и дожидается, пока панель задач поднимется заново.
pub fn restart_explorer() {
    program::log("Перезапуск explorer.exe...");

    for pid in explorer_pids() {
        kill(pid);
    }

    thread::sleep(Duration::from_millis(800));

    if wait_for_taskbar() {
        program::log("Проводник поднялся самостоятельно");
        return;
    }

    match std::env::var("WINDIR") {
        Ok(windir) if !windir.is_empty() => {
            let path = std::path::Path::new(&windir).join("explorer.exe");
            if start_explorer(Some(&path)) {
                program::log("Проводник запущен вручную");
            }
        }
        _ => {
            if start_explorer(None) {
                program::log("Проводник запущен вручную");
            }
        }
    }
}

/// Идентификаторы всех процессов проводника текущего пользователя.
fn explorer_pids() -> Vec<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::core::PWSTR;

    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        };

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut pids = Vec::new();
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name =
                    program::from_pwstr(Some(PWSTR(entry.szExeFile.as_ptr() as *mut u16)));
                if name.eq_ignore_ascii_case("explorer.exe") {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
        pids
    }
}

/// Завершает процесс и ждёт его выхода, игнорируя ошибки доступа.
fn kill(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;

    // Нужны права и на завершение, и на ожидание, чтобы дождаться выхода.
    const SYNCHRONIZE: u32 = 0x0010_0000;
    let access = PROCESS_ACCESS_RIGHTS(PROCESS_TERMINATE.0 | SYNCHRONIZE);

    unsafe {
        let Ok(handle) = OpenProcess(access, false, pid) else {
            return;
        };

        if TerminateProcess(handle, 0).is_ok() {
            let _ = WaitForSingleObject(handle, 1500);
        }

        let _ = CloseHandle(handle);
    }
}

fn start_explorer(path: Option<&std::path::Path>) -> bool {
    let mut command = match path {
        Some(path) => Command::new(path),
        None => Command::new("explorer.exe"),
    };

    // Проводник нужно запустить в своём собственном контексте, иначе
    // оболочка переиспользует уже существующий процесс.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP.0).spawn().is_ok()
}

/// Ждёт появления панели задач, но не дольше 12 секунд.
fn wait_for_taskbar() -> bool {
    for _ in 0..48 {
        if native::find_top_level_by_class_if_exists(TASKBAR_CLASS) {
            return true;
        }
        thread::sleep(Duration::from_millis(250));
    }
    false
}