use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, REG_VALUE_TYPE,
};
use crate::logger::log_message;
use crate::system::paths::get_saved_wallpaper_path;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("DesktopOverlay");

pub fn is_autostart_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_READ, &mut hkey).is_err() {
            return false;
        }

        let mut data_type = REG_VALUE_TYPE(0);
        let mut size = 0u32;
        let res = RegQueryValueExW(hkey, VALUE_NAME, None, Some(&mut data_type), None, Some(&mut size));
        let _ = RegCloseKey(hkey);

        res.is_ok() && size > 0
    }
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Не удалось получить путь к исполняемому файлу: {}", e))?;

    unsafe {
        let mut hkey = HKEY::default();
        let open_res = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_SET_VALUE, &mut hkey);
        if open_res.is_err() {
            return Err(format!("Не удалось открыть реестр для автозапуска: {:?}", open_res));
        }

        if enabled {
            let wallpaper_path = get_saved_wallpaper_path()
                .ok_or_else(|| "Обои еще не сохранены, невозможно включить автозапуск".to_string())?;

            let cmd_str = format!(
                "\"{}\" --apply \"{}\"",
                current_exe.to_string_lossy(),
                wallpaper_path.to_string_lossy()
            );
            let wide_chars: Vec<u16> = cmd_str.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes_len = (wide_chars.len() * std::mem::size_of::<u16>()) as u32;

            let res = RegSetValueExW(
                hkey,
                VALUE_NAME,
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(wide_chars.as_ptr() as *const u8, bytes_len as usize)),
            );
            let _ = RegCloseKey(hkey);

            if res.is_err() {
                return Err(format!("Не удалось записать значение автозапуска: {:?}", res));
            }
            log_message(&format!("Автозапуск включен: {}", cmd_str));
        } else {
            let _ = RegDeleteValueW(hkey, VALUE_NAME);
            let _ = RegCloseKey(hkey);
            log_message("Автозапуск выключен");
        }
    }
    Ok(())
}
