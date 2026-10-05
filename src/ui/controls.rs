//! Модель кнопок и других элементов окна.
//!
//! Окна рисуются вручную, поэтому элементы — это обычные данные: прямоугольник,
//! вид и обработчик. Так же устроены и подписи, и флажок.

use windows::Win32::Foundation::{COLORREF, RECT};

use crate::ui::paint::{self, Painter};

/// Что делает нажатый элемент.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Выбор файла через диалог открытия.
    PickFile,
    /// Вставка картинки из буфера обмена.
    Paste,
    /// Диалог выбора произвольного цвета.
    PickColor,
    /// Установка одного из готовых цветов (индекс в списке пресетов).
    PresetColor(usize),
    /// Установка обоев.
    Apply,
    /// Полный откат с подтверждением.
    Revert,
    /// Переключение темы Windows.
    ToggleTheme,
    /// Уменьшение и восстановление панели задач.
    ToggleTaskbar,
    /// Открепление Edge и выбор браузера по умолчанию.
    DetachEdge,
    /// Включение и выключение автозапуска.
    ToggleAutoStart,
    /// Закрытие окна диалога выбора браузера.
    ClosePicker,
}

#[derive(Debug, Clone)]
pub enum Control {
    Button {
        rect: RECT,
        text: String,
        action: Action,
        primary: bool,
        enabled: bool,
        tooltip: Option<String>,
        points: f32,
    },
    /// Квадратный образец цвета.
    ColorSwatch {
        rect: RECT,
        color: COLORREF,
        action: Action,
        tooltip: Option<String>,
    },
    Checkbox {
        rect: RECT,
        text: String,
        action: Action,
        checked: bool,
    },
    /// Зона перетаскивания картинки: рамка, превью и подсказка.
    DropZone {
        rect: RECT,
    },
    /// Обычная подпись; `centered` — выравнивание по центру.
    Label {
        rect: RECT,
        text: String,
        points: f32,
        color: LabelColor,
        bold: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelColor {
    Primary,
    Muted,
    Dim,
}

impl Control {
    pub fn rect(&self) -> RECT {
        match self {
            Control::Button { rect, .. }
            | Control::ColorSwatch { rect, .. }
            | Control::Checkbox { rect, .. }
            | Control::DropZone { rect }
            | Control::Label { rect, .. } => *rect,
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        paint::contains(self.rect(), x, y)
    }

    /// Нажимается ли элемент в текущем состоянии.
    pub fn is_enabled(&self) -> bool {
        match self {
            Control::Button { enabled, .. } => *enabled,
            _ => true,
        }
    }

    /// Действие элемента. У зон перетаскивания нажатие означает выбор файла.
    pub fn action(&self) -> Option<Action> {
        match self {
            Control::Button { action, .. }
            | Control::ColorSwatch { action, .. }
            | Control::Checkbox { action, .. } => Some(*action),
            Control::DropZone { .. } => Some(Action::PickFile),
            Control::Label { .. } => None,
        }
    }

    pub fn tooltip(&self) -> Option<&str> {
        match self {
            Control::Button { tooltip, .. } | Control::ColorSwatch { tooltip, .. } => {
                tooltip.as_deref()
            }
            _ => None,
        }
    }

    /// Отрисовка элемента.
    pub fn paint(
        &self,
        painter: &Painter,
        hovered: bool,
        pressed: bool,
        state: &ControlState<'_>,
    ) {
        match self {
            Control::Button { rect, text, primary, enabled, points, .. } => painter.button(
                *rect,
                text,
                *primary,
                hovered,
                pressed,
                *enabled,
                *points,
                *primary,
            ),
            Control::ColorSwatch { rect, color, .. } => painter.color_swatch(*rect, *color),
            Control::Checkbox { rect, text, checked, .. } => {
                painter.checkbox(*rect, text, *checked, hovered)
            }
            Control::DropZone { rect } => state.draw_drop_zone(*rect, painter),
            Control::Label { rect, text, points, color, bold } => {
                let color = match color {
                    LabelColor::Primary => painter.text_primary(),
                    LabelColor::Muted => painter.text_muted(),
                    LabelColor::Dim => painter.text_dim(),
                };
                if *bold {
                    painter.label_bold(*rect, text, *points, color);
                } else {
                    painter.label(*rect, text, *points, color);
                }
            }
        }
    }
}

/// Данные, нужные окну при отрисовке, но не входящие в сами элементы.
pub struct ControlState<'a> {
    /// Превью выбранной картинки.
    pub preview: Option<&'a image::DynamicImage>,
    /// Показывать подсказку вместо картинки.
    pub hint_visible: bool,
    /// Фон окна.
    pub background: COLORREF,
    /// Цвет рамки зоны перетаскивания.
    pub drop_border: COLORREF,
}

impl ControlState<'_> {
    /// Рисует зону перетаскивания: рамка, фон, превью или подсказку.
    pub fn draw_drop_zone(&self, rect: RECT, painter: &Painter) {
        painter.surface(rect);

        if let Some(preview) = self.preview {
            draw_image_cover(painter, rect, preview);
        } else if self.hint_visible {
            painter.hint(
                rect,
                "Перетащите картинку сюда\r\nили выберите файл / сплошной цвет ниже\r\n\r\n\
                 Ctrl+V — вставить из буфера обмена\r\nJPG, PNG, BMP, GIF, TIFF",
                9.5,
                painter.text_dim(),
            );
        }

        painter.image_frame(rect, self.drop_border);
    }
}

/// Рисует картинку «по размеру окна» с обрезкой — как `PictureBoxSizeMode.Zoom`.
fn draw_image_cover(painter: &Painter, rect: RECT, image: &image::DynamicImage) {
    use windows::Win32::Graphics::Gdi::{
        SetDIBitsToDevice, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    };

    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 0 || height <= 0 {
        return;
    }

    // Рисуем превью сразу в масштабе окна: так картинка не мылится и не тормозит.
    let preview_width = width.min(960);
    let preview_height = (preview_width as u64 * image.height() as u64 / image.width().max(1) as u64)
        .max(1) as i32;
    let preview_height = preview_height.min(height);

    let resized = image.resize_exact(
        preview_width as u32,
        preview_height as u32,
        image::imageops::FilterType::Triangle,
    );

    let pixels: Vec<u8> = resized
        .to_rgba8()
        .pixels()
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: preview_width,
            biHeight: -preview_height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            ..Default::default()
        },
        ..Default::default()
    };

    unsafe {
        SetDIBitsToDevice(
            painter.hdc(),
            rect.left,
            rect.top,
            preview_width as u32,
            preview_height as u32,
            0,
            0,
            0,
            preview_height as u32,
            pixels.as_ptr().cast(),
            &info,
            DIB_RGB_COLORS,
        );
    }
}

/// Утилита для сборки прямоугольника из логических координат.
pub fn rect(left: i32, top: i32, width: i32, height: i32) -> RECT {
    paint::rect(left, top, width, height)
}