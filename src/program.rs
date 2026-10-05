//! Общие константы, пути, логирование и разбор аргументов командной строки.

use std::fs;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenMutexW, SYNCHRONIZATION_ACCESS_RIGHTS,
};
use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowRect, IsWindow, SetForegroundWindow, SetWindowPos,
    ShowWindow, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOW,
};
use windows::core::{BOOL, PCWSTR, PWSTR};

pub const APP_NAME: &str = "DesktopOverlay";

const MUTEX_NAME: &str = r"Local\DesktopOverlay.SingleInstance";

/// Расширения, которые приложение считает картинками.
pub const IMAGE_EXTENSIONS: [&str; 7] = [".jpg", ".jpeg", ".png", ".bmp", ".gif", ".tif", ".tiff"];

/// Имя файла, в который сохраняется сгенерированный сплошной цвет.
pub const SOLID_COLOR_FILE: &str = "wallpaper.png";

/// Имя файла, в который сохраняется картинка из буфера обмена.
pub const CLIPBOARD_FILE: &str = "wallpaper.png";

/// Заголовок ошибок и логов.
pub fn app_name() -> &'static str {
    APP_NAME
}

/// `%APPDATA%\DesktopOverlay`
pub fn data_dir() -> PathBuf {
    appdata().join(APP_NAME)
}

/// `%APPDATA%`
pub fn appdata() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
            .join("AppData")
            .join("Roaming"),
    }
}

/// Папка, куда Windows кладёт значки, закреплённые на панели задач.
pub fn taskbar_pinned_dir() -> PathBuf {
    appdata()
        .join("Microsoft")
        .join("Internet Explorer")
        .join("Quick Launch")
        .join("User Pinned")
        .join("TaskBar")
}

/// Путь к исполняемому файлу приложения.
pub fn exe_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

pub fn is_supported_image(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    // `Path::extension` возвращает расширение без точки, а список ниже с точкой.
    IMAGE_EXTENSIONS.contains(&format!(".{ext}").as_str())
}

/// Копия картинки, сохранённая при применении обоев, если она есть.
pub fn saved_wallpaper_path() -> Option<PathBuf> {
    let dir = data_dir();
    for ext in IMAGE_EXTENSIONS {
        let candidate = dir.join(format!("wallpaper{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn log(message: impl AsRef<str>) {
    let message = message.as_ref();
    let dir = data_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }

    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("overlay.log"))
    {
        let _ = writeln!(f, "{}  {}", timestamp(), message);
    }
}

/// Дата и время в формате оригинала: `yyyy-MM-dd HH:mm:ss`.
fn timestamp() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;

    let st = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

/// Ошибка, означающая, что приложение уже запущено.
pub struct AlreadyRunning;

/// Хендл, который закрывается при выходе из процесса.
struct HeldHandle(windows::Win32::Foundation::HANDLE);

// Хендл mutex'а используется только в главном потоке приложения, но
// `OnceLock` требует `Sync`, поэтому явно разрешаем передачу.
unsafe impl Send for HeldHandle {}
unsafe impl Sync for HeldHandle {}

impl Drop for HeldHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Mutex живёт вместе с процессом: пока хендл не закрыт, второй экземпляр
/// не запустится. `OnceLock` держит его до самого выхода.
static SINGLE_INSTANCE: std::sync::OnceLock<HeldHandle> = std::sync::OnceLock::new();

/// Один экземпляр приложения: второй экземпляр должен тихо завершиться.
pub fn ensure_single_instance() -> Result<(), AlreadyRunning> {
    /// Право только на ожидание: этого хватает, чтобы обнаружить живой mutex.
    const SYNCHRONIZE: SYNCHRONIZATION_ACCESS_RIGHTS = SYNCHRONIZATION_ACCESS_RIGHTS(0x0010_0000);

    unsafe {
        let name = utf16_with_nul(MUTEX_NAME);

        if let Ok(handle) = OpenMutexW(SYNCHRONIZE, false, PCWSTR(name.as_ptr())) {
            // Хендл чужого процесса нам не нужен — сразу закрываем.
            drop(HeldHandle(handle));
            return Err(AlreadyRunning);
        }

        match CreateMutexW(None, true, PCWSTR(name.as_ptr())) {
            Ok(handle) => {
                let _ = SINGLE_INSTANCE.set(HeldHandle(handle));
                Ok(())
            }
            // Не смогли создать mutex — не блокируем пользователя из-за этого.
            Err(_) => Ok(()),
        }
    }
}

/// Разбор аргументов командной строки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Обычный запуск окна утилиты.
    ShowWindow {
        preselect: Option<PathBuf>,
        auto_apply: bool,
    },
    /// `--apply <картинка>`: только оверлей и трей, без окна утилиты.
    Apply(PathBuf),
    /// `--revert [--light]`: откат без окна.
    Revert { restore_light: bool },
}

