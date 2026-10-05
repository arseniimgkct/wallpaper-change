//! Диалоги Windows: открытие файла, выбор цвета и сообщения.

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Controls::Dialogs::{ChooseColorW, CHOOSECOLORW, CC_FULLOPEN, CC_RGBINIT};
use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_DEFBUTTON2, MB_ICONERROR, MB_ICONEXCLAMATION, MB_ICONINFORMATION, MB_ICONQUESTION,
    MB_OK, MB_YESNO, MESSAGEBOX_STYLE, IDYES,
};
use windows::core::{HSTRING, PCWSTR};

use crate::program;

/// Диалог открытия файла с фильтром на изображения.
pub fn open_image_file(owner: HWND) -> Option<std::path::PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog =
            match CoCreateInstance(&FileOpenDialog, None::<&windows::core::IUnknown>, CLSCTX_INPROC_SERVER) {
                Ok(dialog) => dialog,
                Err(e) => {
                    program::log(format!("не удалось создать диалог открытия файла: {e}"));
                    return None;
                }
            };

        let _ = dialog.SetTitle(&HSTRING::from("Выберите картинку для обоев"));

        // «Изображения» и «Все файлы» — те же два варианта, что и в оригинале.
        let name = HSTRING::from("Изображения");
        let spec = HSTRING::from("*.jpg;*.jpeg;*.png;*.bmp;*.gif;*.tif;*.tiff");
        let all_name = HSTRING::from("Все файлы");
        let all_spec = HSTRING::from("*.*");

        let filters = [
            COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) },
            COMDLG_FILTERSPEC {
                pszName: PCWSTR(all_name.as_ptr()),
                pszSpec: PCWSTR(all_spec.as_ptr()),
            },
        ];
        let _ = dialog.SetFileTypes(&filters);
        let _ = dialog.SetFileTypeIndex(1);

        // Пользователь закрыл диалог — это не ошибка.
        if dialog.Show(Some(owner)).is_err() {
            return None;
        }

        let item = dialog.GetResult().ok()?;
        let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        Some(std::path::PathBuf::from(program::from_pwstr(Some(path))))
    }
}

/// Диалог выбора произвольного цвета.
pub fn choose_color(owner: HWND, initial: [u8; 3]) -> Option<[u8; 3]> {
    unsafe {
        let mut dialog = CHOOSECOLORW {
            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
            hwndOwner: owner,
            rgbResult: colorref(initial),
            Flags: CC_FULLOPEN | CC_RGBINIT,
            lpTemplateName: PCWSTR::null(),
            ..Default::default()
        };

        // `ChooseColor` показывает модальный диалог и ждёт выбора.
        if !ChooseColorW(&mut dialog).as_bool() {
            return None;
        }

        let value = dialog.rgbResult.0;
        Some([(value & 0xFF) as u8, ((value >> 8) & 0xFF) as u8, ((value >> 16) & 0xFF) as u8])
    }
}

/// Цвет в формате GDI: `0x00BBGGRR`.
fn colorref(color: [u8; 3]) -> COLORREF {
    COLORREF((color[0] as u32) | ((color[1] as u32) << 8) | ((color[2] as u32) << 16))
}

/// Вопрос «да/нет». `true` — пользователь согласился.
pub fn confirm(owner: HWND, text: &str, caption: &str) -> bool {
    let text = program::utf16_with_nul(text);
    let caption = program::utf16_with_nul(caption);

    unsafe {
        MessageBoxW(
            Some(owner),
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
        ) == IDYES
    }
}

/// Сообщение об ошибке.
pub fn show_error(owner: HWND, text: &str) {
    show(owner, text, program::APP_NAME, MB_OK | MB_ICONERROR);
}

/// Информационное сообщение.
pub fn show_info(owner: HWND, text: &str, caption: &str) {
    show(owner, text, caption, MB_OK | MB_ICONINFORMATION);
}

/// Предупреждение без значка «ошибка».
pub fn show_warning(owner: HWND, text: &str) {
    show(owner, text, program::APP_NAME, MB_OK | MB_ICONEXCLAMATION);
}

fn show(owner: HWND, text: &str, caption: &str, style: MESSAGEBOX_STYLE) {
    let text = program::utf16_with_nul(text);
    let caption = program::utf16_with_nul(caption);

    unsafe {
        let _ = MessageBoxW(
            Some(owner),
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            style,
        );
    }
}
