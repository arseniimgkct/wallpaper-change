//! Уменьшение панели задач. Работает только в Windows 10: в Windows 11
//! Microsoft отключила параметр `TaskbarSmallIcons`, и штатного способа нет.

use windows::Win32::System::SystemInformation::{GetVersionExW, OSVERSIONINFOW};

use crate::program::{self, RegKey};

const ADVANCED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const VALUE_NAME: &str = "TaskbarSmallIcons";

const SMALL_VALUE: u32 = 1;
const NORMAL_VALUE: u32 = 0;

/// Номер сборки, с которой параметр перестал работать (Windows 11).
const WINDOWS_11_BUILD: u32 = 22_000;

pub fn is_supported() -> bool {
    windows_build() < WINDOWS_11_BUILD
}

pub fn is_supported_message() -> &'static str {
    "Уменьшение панели задач поддерживается только в Windows 10. \
     В Windows 11 этот параметр отключён Microsoft, штатного способа нет."
}

/// Текущее состояние параметра. `None`, если он не задан или недоступен.
pub fn is_small() -> Option<bool> {
    RegKey::open(ADVANCED_KEY).and_then(|k| k.get_flag(VALUE_NAME))
}

/// Включает маленькие значки или возвращает обычный размер.
pub fn set_small(small: bool) -> bool {
    if !is_supported() {
        program::log("taskbar: система не поддерживается");
        return false;
    }

    let Some(key) = RegKey::create(ADVANCED_KEY) else {
        program::log("taskbar: ветка реестра недоступна для записи");
        return false;
    };

    let value = if small { SMALL_VALUE } else { NORMAL_VALUE };
    if !key.set_dword(VALUE_NAME, value) {
        program::log("taskbar: не удалось записать значение");
        return false;
    }

    let applied = key.get_flag(VALUE_NAME) == Some(small);
    program::log(&format!(
        "Панель задач: запрошена {}, подтверждена {}",
        if small { "маленькая" } else { "обычная" },
        if applied { "да" } else { "нет" }
    ));
    applied
}

/// Удаляет параметр, возвращая системное значение по умолчанию.
pub fn restore_default() -> bool {
    let Some(read_only) = RegKey::open(ADVANCED_KEY) else {
        return false;
    };
    if read_only.get_flag(VALUE_NAME) != Some(true) {
        return false;
    }

    let Some(key) = RegKey::create(ADVANCED_KEY) else {
        return false;
    };
    key.delete_value(VALUE_NAME);
    program::log("Панель задач: параметр удалён, обычный размер восстановлен");
    true
}

/// Номер сборки Windows.
fn windows_build() -> u32 {
    unsafe {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        match GetVersionExW(&mut info) {
            Ok(()) => info.dwBuildNumber,
            Err(_) => WINDOWS_11_BUILD,
        }
    }
}