impl Command {
    pub fn parse<S: AsRef<str>>(args: &[S]) -> Command {
        let mut args = args.iter().map(|s| s.as_ref());
        let first = args.next();

        match first {
            Some("--apply") => match args.next() {
                Some(path) => Command::Apply(PathBuf::from(path)),
                None => Command::ShowWindow { preselect: None, auto_apply: false },
            },
            Some("--revert") => Command::Revert {
                restore_light: matches!(args.next(), Some("--light")),
            },
            Some("--set") => match args.next() {
                Some(path) => {
                    Command::ShowWindow { preselect: Some(PathBuf::from(path)), auto_apply: true }
                }
                None => Command::ShowWindow { preselect: None, auto_apply: false },
            },
            _ => Command::ShowWindow { preselect: None, auto_apply: false },
        }
    }
}

/// Тонкая обёртка над веткой реестра `HKEY_CURRENT_USER`.
///
/// Ветки закрываются автоматически при выходе из области видимости.
pub struct RegKey(HKEY);

impl RegKey {
    /// Открывает существующий ключ только для чтения.
    pub fn open(path: &str) -> Option<RegKey> {
        Self::open_in(HKEY_CURRENT_USER, path, 0)
    }

    /// Открывает существующий ключ в произвольном «улье» реестра.
    ///
    /// `options` может содержать `KEY_WOW64_64KEY` или `KEY_WOW64_32KEY`:
    /// на 64-разрядной Windows ветки бывают как в 64-, так и в 32-разрядном виде.
    pub fn open_in(hive: HKEY, path: &str, options: u32) -> Option<RegKey> {
        unsafe {
            let sub = utf16_with_nul(path);
            let mut hkey = HKEY::default();
            let status = RegOpenKeyExW(
                hive,
                PCWSTR(sub.as_ptr()),
                (options != 0).then_some(options),
                KEY_READ,
                &mut hkey,
            );
            if status.0 == 0 {
                Some(RegKey(hkey))
            } else {
                None
            }
        }
    }

    /// Имена подключей. Нужны, чтобы найти зарегистрированный обработчик
    /// протокола среди сотен классов.
    pub fn subkey_names(&self) -> Vec<String> {
        use windows::Win32::System::Registry::RegEnumKeyExW;

        let mut names = Vec::new();
        let mut index = 0u32;

        unsafe {
            loop {
                // Первый вызов с `None` узнаёт нужную длину имени.
                let mut size = 0u32;
                if RegEnumKeyExW(self.0, index, None, &mut size, None, None, None, None).0 != 0 {
                    break;
                }

                let mut buffer = vec![0u16; size as usize + 1];
                if RegEnumKeyExW(
                    self.0,
                    index,
                    Some(windows::core::PWSTR(buffer.as_mut_ptr())),
                    &mut size,
                    None,
                    None,
                    None,
                    None,
                )
                .0 != 0
                {
                    break;
                }

                buffer.truncate(size as usize);
                names.push(String::from_utf16_lossy(&buffer));
                index += 1;
            }
        }

        names
    }

