//! Определение установленных браузеров и текущего браузера по умолчанию.
//!
//! Смена самого браузера по умолчанию намеренно не делается записью в реестр:
//! Windows защищает ветки `UserChoice` и отклоняет прямую запись `ProgId`/`Hash`
//! даже для владельца ключа. Рабочий путь — сам браузер или диалог Параметров.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use windows::Win32::System::Registry::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::program::{self, RegKey};

const URL_ASSOCIATIONS_KEY: &str = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations";
const FILE_EXTS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

/// Ключи `App Paths` смотрим и в 64-, и в 32-разрядном виде.
const KEY_WOW64_64KEY: u32 = 0x0100;
const KEY_WOW64_32KEY: u32 = 0x0200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserKind {
    Chrome,
    Firefox,
    Edge,
}

impl BrowserKind {
    pub fn title(self) -> &'static str {
        match self {
            BrowserKind::Chrome => "Google Chrome",
            BrowserKind::Firefox => "Mozilla Firefox",
            BrowserKind::Edge => "Microsoft Edge",
        }
    }

    fn exe_name(self) -> &'static str {
        match self {
            BrowserKind::Chrome => "chrome.exe",
            BrowserKind::Firefox => "firefox.exe",
            BrowserKind::Edge => "msedge.exe",
        }
    }

    fn url_prog_id_prefix(self) -> &'static str {
        match self {
            BrowserKind::Chrome => "ChromeHTML",
            BrowserKind::Firefox => "FirefoxURL",
            BrowserKind::Edge => "MSEdgeHTM",
        }
    }

    fn html_prog_id_prefix(self) -> &'static str {
        match self {
            BrowserKind::Chrome => "ChromeHTML",
            BrowserKind::Firefox => "FirefoxHTML",
            BrowserKind::Edge => "MSEdgeHTM",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowserInfo {
    pub kind: BrowserKind,
    pub title: String,
    pub exe_path: PathBuf,
    pub url_prog_id: String,
    pub html_prog_id: String,
}

/// Список всех классов реестра. Читается один раз за время работы процесса.
fn class_names() -> &'static [String] {
    use std::sync::OnceLock;
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();

    NAMES.get_or_init(|| match RegKey::open_in(HKEY_CLASSES_ROOT, "", 0) {
        Some(key) => key.subkey_names(),
        None => {
            program::log("ошибка чтения списка классов: ветка HKCR недоступна");
            Vec::new()
        }
    })
}

/// Информация о браузере либо `None`, если он не установлен.
pub fn detect(kind: BrowserKind) -> Option<BrowserInfo> {
    let exe_path = find_app_path(kind.exe_name())?;

    Some(BrowserInfo {
        kind,
        title: kind.title().to_string(),
        exe_path,
        url_prog_id: resolve_prog_id(kind.url_prog_id_prefix())
            .unwrap_or_else(|| format!("{}.x", kind.url_prog_id_prefix())),
        html_prog_id: resolve_prog_id(kind.html_prog_id_prefix())
            .unwrap_or_else(|| format!("{}.x", kind.html_prog_id_prefix())),
    })
}

/// Chrome и Firefox, которые реально установлены.
pub fn detect_alternatives() -> Vec<BrowserInfo> {
    [BrowserKind::Chrome, BrowserKind::Firefox]
        .into_iter()
        .filter_map(detect)
        .collect()
}

pub fn current_title() -> String {
    current_kind().map(BrowserKind::title).unwrap_or("неизвестен").to_string()
}

/// Какой браузер сейчас назначен по умолчанию.
pub fn current_kind() -> Option<BrowserKind> {
    let mut prog_id = read_user_choice(&format!("{URL_ASSOCIATIONS_KEY}\\http\\UserChoice"));
    if prog_id.is_none() {
        prog_id = read_user_choice(&format!("{URL_ASSOCIATIONS_KEY}\\https\\UserChoice"));
    }
    if prog_id.is_none() {
        prog_id = read_user_choice(&format!("{FILE_EXTS_KEY}\\.htm\\UserChoice"));
    }

    let prog_id = prog_id?;
    let lowered = prog_id.to_ascii_lowercase();

    // Достаточно сравнения по началу ProgId — так не нужно обходить весь реестр.
    if lowered.starts_with("chromehtml") {
        return Some(BrowserKind::Chrome);
    }
    if lowered.starts_with("firefoxurl") || lowered.starts_with("firefoxhtml") {
        return Some(BrowserKind::Firefox);
    }
    if lowered.starts_with("msedge") {
        return Some(BrowserKind::Edge);
    }

    None
}

