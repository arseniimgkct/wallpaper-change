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
    Muted,
    Dim,
}

impl Control {
    /// Логический прямоугольник элемента: раскладка задана в координатах
    /// 96 dpi, масштаб под экран применяется при отрисовке и при проверке
    /// попадания мыши.
    pub fn rect(&self) -> RECT {
        match self {
            Control::Button { rect, .. }
            | Control::ColorSwatch { rect, .. }
            | Control::Checkbox { rect, .. }
            | Control::DropZone { rect }
            | Control::Label { rect, .. } => *rect,
        }
    }

    /// Текст подсказки для этого элемента, если он задан.
    pub fn tooltip(&self) -> Option<&str> {
        match self {
            Control::Button { tooltip, .. } | Control::ColorSwatch { tooltip, .. } => {
                tooltip.as_deref()
            }
            _ => None,
        }
    }


    /// Прямоугольник элемента в пикселях окна.
    ///
    /// Раскладка задана в логических координатах (как в оригинале на 96 dpi),
    /// поэтому масштаб под экран применяется здесь — и в отрисовке, и в
    /// проверке попадания мыши, иначе кнопки не совпали бы с картинкой.
    pub fn scaled_rect(&self, dpi: i32) -> RECT {
        paint::scale_rect(self.rect(), dpi)
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

    /// Отрисовка элемента.
    pub fn paint(
        &self,
        painter: &Painter,
        hovered: bool,
        pressed: bool,
        state: &ControlState<'_>,
    ) {
        let dpi = painter.dpi();

        match self {
            Control::Button { rect, text, primary, enabled, points, .. } => painter.button(
                paint::scale_rect(*rect, dpi),
                text,
                *primary,
                hovered,
                pressed,
                *enabled,
                *points,
                *primary,
            ),
            Control::ColorSwatch { rect, color, .. } => {
                painter.color_swatch(paint::scale_rect(*rect, dpi), *color)
            }
            Control::Checkbox { rect, text, checked, .. } => {
                painter.checkbox(paint::scale_rect(*rect, dpi), text, *checked, hovered)
            }
            Control::DropZone { rect } => {
                state.draw_drop_zone(paint::scale_rect(*rect, dpi), painter)
            }
            Control::Label { rect, text, points, color, bold } => {
                let color = match color {
                    LabelColor::Muted => painter.text_muted(),
                    LabelColor::Dim => painter.text_dim(),
                };
                let rect = paint::scale_rect(*rect, dpi);
                if *bold {
                    painter.label_bold(rect, text, *points, color);
                } else {
                    painter.label(rect, text, *points, color);
                }
            }
        }
    }
}

/// Данные, нужные окну при отрисовке, но не входящие в сами элементы.
pub struct ControlState<'a> {
    /// Превью выбранной картинки, уже пересчитанное в BGRA.
    pub preview: Option<&'a Preview>,
    /// Показывать подсказку вместо картинки.
    pub hint_visible: bool,
    /// Цвет рамки зоны перетаскивания.
    pub drop_border: COLORREF,
}

impl ControlState<'_> {
    /// Рисует зону перетаскивания: рамка, фон, превью или подсказку.
    pub fn draw_drop_zone(&self, rect: RECT, painter: &Painter) {
        painter.surface(rect);

        if let Some(preview) = self.preview {
            preview.blit(painter, rect);
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

/// Готовое превью: картинка, один раз пересчитанная в BGRA под размер окна.
///
/// Пересчёт на каждый кадр — самая дорогая операция в окне: картинка
/// растягивается и переводится в BGRA заново при каждом движении мыши,
/// из-за чего кнопки «подтормаживают» под курсором. Поэтому превью
/// пересчитывается только когда сменилась картинка или размер окна.
pub struct Preview {
    bgra: Vec<u8>,
    pub width: i32,
    pub height: i32,
}

impl Preview {
    /// Пересчитывает превью под размер зоны перетаскивания.
    ///
    /// `None`, если вписать картинку не во что: пустой прямоугольник.
    pub fn build(image: &image::DynamicImage, rect: RECT) -> Option<Preview> {
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        if width <= 0 || height <= 0 {
            return None;
        }

        // Держим превью не крупнее зоны: иначе на каждый кадр гоняем мегапиксели.
        let preview_width = width.min(960);
        let preview_height = (preview_width as u64 * image.height() as u64
            / image.width().max(1) as u64)
            .max(1) as i32;
        let preview_height = preview_height.min(height);

        let resized = image.resize_exact(
            preview_width as u32,
            preview_height as u32,
            image::imageops::FilterType::Triangle,
        );

        let bgra = resized
            .to_rgba8()
            .pixels()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect();

        Some(Preview { bgra, width: preview_width, height: preview_height })
    }

    /// Копирует превью в прямоугольник `rect`.
    pub fn blit(&self, painter: &Painter, rect: RECT) {
        use windows::Win32::Graphics::Gdi::{
            SetDIBitsToDevice, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
        };

        if self.width <= 0 || self.height <= 0 {
            return;
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: self.width,
                biHeight: -self.height,
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
                self.width as u32,
                self.height as u32,
                0,
                0,
                0,
                self.height as u32,
                self.bgra.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
            );
        }
    }
}
