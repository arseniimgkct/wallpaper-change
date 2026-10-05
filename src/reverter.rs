//! Полный откат: снимает оверлей, возвращает системные обои, размер панели
//! задач, тему и Edge.

use std::thread;
use std::time::Duration;

use crate::browser::{self, BrowserKind};
use crate::images;
use crate::native;
use crate::program::{self, RegKey};
use crate::taskbar;
use crate::taskbar_pins::{self, TaskbarResult};
use crate::theme;

const DESKTOP_KEY: &str = r"Control Panel\Desktop";

/// Возвращает систему в исходное состояние и описывает, что именно изменилось.
pub fn revert_all(engine: Option<&mut crate::engine::OverlayEngine>, restore_light_theme: bool) -> String {
    let mut done: Vec<&str> = Vec::new();

    if let Some(engine) = engine {
        engine.remove();
    }
    done.push("оверлей снят");

    if theme::autostart_is_enabled() {
        if theme::set_autostart(false).is_ok() {
            done.push("автозапуск выключен");
        }
    }

    if clear_wallpaper_override() {
        done.push("системные обои возвращены");
    }

    if taskbar::restore_default() {
        done.push("обычный размер панели задач восстановлен");
        crate::shell::restart_explorer();
        thread::sleep(Duration::from_millis(400));
    }

    if restore_light_theme && theme::set_light() {
        done.push("светлая тема восстановлена");
    }

    if restore_edge() {
        done.push("Edge возвращён на панель задач и как браузер по умолчанию");
        crate::shell::restart_explorer();
        thread::sleep(Duration::from_millis(400));
    }

    if images::delete_saved_images() {
        done.push("копия картинки удалена");
    }

    native::redraw_desktop();

    let report = done.join(", ");
    program::log(&format!("откат: {report}"));
    report
}

/// Возвращает Edge на панель задач и как браузер по умолчанию.
fn restore_edge() -> bool {
    let mut changed = false;

    if !taskbar_pins::is_pinned(taskbar_pins::EDGE_NAME) {
        let (pinned, _) = taskbar_pins::try_pin(taskbar_pins::EDGE_NAME);
        changed |= pinned;
    }

    if browser::current_kind() != Some(BrowserKind::Edge) {
        match browser::detect(BrowserKind::Edge) {
            None => program::log("откат: Edge не установлен, браузер по умолчанию не трогаем"),
            Some(edge) => {
                let (ok, report) = browser::make_default(&edge);
                if ok {
                    program::log("откат: Edge снова браузер по умолчанию");
                    changed = true;
                } else {
                    program::log(&format!("откат: Edge не удалось вернуть браузером по умолчанию: {report}"));
                }
            }
        }
    }

    program::log(&format!(
        "откат Edge: {}",
        if changed { "восстановлено" } else { "изменений не потребовалось" }
    ));
    changed
}

/// Убирает принудительные обои, заданные политикой или приложением.
///
/// UNC-пути пропускаем: это сетевые обои домена, их сброс ничего не даёт.
fn clear_wallpaper_override() -> bool {
    let Some(key) = RegKey::open(DESKTOP_KEY) else {
        return false;
    };

    let Some(wallpaper) = key.get_string("Wallpaper") else {
        return false;
    };
    if wallpaper.trim().is_empty() || wallpaper.starts_with(r"\\") {
        return false;
    }

    let Some(writable) = RegKey::create(DESKTOP_KEY) else {
        return false;
    };

    for name in ["Wallpaper", "WallpaperStyle", "TileWallpaper"] {
        writable.delete_value(name);
    }
    true
}