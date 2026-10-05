//! Тонкий слой над GDI: кисти, шрифты и вывод текста.
//!
//! Окна рисуются сами (owner draw), поэтому здесь собрано всё, что нужно для
//! отрисовки кнопок, подписей и подсказок в стиле исходного окна WinForms.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DeleteObject, DrawTextW, SelectObject,
    SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DT_CENTER, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_TOP, DT_VCENTER, DT_WORDBREAK, DRAW_TEXT_FORMAT,
    FW_NORMAL, HGDIOBJ, HFONT, OUT_DEFAULT_PRECIS, TRANSPARENT,
};

/// `DEFAULT_PUI_FONT` передаётся в `CreateFontW` как `ipitchandfamily`.
const DEFAULT_PUI_FONT: u32 = 0x0400;

/// Цвет в формате GDI: `0x00BBGGRR`.
pub const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF((red as u32) | ((green as u32) << 8) | ((blue as u32) << 16))
}

/// Фон окна.
pub const BACKGROUND: COLORREF = COLORREF(0x0012_1212);
/// Фон зоны drop и превью.
pub const SURFACE: COLORREF = COLORREF(0x0018_1818);
/// Обычная кнопка.
pub const BUTTON_FACE: COLORREF = COLORREF(0x001C_1C1C);
const BUTTON_BORDER: COLORREF = COLORREF(0x0032_3232);
const BUTTON_HOVER: COLORREF = COLORREF(0x0028_2828);
const BUTTON_PRESSED: COLORREF = COLORREF(0x0014_1414);
const BUTTON_TEXT: COLORREF = COLORREF(0x00DC_DCDC);

/// Главная (белая) кнопка.
const PRIMARY_FACE: COLORREF = COLORREF(0x00F0_F0F0);
const PRIMARY_TEXT: COLORREF = COLORREF(0x000F_0F0F);
const PRIMARY_HOVER: COLORREF = COLORREF(0x00FF_FFFF);
const PRIMARY_PRESSED: COLORREF = COLORREF(0x00D2_D2D2);

const DISABLED_TEXT: COLORREF = COLORREF(0x006E_6E6E);

const TEXT_PRIMARY: COLORREF = COLORREF(0x00E6_E6E6);
const TEXT_MUTED: COLORREF = COLORREF(0x0096_9696);
const TEXT_DIM: COLORREF = COLORREF(0x007D_7D7D);

thread_local! {
    /// Шрифты живут до конца процесса: создавать их на каждый кадр дорого.
    static FONTS: RefCell<HashMap<(i32, bool), HFONT>> = RefCell::new(HashMap::new());
}

/// Освобождает кэшированные шрифты. Вызывается при выходе из приложения.
pub fn release_fonts() {
    FONTS.with(|fonts| {
        for (_, font) in fonts.borrow_mut().drain() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
        }
    });
}

/// Шрифт Segoe UI нужного кегля. Кегль задаётся в логических точках, как в WinForms.
pub fn font(points: f32, bold: bool, dpi: i32) -> HFONT {
    let pixel_height = ((points as f64 * dpi as f64 / 72.0).round() as i32).max(8);
    let weight = if bold { 700 } else { FW_NORMAL.0 as i32 };
    let key = (pixel_height, bold);

    FONTS.with(|fonts| {
        let mut fonts = fonts.borrow_mut();
        if let Some(font) = fonts.get(&key) {
            return *font;
        }

        let face = windows::core::HSTRING::from("Segoe UI");
        let font = unsafe {
            CreateFontW(
                -pixel_height,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PUI_FONT,
                &face,
            )
        };

        fonts.insert(key, font);
        font
    })
}

/// Холст для отрисовки окна.
pub struct Painter {
    hdc: windows::Win32::Graphics::Gdi::HDC,
    dpi: i32,
}

impl Painter {
    pub fn new(hdc: windows::Win32::Graphics::Gdi::HDC, dpi: i32) -> Painter {
        unsafe {
            // Прозрачный фон нужен, чтобы видеть цвет кнопки под текстом.
            SetBkMode(hdc, TRANSPARENT);
        }
        Painter { hdc, dpi }
    }

    pub fn dpi(&self) -> i32 {
        self.dpi
    }

    pub fn hdc(&self) -> windows::Win32::Graphics::Gdi::HDC {
        self.hdc
    }

    /// Масштаб логических пикселей WinForms под текущий экран.
    pub fn scale(&self) -> i32 {
        (self.dpi / 96).max(1)
    }

    pub fn scale_len(&self, logical: i32) -> i32 {
        (logical as f64 * self.dpi as f64 / 96.0).round() as i32
    }

    pub fn scale_rect(&self, rect: RECT) -> RECT {
        RECT {
            left: self.scale_len(rect.left),
            top: self.scale_len(rect.top),
            right: self.scale_len(rect.right),
            bottom: self.scale_len(rect.bottom),
        }
    }

