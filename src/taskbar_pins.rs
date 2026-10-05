//! Открепление и закрепление значков на панели задач.
//!
//! Используется штатный глагол оболочки «Открепить от панели задач», поэтому
//! обходятся и Windows 10, и Windows 11 без прав администратора. Если глагола
//! нет, остаётся запасной путь — удалить ярлык вручную.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{FolderItem, FolderItemVerbs, FolderItems, IShellDispatch, Shell};

use crate::program;

pub const EDGE_NAME: &str = "Microsoft Edge";

/// Пространство имён «Все программы» — там лежат ярлыки автозапуска.
const ALL_PROGRAMS_NAMESPACE: &str = "shell:::{4234d49b-0245-4df3-b780-3893943456e1}";

/// Канонические имена глаголов оболочки.
const UNPIN_VERB: &str = "taskbarunpin";
const PIN_VERB: &str = "taskbarpin";

/// Подсказки в тексте пункта меню: у Windows он локализован.
const UNPIN_HINTS: [&str; 2] = ["unpin from taskbar", "открепить от панели задач"];
const PIN_HINTS: [&str; 2] = ["pin to taskbar", "закрепить на панели задач"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarResult {
    /// Значок и не был закреплён — ничего не меняли.
    NothingToDo,
    /// Значок откреплён штатным глаголом оболочки.
    Done,
    /// Значок откреплён удалением ярлыка, нужен перезапуск проводника.
    DoneNeedsExplorerRestart,
    /// Не получилось.
    Failed,
}

/// Ярлык закреплённого значка, если он есть.
pub fn find_pinned_shortcut(item_name: &str) -> Option<PathBuf> {
    let path = program::taskbar_pinned_dir().join(format!("{item_name}.lnk"));
    path.is_file().then_some(path)
}

pub fn is_pinned(item_name: &str) -> bool {
    find_pinned_shortcut(item_name).is_some()
}

/// Снимает значок с панели задач.
pub fn try_unpin(item_name: &str) -> (TaskbarResult, String) {
    if find_pinned_shortcut(item_name).is_none() {
        return (TaskbarResult::NothingToDo, format!("«{item_name}» не был закреплён на панели задач."));
    }

    let via_verb = invoke_taskbar_verb(item_name, UNPIN_VERB, &UNPIN_HINTS);
    program::log(&format!(
        "Панель задач: попытка открепить «{item_name}», глагол={}",
        if via_verb { "сработал" } else { "не найден" }
    ));

    if via_verb {
        thread::sleep(Duration::from_millis(300));
    }

    // Глагол мог сработать не полностью — проверяем результат.
    if let Some(shortcut) = find_pinned_shortcut(item_name) {
        match std::fs::remove_file(&shortcut) {
            Ok(()) => {
                program::log(&format!("Панель задач: ярлык «{item_name}» удалён напрямую"));
                return (
                    TaskbarResult::DoneNeedsExplorerRestart,
                    format!("«{item_name}» откреплён от панели задач."),
                );
            }
            Err(e) => program::log(&format!("ошибка удаления ярлыка панели задач: {e}")),
        }
    }

    if is_pinned(item_name) {
        (
            TaskbarResult::Failed,
            format!(
                "Не удалось открепить «{item_name}» — правый клик по значку → \
                 «Открепить от панели задач»."
            ),
        )
    } else {
        (TaskbarResult::Done, format!("«{item_name}» откреплён от панели задач."))
    }
}

/// Возвращает значок на панель задач.
pub fn try_pin(item_name: &str) -> (bool, String) {
    if is_pinned(item_name) {
        return (true, format!("«{item_name}» уже закреплён на панели задач."));
    }

    let via_verb = invoke_taskbar_verb(item_name, PIN_VERB, &PIN_HINTS);
    program::log(&format!(
        "Панель задач: попытка закрепить «{item_name}», глагол={}",
        if via_verb { "сработал" } else { "не найден" }
    ));

    thread::sleep(Duration::from_millis(400));

    let pinned = is_pinned(item_name);
    let report = if pinned {
        format!("«{item_name}» закреплён на панели задач.")
    } else {
        format!(
            "Не удалось закрепить «{item_name}» автоматически — правый клик по значку \
             в меню «Пуск»."
        )
    };
    (pinned, report)
}

/// Вызывает глагол оболочки по каноническому имени, а если его нет — ищет
/// пункт меню по названию на текущем языке интерфейса.
fn invoke_taskbar_verb(item_name: &str, canonical_verb: &str, display_hints: &[&str]) -> bool {
    // COM нужен только на время вызова: создавать и уничтожать его каждый раз
    // дорого, а постоянно живущий Apartment в программе не нужен.
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = invoke_taskbar_verb_impl(item_name, canonical_verb, display_hints);
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

fn invoke_taskbar_verb_impl(item_name: &str, canonical_verb: &str, display_hints: &[&str]) -> bool {
    unsafe {
        let shell: IShellDispatch = match CoCreateInstance(&Shell, None, CLSCTX_ALL) {
            Ok(shell) => shell,
            Err(e) => {
                program::log(&format!("Панель задач: не удалось подключиться к оболочке: {e}"));
                return false;
            }
        };

        let namespace = VARIANT::from(ALL_PROGRAMS_NAMESPACE);

        let folder = match shell.NameSpace(&namespace) {
            Ok(folder) => folder,
            Err(e) => {
                program::log(&format!("Панель задач: нет доступа к меню «Пуск»: {e}"));
                return false;
            }
        };

        let items: FolderItems = match folder.Items() {
            Ok(items) => items,
            Err(_) => return false,
        };

        let count = items.Count().unwrap_or(0);
        for index in 0..count {
            let Ok(item) = items.Item(&VARIANT::from(index)) else { continue };

            let name = item.Name().map(|b| b.to_string()).unwrap_or_default();
            if !name.to_lowercase().contains(&item_name.to_lowercase()) {
                continue;
            }

            match invoke_verb(&item, canonical_verb) {
                Ok(()) => return true,
                Err(e) => program::log(&format!(
                    "Панель задач: глагол {canonical_verb} для «{name}» недоступен ({e})"
                )),
            }

            if invoke_verb_by_text(&item, display_hints) {
                return true;
            }
        }
    }

    false
}

/// Вызывает глагол по каноническому имени.
unsafe fn invoke_verb(item: &FolderItem, canonical_verb: &str) -> windows::core::Result<()> {
    item.InvokeVerb(&VARIANT::from(canonical_verb))
}

/// Ищет пункт меню по названию — запасной путь для локализованных систем.
unsafe fn invoke_verb_by_text(item: &FolderItem, display_hints: &[&str]) -> bool {
    let Ok(verbs): Result<FolderItemVerbs, _> = item.Verbs() else {
        return false;
    };

    let count = verbs.Count().unwrap_or(0);
    for index in 0..count {
        let Ok(verb) = verbs.Item(&VARIANT::from(index)) else { continue };

        let text = verb.Name().map(|b| b.to_string()).unwrap_or_default();
        let text = text.replace('&', "");
        let lowered = text.to_lowercase();

        if !display_hints.iter().any(|hint| lowered.contains(&hint.to_lowercase())) {
            continue;
        }

        if verb.DoIt().is_ok() {
            return true;
        }
    }

    false
}