    /// Открывает ключ для записи, создавая его при необходимости.
    pub fn create(path: &str) -> Option<RegKey> {
        unsafe {
            let sub = utf16_with_nul(path);
            let mut hkey = HKEY::default();
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_READ | KEY_WRITE,
                None,
                &mut hkey,
                None,
            );
            if status.0 == 0 {
                Some(RegKey(hkey))
            } else {
                None
            }
        }
    }

    /// Читает строковое значение.
    pub fn get_string(&self, name: &str) -> Option<String> {
        let mut kind = REG_VALUE_TYPE(0);
        let bytes = self.query_bytes(name, &mut kind)?;

        // REG_SZ (1), REG_EXPAND_SZ (2) и REG_NONE (0, тоже строка).
        if !matches!(kind.0, 0 | 1 | 2) {
            return None;
        }

        let text = utf16_from_bytes(&bytes);
        let text = text.trim_end_matches('\0').to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    /// Читает значение как флаг. Строки с числом тоже принимаются: в ветке
    /// Explorer такие значения иногда остаются строкой, и .NET их тоже читает.
    pub fn get_flag(&self, name: &str) -> Option<bool> {
        let mut kind = REG_VALUE_TYPE(0);
        let bytes = self.query_bytes(name, &mut kind)?;

        match kind.0 {
            4 => {
                // REG_DWORD
                if bytes.len() < 4 {
                    return None;
                }
                Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != 0)
            }
            3 => {
                // REG_BINARY — берём первый байт
                Some(bytes.first().copied().unwrap_or(0) != 0)
            }
            1 | 2 => {
                let text = utf16_from_bytes(&bytes);
                match text.trim_end_matches('\0').trim().parse::<i64>() {
                    Ok(v) => Some(v != 0),
                    Err(_) => None,
                }
            }
            _ => None,
        }
    }

    /// Первичное чтение значения: возвращает сырые байты и тип.
    fn query_bytes(&self, name: &str, kind: &mut REG_VALUE_TYPE) -> Option<Vec<u8>> {
        let value = utf16_with_nul(name);
        unsafe {
            let mut size = 0u32;
            let status = RegQueryValueExW(
                self.0,
                PCWSTR(value.as_ptr()),
                None,
                Some(kind),
                None,
                Some(&mut size),
            );
            if status.0 != 0 || size == 0 {
                return None;
            }

            let mut raw = vec![0u8; size as usize + 2];
            let status = RegQueryValueExW(
                self.0,
                PCWSTR(value.as_ptr()),
                None,
                Some(kind),
                Some(raw.as_mut_ptr()),
                Some(&mut size),
            );
            if status.0 != 0 {
                return None;
            }
            raw.truncate(size as usize);
            Some(raw)
        }
    }

    pub fn set_string(&self, name: &str, value: &str) -> bool {
        let name = utf16_with_nul(name);
        let data = utf16_with_nul(value);
        unsafe {
            let status = RegSetValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2)),
            );
            status.0 == 0
        }
    }

    pub fn set_dword(&self, name: &str, value: u32) -> bool {
        let name = utf16_with_nul(name);
        unsafe {
            let status = RegSetValueExW(
                self.0,
                PCWSTR(name.as_ptr()),
                None,
                REG_DWORD,
                Some(std::slice::from_raw_parts((&value as *const u32).cast::<u8>(), 4)),
            );
            status.0 == 0
        }
    }

    /// Удаляет значение. Отсутствие значения — не ошибка.
    pub fn delete_value(&self, name: &str) {
        let name = utf16_with_nul(name);
        unsafe {
            let _ = RegDeleteValueW(self.0, PCWSTR(name.as_ptr()));
        }
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

pub fn get_registry_string(path: &str, name: &str) -> Option<String> {
    RegKey::open(path).and_then(|k| k.get_string(name))
}

pub fn set_registry_string(path: &str, name: &str, value: &str) -> bool {
    RegKey::create(path).is_some_and(|k| k.set_string(name, value))
}

pub fn set_registry_dword(path: &str, name: &str, value: u32) -> bool {
    RegKey::create(path).is_some_and(|k| k.set_dword(name, value))
}

pub fn delete_registry_value(path: &str, name: &str) {
    if let Some(k) = RegKey::open(path) {
        k.delete_value(name);
    }
}

/// `Some(())`, если ветка существует и открывается, `None` — если нет.
pub fn registry_key_exists(path: &str) -> Option<()> {
    RegKey::open(path).map(|_| ())
}

/// Диалогам Windows нужны строки COM: они живут дольше вызова.
pub use windows::core::HSTRING;

/// Строка UTF-16 с завершающим нулём.
pub fn utf16_with_nul(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Строка UTF-16 без завершающего нуля: указатель на её начало можно
/// передавать в Win32, ожидающий `LPCWSTR`.
pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// Читает строку по указателю `PWSTR`, не падая на отсутствии нуля.
pub fn from_pwstr(ptr: Option<PWSTR>) -> String {
    match ptr {
        Some(p) if !p.is_null() => unsafe { String::from_utf16_lossy(p.as_wide()) },
        _ => String::new(),
    }
}

/// Интерпретирует сырые байты реестра как UTF-16 без завершающего нуля.
fn utf16_from_bytes(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if let Some(last) = units.last() {
        if *last == 0 {
            units.pop();
        }
    }
    String::from_utf16_lossy(&units)
}

/// Периметр окна. `None`, если окно не существует.
pub fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect).ok()? };
    Some(rect)
}

