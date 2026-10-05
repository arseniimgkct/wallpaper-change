//! Переключение темы оформления Windows и автозапуск.

use std::path::PathBuf;

use crate::program::{self, RegKey};

const THEME_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsTheme {
    Light,
    Dark,
}

impl WindowsTheme {
    pub fn title(self) -> &'static str {
        match self {
            WindowsTheme::Light => "Светлая",
            WindowsTheme::Dark => "Тёмная",
        }
    }
}

/// Текущая тема Windows: `None`, если ветка реестра недоступна.
pub fn read_theme() -> Option<WindowsTheme> {
    let key = RegKey::open(THEME_KEY)?;

    // Сначала ориентируемся на тему приложений, потом на системную:
    // именно в таком порядке их проверяет сам Windows.
    if let Some(is_light) = key.get_flag("AppsUseLightTheme") {
        return Some(theme_of(is_light));
    }
    if let Some(is_light) = key.get_flag("SystemUsesLightTheme") {
        return Some(theme_of(is_light));
    }

    Some(WindowsTheme::Light)
}

fn theme_of(is_light: bool) -> WindowsTheme {
    if is_light {
        WindowsTheme::Light
    } else {
        WindowsTheme::Dark
    }
}

pub fn is_dark_theme() -> bool {
    read_theme() == Some(WindowsTheme::Dark)
}

/// Записывает тему в реестр и проверяет, что она действительно применилась.
pub fn set_theme(dark: bool) -> bool {
    let target = if dark { WindowsTheme::Dark } else { WindowsTheme::Light };
    let Some(key) = RegKey::create(THEME_KEY) else {
        program::log("ошибка переключения темы: ветка реестра недоступна");
        return false;
    };

    let value = if dark { 0 } else { 1 };
    key.set_dword("AppsUseLightTheme", value);
    key.set_dword("SystemUsesLightTheme", value);

    let applied = read_theme() == Some(target);
    program::log(&format!(
        "Тема Windows: запрошена {}, подтверждена {}",
        if dark { "тёмная" } else { "светлая" },
        if applied { "да" } else { "нет" }
    ));
    applied
}

pub fn set_light() -> bool {
    set_theme(false)
}

pub fn set_dark() -> bool {
    set_theme(true)
}

/// Меняет тему и перезапускает проводник, если это действительно нужно.
pub fn apply_and_restart_explorer(dark: bool) -> bool {
    let target = if dark { WindowsTheme::Dark } else { WindowsTheme::Light };
    let changed = read_theme() != Some(target);
    let applied = set_theme(dark);

    if changed || !applied {
        crate::shell::restart_explorer();
    }

    applied
}

/// Реестр автозапуска.
mod run_key {
    pub const PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    pub const VALUE: &str = "DesktopOverlay";
}

/// Автозапуск включён, если в ветке Run есть наша запись.
pub fn autostart_is_enabled() -> bool {
    autostart_value().is_some()
}

fn autostart_value() -> Option<String> {
    RegKey::open(run_key::PATH).and_then(|k| k.get_string(run_key::VALUE))
}

/// Команда автозапуска: путь к exe с сохранённой копией картинки.
pub fn build_autostart_command() -> Option<String> {
    let exe = program::exe_path()?;
    let image = program::saved_wallpaper_path()?;
    if !image.is_file() {
        return None;
    }
    Some(format!("\"{}\" --apply \"{}\"", exe.display(), image.display()))
}

/// Включает или выключает автозапуск.
///
/// Возвращает `Err` с причиной, если включить не удалось: вызывающий код
/// показывает это пользователю сообщением.
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let Some(key) = RegKey::create(run_key::PATH) else {
        program::log("ошибка автозапуска: ветка реестра недоступна");
        return Err("не удалось открыть ветку автозапуска в реестре".to_string());
    };

    if enabled {
        let Some(command) = build_autostart_command() else {
            return Err(
                "не удалось построить команду автозапуска: сначала установите обои".to_string()
            );
        };
        if !key.set_string(run_key::VALUE, &command) {
            return Err("не удалось записать команду автозапуска в реестр".to_string());
        }
    } else {
        key.delete_value(run_key::VALUE);
    }

    program::log(if enabled { "автозапуск включён" } else { "автозапуск выключен" });
    Ok(())
}

/// Путь к сохранённой копии картинки, если она пригодна для автозапуска.
pub fn autostart_image() -> Option<PathBuf> {
    program::saved_wallpaper_path().filter(|p| p.is_file())
}