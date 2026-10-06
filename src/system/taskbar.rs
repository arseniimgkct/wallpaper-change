use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_SZ, REG_VALUE_TYPE,
};
use crate::logger::log_message;
use crate::system::theme::restart_explorer;

const ADVANCED_REG_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced");
const TASKBAR_SMALL_ICONS_VAL: PCWSTR = w!("TaskbarSmallIcons");
const CURRENT_VERSION_KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");

pub fn get_windows_build() -> u32 {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, CURRENT_VERSION_KEY, 0, KEY_READ, &mut hkey).is_err() {
            return 0;
        }

        let mut buf = [0u16; 64];
        let mut data_type = REG_VALUE_TYPE(0);
        let mut data_size = (buf.len() * std::mem::size_of::<u16>()) as u32;

        let res = RegQueryValueExW(
            hkey,
            w!("CurrentBuild"),
            None,
            Some(&mut data_type),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut data_size),
        );

        let _ = RegCloseKey(hkey);

        if res.is_ok() && data_type == REG_SZ {
            let s = String::from_utf16_lossy(&buf);
            let trimmed = s.trim_matches('\0').trim();
            if let Ok(build) = trimmed.parse::<u32>() {
                return build;
            }
        }
    }
    0
}

pub fn is_windows_11() -> bool {
    get_windows_build() >= 22000
}

pub fn is_taskbar_small_icons() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, ADVANCED_REG_KEY, 0, KEY_READ, &mut hkey).is_err() {
            return false;
        }

        let mut data = 0u32;
        let mut data_type = REG_VALUE_TYPE(0);
        let mut data_size = std::mem::size_of::<u32>() as u32;

        let result = RegQueryValueExW(
            hkey,
            TASKBAR_SMALL_ICONS_VAL,
            None,
            Some(&mut data_type),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut data_size),
        );

        let _ = RegCloseKey(hkey);
        if result.is_ok() && data_type == REG_DWORD {
            data == 1
        } else {
            false
        }
    }
}

pub fn set_taskbar_small_icons(small: bool) -> Result<bool, String> {
    if is_windows_11() {
        return Err("В Windows 11 настройка удалена Microsoft".to_string());
    }

    let current = is_taskbar_small_icons();
    if current == small {
        return Ok(false);
    }

    let val = if small { 1u32 } else { 0u32 };
    let val_bytes = val.to_ne_bytes();

    unsafe {
        let mut hkey = HKEY::default();
        let open_res = RegOpenKeyExW(HKEY_CURRENT_USER, ADVANCED_REG_KEY, 0, KEY_SET_VALUE, &mut hkey);
        if open_res.is_err() {
            return Err(format!("Failed to open registry key: {:?}", open_res));
        }

        let res = RegSetValueExW(
            hkey,
            TASKBAR_SMALL_ICONS_VAL,
            0,
            REG_DWORD,
            Some(&val_bytes),
        );

        let _ = RegCloseKey(hkey);

        if res.is_err() {
            return Err(format!("Failed to set TaskbarSmallIcons: {:?}", res));
        }

        log_message(&format!("Размер значков панели задач изменен: small={}", small));
    }

    restart_explorer();
    Ok(true)
}