    pub fn fill(&self, rect: RECT, color: COLORREF) {
        unsafe {
            let brush = CreateSolidBrush(color);
            let old = SelectObject(self.hdc, HGDIOBJ(brush.0));
            windows::Win32::Graphics::Gdi::FillRect(self.hdc, &rect, brush);
            SelectObject(self.hdc, old);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }

    /// Рамка в один пиксель: рамка рисуется по границе прямоугольника.
    pub fn frame(&self, rect: RECT, color: COLORREF) {
        unsafe {
            let brush = CreateSolidBrush(color);
            let old = SelectObject(self.hdc, HGDIOBJ(brush.0));
            windows::Win32::Graphics::Gdi::FrameRect(self.hdc, &rect, brush);
            SelectObject(self.hdc, old);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }

    /// Текст внутри прямоугольника. Ширина и высота результата игнорируются.
    pub fn text(&self, rect: RECT, text: &str, points: f32, bold: bool, color: COLORREF) {
        self.text_with(rect, text, points, bold, color, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX)
    }

    /// Текст по центру, с переносом строк — так рисуются многострочные подписи.
    pub fn text_centered_wrapped(
        &self,
        rect: RECT,
        text: &str,
        points: f32,
        bold: bool,
        color: COLORREF,
    ) {
        self.text_with(
            rect,
            text,
            points,
            bold,
            color,
            DT_CENTER | DT_VCENTER | DT_WORDBREAK | DT_NOPREFIX,
        )
    }

    fn text_with(
        &self,
        mut rect: RECT,
        text: &str,
        points: f32,
        bold: bool,
        color: COLORREF,
        format: DRAW_TEXT_FORMAT,
    ) {
        let mut buffer: Vec<u16> = text.encode_utf16().collect();
        buffer.push(0);

        let selected = unsafe { SelectObject(self.hdc, HGDIOBJ(font(points, bold, self.dpi).0)) };
        unsafe {
            SetTextColor(self.hdc, color);
            DrawTextW(self.hdc, &mut buffer, &mut rect, format);
            SelectObject(self.hdc, selected);
        }
    }

    /// Прямоугольная кнопка в стиле исходного окна.
    pub fn button(
        &self,
        rect: RECT,
        text: &str,
        primary: bool,
        hovered: bool,
        pressed: bool,
        enabled: bool,
        points: f32,
        bold: bool,
    ) {
        let face = if !enabled {
            BUTTON_FACE
        } else if primary {
            if pressed {
                PRIMARY_PRESSED
            } else if hovered {
                PRIMARY_HOVER
            } else {
                PRIMARY_FACE
            }
        } else if pressed {
            BUTTON_PRESSED
        } else if hovered {
            BUTTON_HOVER
        } else {
            BUTTON_FACE
        };

        let border = if primary { PRIMARY_FACE } else { BUTTON_BORDER };
        let foreground = if !enabled {
            DISABLED_TEXT
        } else if primary {
            PRIMARY_TEXT
        } else {
            BUTTON_TEXT
        };

        self.fill(rect, face);
        self.frame(rect, border);

        // Горизонтальные отступы нужны, чтобы текст не липнул к рамке.
        let padding = self.scale_len(4);
        let text_rect = RECT {
            left: rect.left + padding,
            right: rect.right - padding,
            ..rect
        };

        self.text_with(
            text_rect,
            text,
            points,
            bold,
            foreground,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    }

    /// Цветная кнопка-образец: квадрат с рамкой.
    pub fn color_swatch(&self, rect: RECT, color: COLORREF) {
        self.fill(rect, color);
        self.frame(rect, rgb(60, 60, 60));
    }

    /// Флажок в стиле исходного окна.
    pub fn checkbox(&self, rect: RECT, text: &str, checked: bool, hovered: bool) {
        let box_size = self.scale_len(16);
        let box_top = rect.top + (rect.bottom - rect.top - box_size) / 2;
        let box_rect = RECT {
            left: rect.left,
            top: box_top,
            right: rect.left + box_size,
            bottom: box_top + box_size,
        };

        self.fill(box_rect, if checked { PRIMARY_FACE } else { rgb(45, 45, 45) });
        self.frame(box_rect, rgb(70, 70, 70));

        if checked {
            let size = self.scale_len(10);
            let left = box_rect.left + (box_size - size) / 2;
            let top = box_rect.top + (box_size - size) / 2;
            self.fill(
                RECT { left, top, right: left + size, bottom: top + size },
                rgb(20, 20, 20),
            );
        }

        let text_rect = RECT {
            left: box_rect.right + self.scale_len(8),
            ..rect
        };
        let color = if hovered { TEXT_PRIMARY } else { rgb(200, 200, 200) };
        self.text_with(
            text_rect,
            text,
            9.0,
            false,
            color,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }

    /// Обычная подпись окна.
    pub fn label(&self, rect: RECT, text: &str, points: f32, color: COLORREF) {
        self.text_with(
            rect,
            text,
            points,
            false,
            color,
            DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX,
        );
    }

    pub fn label_bold(&self, rect: RECT, text: &str, points: f32, color: COLORREF) {
        self.text_with(
            rect,
            text,
            points,
            true,
            color,
            DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX,
        );
    }

    /// Многострочная подсказка по центру.
    pub fn hint(&self, rect: RECT, text: &str, points: f32, color: COLORREF) {
        self.text_centered_wrapped(rect, text, points, false, color);
    }

    /// Рамка окна подсказки с закруглённым фоном.
    pub fn tooltip_frame(&self, rect: RECT) {
        self.fill(rect, rgb(45, 45, 45));
        self.frame(rect, rgb(80, 80, 80));
    }

    pub fn surface(&self, rect: RECT) {
        self.fill(rect, SURFACE);
    }

    pub fn text_primary(&self) -> COLORREF {
        TEXT_PRIMARY
    }

    pub fn text_muted(&self) -> COLORREF {
        TEXT_MUTED
    }

    pub fn text_dim(&self) -> COLORREF {
        TEXT_DIM
    }

    /// Рамка заглушки для отрисовки картинки из DIB.
    pub fn image_frame(&self, rect: RECT, color: COLORREF) {
        self.frame(rect, color);
    }
}

/// Прямоугольник из четырёх чисел — удобно задавать раскладку в коде окна.
pub fn rect(left: i32, top: i32, width: i32, height: i32) -> RECT {
    RECT { left, top, right: left + width, bottom: top + height }
}

/// Лежит ли точка внутри прямоугольника.
pub fn contains(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}