/// Имя класса окна.
pub fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..len as usize])
}

/// Ищет верхнее окно по имени класса. `None`, если такого окна нет.
pub fn find_top_level_by_class(name: &str) -> Option<HWND> {
    struct Search<'a> {
        name: &'a str,
        found: *mut Option<HWND>,
    }

    unsafe extern "system" fn callback(hwnd: HWND, data: LPARAM) -> BOOL {
        let search = &*(data.0 as *const Search<'_>);
        if class_name(hwnd) == search.name {
            unsafe { *search.found = Some(hwnd) };
            return false.into(); // нашли — прекращаем перебор
        }
        true.into()
    }

    let mut found: Option<HWND> = None;
    let search = Search { name, found: std::ptr::addr_of_mut!(found) };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(std::ptr::addr_of!(search) as isize));
    }
    found
}

/// `Some(окно)`, если окно всё ещё существует.
pub fn window_alive(hwnd: HWND) -> Option<HWND> {
    unsafe { IsWindow(Some(hwnd)).as_bool().then_some(hwnd) }
}

/// Прямоугольник основного монитора.
///
/// Оверлей живёт внутри окна рабочего стола, поэтому ему нужен именно
/// монитор, который Windows считает основным, а не весь виртуальный экран.
pub fn primary_screen_rect() -> RECT {
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    /// `MONITORINFOF_PRIMARY` — единственный нужный нам флаг из dwFlags.
    const MONITORINFOF_PRIMARY: u32 = 1;

    unsafe extern "system" fn callback(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let out = &mut *(data.0 as *mut Option<RECT>);

        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmonitor, &mut info).as_bool()
            && info.dwFlags & MONITORINFOF_PRIMARY != 0
        {
            *out = Some(info.rcMonitor);
            return false.into(); // нашли — прекращаем перебор
        }
        true.into()
    }

    let mut result: Option<RECT> = None;
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(callback),
            LPARAM(std::ptr::addr_of_mut!(result) as isize),
        );
    }

    result.unwrap_or_else(|| {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        if w > 0 && h > 0 {
            RECT { left: 0, top: 0, right: w, bottom: h }
        } else {
            RECT { left: 0, top: 0, right: 1920, bottom: 1080 }
        }
    })
}

/// Включает режим Per-Monitor V2, как это делает WinForms при старте.
pub fn enable_per_monitor_dpi_v2() {
    use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Преобразует экранные координаты в клиентские координаты окна `host`.
pub fn screen_to_client(host: HWND, x: i32, y: i32) -> POINT {
    use windows::Win32::Graphics::Gdi::MapWindowPoints;

    let mut points = [POINT { x, y }];
    unsafe {
        let _ = MapWindowPoints(Some(HWND::default()), Some(host), &mut points);
    }
    points[0]
}

/// Показывает окно и ставит его на передний план.
pub fn show_window(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Приблизительный размер экрана пользователя в физических пикселях.
/// Используется при генерации сплошного цвета: картинка не должна быть
/// меньше экрана, иначе получится мыло на многомониторных конфигурациях.
pub fn screen_size_for_wallpaper() -> (u32, u32) {
    let rect = primary_screen_rect();
    (
        ((rect.right - rect.left).max(1920)) as u32,
        ((rect.bottom - rect.top).max(1080)) as u32,
    )
}
