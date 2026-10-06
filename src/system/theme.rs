use std::process::Command;
use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};
use crate::logger::log_message;

const THEME_REG_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
const APPS_VALUE_NAME: PCWSTR = w!("AppsUseLightTheme");
const SYSTEM_VALUE_NAME: PCWSTR = w!("SystemUsesLightTheme");

pub fn is_light_theme() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, THEME_REG_KEY, 0, KEY_READ, &mut hkey).is_err() {
            return true;
        }

        let mut data = 0u32;
        let mut data_type = REG_VALUE_TYPE(0);
        let mut data_size = std::mem::size_of::<u32>() as u32;

        let result = RegQueryValueExW(
            hkey,
            APPS_VALUE_NAME,
            None,
            Some(&mut data_type),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut data_size),
        );

        let _ = RegCloseKey(hkey);
        if result.is_ok() && data_type == REG_DWORD {
            data != 0
        } else {
            true
        }
    }
}

pub fn set_light_theme(light: bool) -> Result<bool, String> {
    let current = is_light_theme();
    if current == light {
        return Ok(false);
    }

    let val = if light { 1u32 } else { 0u32 };
    let val_bytes = val.to_ne_bytes();

    unsafe {
        let mut hkey = HKEY::default();
        let open_res = RegOpenKeyExW(HKEY_CURRENT_USER, THEME_REG_KEY, 0, KEY_SET_VALUE, &mut hkey);
        if open_res.is_err() {
            return Err(format!("Failed to open registry key: {:?}", open_res));
        }

        let r1 = RegSetValueExW(
            hkey,
            APPS_VALUE_NAME,
            0,
            REG_DWORD,
            Some(&val_bytes),
        );

        let r2 = RegSetValueExW(
            hkey,
            SYSTEM_VALUE_NAME,
            0,
            REG_DWORD,
            Some(&val_bytes),
        );

        let _ = RegCloseKey(hkey);

        if r1.is_err() {
            return Err(format!("Failed to set AppsUseLightTheme: {:?}", r1));
        }
        if r2.is_err() {
            return Err(format!("Failed to set SystemUsesLightTheme: {:?}", r2));
        }

        // Broadcast setting change
        let mut result_lparam = 0;
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            500,
            Some(&mut result_lparam),
        );

        log_message(&format!("Смена темы Windows: light={}", light));
    }

    restart_explorer();
    Ok(true)
}

pub fn restart_explorer() {
    log_message("Перезапуск Explorer.exe для применения настроек...");
    let _ = Command::new("taskkill").args(["/F", "/IM", "explorer.exe"]).status();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let _ = Command::new("explorer.exe").spawn();
}
