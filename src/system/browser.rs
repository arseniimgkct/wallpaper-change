use std::path::PathBuf;
use std::process::Command;
use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, REG_SZ, REG_VALUE_TYPE,
};
use crate::logger::log_message;
use crate::system::paths::get_user_pinned_taskbar_dir;

const HTTP_USER_CHOICE_KEY: PCWSTR =
    w!("Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\http\\UserChoice");

pub struct BrowserInfo {
    pub name: String,
    pub prog_id: String,
}

pub fn get_current_default_browser() -> BrowserInfo {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, HTTP_USER_CHOICE_KEY, 0, KEY_READ, &mut hkey).is_ok() {
            let mut buf = [0u16; 256];
            let mut data_type = REG_VALUE_TYPE(0);
            let mut data_size = (buf.len() * std::mem::size_of::<u16>()) as u32;

            let res = RegQueryValueExW(
                hkey,
                w!("ProgId"),
                None,
                Some(&mut data_type),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut data_size),
            );
            let _ = RegCloseKey(hkey);

            if res.is_ok() && data_type == REG_SZ {
                let prog_id = String::from_utf16_lossy(&buf)
                    .trim_matches('\0')
                    .trim()
                    .to_string();

                let name = if prog_id.starts_with("MSEdge") {
                    "Microsoft Edge".to_string()
                } else if prog_id.starts_with("Chrome") {
                    "Google Chrome".to_string()
                } else if prog_id.starts_with("Firefox") {
                    "Mozilla Firefox".to_string()
                } else if prog_id.starts_with("Brave") {
                    "Brave Browser".to_string()
                } else if prog_id.starts_with("Opera") {
                    "Opera".to_string()
                } else if prog_id.starts_with("IE.") {
                    "Internet Explorer".to_string()
                } else if prog_id.is_empty() {
                    "Неизвестно".to_string()
                } else {
                    prog_id.clone()
                };

                return BrowserInfo { name, prog_id };
            }
        }
    }

    BrowserInfo {
        name: "Неизвестно".to_string(),
        prog_id: String::new(),
    }
}

pub fn find_chrome_executable() -> Option<PathBuf> {
    let candidates = [
        std::env::var("ProgramFiles").map(|p| PathBuf::from(p).join(r"Google\Chrome\Application\chrome.exe")),
        std::env::var("ProgramFiles(x86)").map(|p| PathBuf::from(p).join(r"Google\Chrome\Application\chrome.exe")),
        std::env::var("LOCALAPPDATA").map(|p| PathBuf::from(p).join(r"Google\Chrome\Application\chrome.exe")),
    ];

    for c in candidates.into_iter().flatten() {
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

pub fn find_firefox_executable() -> Option<PathBuf> {
    let candidates = [
        std::env::var("ProgramFiles").map(|p| PathBuf::from(p).join(r"Mozilla Firefox\firefox.exe")),
        std::env::var("ProgramFiles(x86)").map(|p| PathBuf::from(p).join(r"Mozilla Firefox\firefox.exe")),
    ];

    for c in candidates.into_iter().flatten() {
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

pub fn unpin_edge_from_taskbar() -> Result<(), String> {
    log_message("Попытка открепления Microsoft Edge от панели задач...");

    // 1. Delete shortcut in User Pinned Taskbar
    if let Some(pinned_dir) = get_user_pinned_taskbar_dir() {
        if let Ok(entries) = std::fs::read_dir(&pinned_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.contains("edge") && name.ends_with(".lnk") {
                    let _ = std::fs::remove_file(entry.path());
                    log_message(&format!("Удален ярлык Edge: {:?}", entry.path()));
                }
            }
        }
    }

    // 2. PowerShell Shell.Application verb unpin
    let ps_cmd = r#"
$shell = New-Object -ComObject Shell.Application
$folder = $shell.NameSpace('shell:::{4234d49b-0245-4df3-b780-3893943456e1}')
if ($folder) {
    $item = $folder.Items() | Where-Object { $_.Name -like '*Edge*' }
    if ($item) {
        $verb = $item.Verbs() | Where-Object { $_.Name -match 'taskbarunpin|открепить от панели задач' }
        if ($verb) { $verb.DoIt() }
    }
}
"#;
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_cmd])
        .output();

    Ok(())
}

pub fn make_firefox_default() -> Result<(), String> {
    if let Some(firefox) = find_firefox_executable() {
        log_message("Установка Firefox браузером по умолчанию...");
        let _ = Command::new(firefox).arg("-setDefaultBrowser").spawn();
        open_default_apps_settings();
        Ok(())
    } else {
        Err("Mozilla Firefox не найден".to_string())
    }
}

pub fn make_chrome_default() -> Result<(), String> {
    if let Some(chrome) = find_chrome_executable() {
        log_message("Установка Chrome браузером по умолчанию...");
        let _ = Command::new(chrome).arg("chrome://settings/defaultBrowser").spawn();
        open_default_apps_settings();
        Ok(())
    } else {
        Err("Google Chrome не найден".to_string())
    }
}

pub fn open_default_apps_settings() {
    let _ = Command::new("cmd")
        .args(["/C", "start", "ms-settings:defaultapps"])
        .spawn();
}
