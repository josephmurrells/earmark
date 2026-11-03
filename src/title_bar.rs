//! Colours the native Windows title bar to match the app, and draws the app icon.
//!
//! The window keeps its normal frame, so snap layouts, resizing and keyboard
//! shortcuts behave as usual; only the caption, text and border colours change.

use egui::{Color32, IconData};

use crate::theme::Palette;

/// Keeps the title bar's colours in step with the app's theme.
#[derive(Debug, Default)]
pub struct TitleBar {
    /// The last (dark, focus) state applied, so colours are only set on change.
    applied: Option<(bool, bool)>,
}

impl TitleBar {
    /// Recolours the title bar if the theme or mode changed since the last call.
    ///
    /// Normally the title bar matches the step header, so the two read as one
    /// band. In `focus` mode (the hearing test) there's no header, so it
    /// matches the page background instead.
    pub fn update(&mut self, frame: &eframe::Frame, dark: bool, focus: bool) {
        if self.applied == Some((dark, focus)) {
            return;
        }
        let palette = if dark { Palette::DARK } else { Palette::LIGHT };
        let caption = if focus {
            palette.background
        } else {
            palette.surface
        };
        if set_colors(frame, dark, caption, palette.text, palette.border) {
            self.applied = Some((dark, focus));
        }
    }
}

/// Sets the caption, text and border colours. Returns false if the window
/// handle wasn't available or Windows rejected the change (Windows 10 doesn't
/// support caption colours, and keeps its default title bar).
#[cfg(windows)]
fn set_colors(
    frame: &eframe::Frame,
    dark: bool,
    caption: Color32,
    text: Color32,
    border: Color32,
) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    };

    let Ok(handle) = frame.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = win32.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    // Dark mode also gives dark caption buttons, in case colours aren't supported.
    let dark_mode_ok = set_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(dark));
    let colors_ok = [
        (DWMWA_CAPTION_COLOR, caption),
        (DWMWA_TEXT_COLOR, text),
        (DWMWA_BORDER_COLOR, border),
    ]
    .into_iter()
    .all(|(attribute, color)| set_attribute(hwnd, attribute, colorref(color)));
    dark_mode_ok || colors_ok
}

#[cfg(not(windows))]
fn set_colors(
    _frame: &eframe::Frame,
    _dark: bool,
    _caption: Color32,
    _text: Color32,
    _border: Color32,
) -> bool {
    true
}

/// Sets one 32-bit DWM window attribute. Returns true on success.
#[cfg(windows)]
fn set_attribute(hwnd: windows_sys::Win32::Foundation::HWND, attribute: i32, value: u32) -> bool {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;

    // SAFETY: `hwnd` comes from eframe's live window handle, borrowed for the
    // duration of this call. `value` is a valid u32 on the stack, and every
    // attribute passed here (dark mode BOOL, COLORREFs) is documented as a
    // 4-byte value, matching the size passed.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute as u32,
            std::ptr::from_ref(&value).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    result >= 0
}

/// Converts a colour to a Windows COLORREF (0x00BBGGRR).
#[cfg(windows)]
fn colorref(color: Color32) -> u32 {
    u32::from(color.r()) | (u32::from(color.g()) << 8) | (u32::from(color.b()) << 16)
}

/// Sizes written to `assets/earmark.ico`, the icon embedded in `earmark.exe`.
#[cfg(test)]
const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// Draws the window icon.
#[must_use]
pub fn app_icon() -> IconData {
    app_icon_sized(256)
}

/// Draws the app icon at `size` pixels square: an indigo rounded square with
/// three white EQ bars.
#[must_use]
fn app_icon_sized(icon_size: u32) -> IconData {
    let accent = Palette::LIGHT.accent;
    let size = icon_size as f32;
    let mut rgba = Vec::with_capacity((icon_size * icon_size * 4) as usize);
    // (centre x, half height) of each bar, in units of the icon size.
    let bars = [(0.30, 0.16), (0.50, 0.28), (0.70, 0.20)];
    for y in 0..icon_size {
        for x in 0..icon_size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let square = coverage(rounded_rect_distance(
                px,
                py,
                size / 2.0,
                size / 2.0,
                size / 2.0,
                size / 2.0,
                size * 0.22,
            ));
            let bar = bars
                .iter()
                .map(|&(cx, half_height)| {
                    coverage(rounded_rect_distance(
                        px,
                        py,
                        cx * size,
                        size / 2.0,
                        size * 0.055,
                        half_height * size,
                        size * 0.055,
                    ))
                })
                .fold(0.0, f32::max);
            let mix =
                |background: u8| (f32::from(background) * (1.0 - bar) + 255.0 * bar).round() as u8;
            rgba.extend_from_slice(&[
                mix(accent.r()),
                mix(accent.g()),
                mix(accent.b()),
                (square * 255.0).round() as u8,
            ]);
        }
    }
    IconData {
        rgba,
        width: icon_size,
        height: icon_size,
    }
}