/// Передаёт выбранному браузеру `http`, `https`, `.htm` и `.html`.
///
/// Firefox умеет назначить себя сам, Chrome — только через Параметры,
/// поэтому в последнем случае открывается нужная страница настроек.
pub fn make_default(browser: &BrowserInfo) -> (bool, String) {
    if !launch_as_default(browser) {
        let report = format!(
            "Windows не дал назначить браузер без вашего участия. Откройте \
             «Параметры → Приложения → Веб-браузер по умолчанию» и выберите {}.",
            browser.title
        );
        program::log(&format!(
            "Браузер по умолчанию: {} не смог назначить себя сам, нужны Параметры",
            browser.title
        ));
        return (false, report);
    }

    // Проверяем результат: браузер мог не успеть или не суметь.
    for _ in 0..10 {
        if current_kind() == Some(browser.kind) {
            break;
        }
        thread::sleep(Duration::from_millis(300));
    }

    let done = current_kind() == Some(browser.kind);
    program::log(&format!(
        "Браузер по умолчанию: запрошен {}, подтверждено {}",
        browser.title,
        if done { "да" } else { "нет" }
    ));

    if done {
        return (
            true,
            format!(
                "{} теперь браузер по умолчанию (http, https, .htm, .html).",
                browser.title
            ),
        );
    }

    (
        false,
        format!(
            "Не удалось подтвердить смену браузера на {}. Проверьте \
             «Параметры → Приложения → Веб-браузер по умолчанию».",
            browser.title
        ),
    )
}

/// `true`, если браузер сам записал себя в систему.
fn launch_as_default(browser: &BrowserInfo) -> bool {
    // Firefox обрабатывает флаг и прописывает себя сам.
    if browser.kind == BrowserKind::Firefox {
        start(&browser.exe_path, "-setDefaultBrowser");
        return true;
    }

    // Ни Chrome, ни Edge больше не назначают себя из командной строки,
    // поэтому открываем их страницу настроек и ждём подтверждения от Параметров.
    let page = if browser.kind == BrowserKind::Chrome {
        "chrome://settings/defaultBrowser"
    } else {
        "edge://settings/defaultBrowser"
    };
    start(&browser.exe_path, page);

    // Параметры всё равно открываются, но ждать подтверждения здесь бессмысленно.
    false
}

/// Запускает программу через оболочку, чтобы работали её внутренние протоколы.
fn start(path: &std::path::Path, arguments: &str) {
    let executable = program::utf16_with_nul(&path.to_string_lossy());
    let parameters = program::utf16_with_nul(arguments);
    let operation = program::utf16_with_nul("open");

    unsafe {
        let result = ShellExecuteW(
            None,
            windows::core::PCWSTR(operation.as_ptr()),
            windows::core::PCWSTR(executable.as_ptr()),
            windows::core::PCWSTR(parameters.as_ptr()),
            windows::core::PCWSTR::null(),
            SW_SHOWNORMAL,
        );

        // ShellExecute возвращает значение больше 32 при успехе.
        if (result.0 as isize) <= 32 {
            program::log("не удалось запустить браузер");
        }
    }
}

/// Ищет ProgID обработчика: сначала точный, затем зарегистрированный с суффиксом.
fn resolve_prog_id(prefix: &str) -> Option<String> {
    if is_handler(prefix) {
        return Some(prefix.to_string());
    }

    class_names()
        .iter()
        .find(|name| {
            name.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase())
                && is_handler(name)
        })
        .cloned()
}

/// Есть ли у класса команда открытия — то есть он зарегистрирован как обработчик.
fn is_handler(prog_id: &str) -> bool {
    RegKey::open_in(HKEY_CLASSES_ROOT, &format!("{prog_id}\\shell\\open\\command"), 0).is_some()
}

/// ProgID, выбранный пользователем в Параметрах.
fn read_user_choice(key_path: &str) -> Option<String> {
    RegKey::open_in(HKEY_CURRENT_USER, key_path, 0).and_then(|k| k.get_string("ProgId"))
}

/// Путь к исполняемому файлу из `App Paths`.
fn find_app_path(exe_name: &str) -> Option<PathBuf> {
    let relative = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe_name}");

    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for options in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Some(key) = RegKey::open_in(hive, &relative, options) else {
                continue;
            };
            let Some(path) = key.get_string("") else {
                continue;
            };

            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    None
}