/// Encodes icons as a Windows `.ico` file with 32-bit BMP entries.
#[cfg(test)]
fn encode_ico(icons: &[IconData]) -> Vec<u8> {
    let header_len = 6 + 16 * icons.len();
    let mut entries = Vec::new();
    let mut images = Vec::new();
    for icon in icons {
        let (width, height) = (icon.width as usize, icon.height as usize);
        let mut image = Vec::new();
        // BITMAPINFOHEADER; the height counts the colour rows plus the mask rows.
        image.extend_from_slice(&40u32.to_le_bytes());
        image.extend_from_slice(&(width as i32).to_le_bytes());
        image.extend_from_slice(&((height * 2) as i32).to_le_bytes());
        image.extend_from_slice(&1u16.to_le_bytes());
        image.extend_from_slice(&32u16.to_le_bytes());
        image.extend_from_slice(&[0; 24]);
        // Rows are stored bottom-up, as BGRA.
        for y in (0..height).rev() {
            for x in 0..width {
                let i = (y * width + x) * 4;
                let [r, g, b, a] = [
                    icon.rgba[i],
                    icon.rgba[i + 1],
                    icon.rgba[i + 2],
                    icon.rgba[i + 3],
                ];
                image.extend_from_slice(&[b, g, r, a]);
            }
        }
        // The AND mask is unused with an alpha channel, but must be present.
        let mask_row = width.div_ceil(32) * 4;
        image.resize(image.len() + mask_row * height, 0);

        let offset = header_len + images.len();
        // A width or height of 256 is written as 0.
        entries.push(width as u8);
        entries.push(height as u8);
        entries.extend_from_slice(&[0, 0]);
        entries.extend_from_slice(&1u16.to_le_bytes());
        entries.extend_from_slice(&32u16.to_le_bytes());
        entries.extend_from_slice(&(image.len() as u32).to_le_bytes());
        entries.extend_from_slice(&(offset as u32).to_le_bytes());
        images.extend_from_slice(&image);
    }
    let mut file = Vec::with_capacity(header_len + images.len());
    file.extend_from_slice(&[0, 0, 1, 0]);
    file.extend_from_slice(&(icons.len() as u16).to_le_bytes());
    file.extend_from_slice(&entries);
    file.extend_from_slice(&images);
    file
}

/// Returns the signed distance from a point to a rounded rectangle centred at
/// (`cx`, `cy`) with half-size (`hx`, `hy`) and corner `radius`.
fn rounded_rect_distance(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, radius: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - radius);
    let qy = (py - cy).abs() - (hy - radius);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    outside + qx.max(qy).min(0.0) - radius
}

/// Turns a signed distance into anti-aliased pixel coverage.
fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_transparent_corners_and_white_bars() {
        let icon = app_icon_sized(64);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        let pixel = |x: u32, y: u32| {
            let i = ((y * icon.width + x) * 4) as usize;
            [
                icon.rgba[i],
                icon.rgba[i + 1],
                icon.rgba[i + 2],
                icon.rgba[i + 3],
            ]
        };
        assert_eq!(pixel(0, 0)[3], 0, "corner should be transparent");
        assert_eq!(
            pixel(32, 32),
            [255, 255, 255, 255],
            "middle bar should be white"
        );
        let accent = Palette::LIGHT.accent;
        assert_eq!(pixel(8, 32), [accent.r(), accent.g(), accent.b(), 255]);
    }

    /// Regenerates `assets/earmark.ico`, which `build.rs` embeds in the exe.
    /// Run after changing the icon: `cargo test write_ico -- --ignored`.
    #[test]
    #[ignore = "writes assets/earmark.ico"]
    fn write_ico() {
        let icons: Vec<IconData> = ICO_SIZES.iter().map(|&size| app_icon_sized(size)).collect();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/earmark.ico");
        std::fs::create_dir_all(path.parent().expect("assets folder")).expect("create assets");
        std::fs::write(path, encode_ico(&icons)).expect("write icon");
    }

    #[test]
    fn ico_has_an_entry_per_size() {
        let icons: Vec<IconData> = [16, 256].iter().map(|&size| app_icon_sized(size)).collect();
        let ico = encode_ico(&icons);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(ico[6], 16, "first entry is 16 px");
        assert_eq!(ico[22], 0, "256 px is written as 0");
        let second_offset = u32::from_le_bytes([ico[34], ico[35], ico[36], ico[37]]) as usize;
        let first_len = u32::from_le_bytes([ico[14], ico[15], ico[16], ico[17]]) as usize;
        assert_eq!(second_offset, 6 + 32 + first_len);
        assert_eq!(ico.len(), second_offset + 40 + 256 * 256 * 4 + 32 * 256);
    }

    #[cfg(windows)]
    #[test]
    fn colorref_is_bgr() {
        assert_eq!(colorref(Color32::from_rgb(0x11, 0x22, 0x33)), 0x0033_2211);
    }
}
