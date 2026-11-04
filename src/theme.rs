//! The app's look: colours for light and dark mode, Segoe fonts, Fluent icons,
//! and the egui style built from them.

use std::collections::BTreeMap;
use std::fs;

use egui::epaint::text::{FontInsert, InsertFontFamily};
use egui::epaint::{CornerRadius, Shadow};
use egui::{
    Color32, Context, FontData, FontFamily, FontId, Margin, Stroke, Style, TextStyle, Theme, Ui,
    Visuals,
};

/// Corner radius of cards, in points.
pub const CARD_RADIUS: u8 = 12;
/// Corner radius of buttons and inputs, in points.
pub const CONTROL_RADIUS: u8 = 8;

/// Display text size (screen titles).
pub const DISPLAY_SIZE: f32 = 28.0;
/// Title text size (card titles).
pub const TITLE_SIZE: f32 = 18.0;
/// Body text size.
pub const BODY_SIZE: f32 = 14.0;
/// Caption text size.
pub const CAPTION_SIZE: f32 = 12.5;

/// Named font family for semibold text.
const SEMIBOLD: &str = "semibold";
/// Named font family for Fluent icons.
const ICONS: &str = "icons";

/// Colours for one theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Window background.
    pub background: Color32,
    /// Cards and raised surfaces.
    pub surface: Color32,
    /// Inputs, secondary buttons, and inset areas.
    pub surface_alt: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    /// Text and icons drawn on the accent colour.
    pub on_accent: Color32,
    /// A faint accent tint for selected backgrounds.
    pub accent_soft: Color32,
    pub left_ear: Color32,
    pub right_ear: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
}

impl Palette {
    /// The light-mode palette.
    pub const LIGHT: Palette = Palette {
        background: Color32::from_rgb(246, 247, 251),
        surface: Color32::from_rgb(255, 255, 255),
        surface_alt: Color32::from_rgb(239, 241, 247),
        border: Color32::from_rgb(226, 229, 238),
        text: Color32::from_rgb(27, 29, 41),
        text_muted: Color32::from_rgb(104, 109, 130),
        accent: Color32::from_rgb(79, 70, 229),
        accent_hover: Color32::from_rgb(67, 56, 202),
        on_accent: Color32::WHITE,
        accent_soft: Color32::from_rgb(238, 237, 253),
        left_ear: Color32::from_rgb(37, 125, 235),
        right_ear: Color32::from_rgb(230, 92, 76),
        success: Color32::from_rgb(22, 150, 90),
        warning: Color32::from_rgb(196, 125, 10),
        error: Color32::from_rgb(210, 50, 60),
    };

    /// The dark-mode palette.
    pub const DARK: Palette = Palette {
        background: Color32::from_rgb(17, 18, 24),
        surface: Color32::from_rgb(26, 28, 36),
        surface_alt: Color32::from_rgb(35, 38, 48),
        border: Color32::from_rgb(45, 48, 60),
        text: Color32::from_rgb(236, 237, 243),
        text_muted: Color32::from_rgb(150, 155, 175),
        accent: Color32::from_rgb(110, 104, 245),
        accent_hover: Color32::from_rgb(129, 124, 248),
        on_accent: Color32::WHITE,
        accent_soft: Color32::from_rgb(40, 40, 72),
        left_ear: Color32::from_rgb(96, 165, 250),
        right_ear: Color32::from_rgb(251, 128, 114),
        success: Color32::from_rgb(52, 200, 130),
        warning: Color32::from_rgb(240, 180, 60),
        error: Color32::from_rgb(248, 100, 110),
    };

    /// Returns the palette for the theme `ui` is drawn in.
    #[must_use]
    pub fn of(ui: &Ui) -> Palette {
        if ui.visuals().dark_mode {
            Palette::DARK
        } else {
            Palette::LIGHT
        }
    }
}

/// Glyphs from the Segoe Fluent Icons font, checked against a rendered sheet.
pub mod icon {
    pub const ADD: &str = "\u{E710}";
    pub const MUSIC: &str = "\u{EC4F}";
    pub const PLAY: &str = "\u{E768}";
    pub const PAUSE: &str = "\u{E769}";
    pub const REPLAY: &str = "\u{E72C}";
    pub const BACK: &str = "\u{E72B}";
    pub const CHECK: &str = "\u{E73E}";
    pub const CLOSE: &str = "\u{E711}";
    pub const COPY: &str = "\u{E8C8}";
    pub const SAVE: &str = "\u{E74E}";
    pub const OPEN_FILE: &str = "\u{E8E5}";
    pub const VOLUME: &str = "\u{E767}";
    pub const WARNING: &str = "\u{E7BA}";
    pub const INFO: &str = "\u{E946}";
    pub const CHEVRON_DOWN: &str = "\u{E70D}";
    pub const CHEVRON_RIGHT: &str = "\u{E76C}";
    pub const SLIDERS: &str = "\u{E9E9}";
    pub const PHONE: &str = "\u{E8EA}";
    pub const DOWNLOAD: &str = "\u{E896}";
    pub const EAR: &str = "\u{F270}";
    pub const SYNC: &str = "\u{E895}";
    pub const COMPLETED: &str = "\u{E930}";
}

/// Returns a font for body text at `size`.
#[must_use]
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Returns a semibold font at `size` (falls back to regular if Segoe UI is missing).
#[must_use]
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Returns the icon font at `size`.
#[must_use]
pub fn icons(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

/// Installs fonts and the light and dark styles.
pub fn install(ctx: &Context) {
    install_fonts(ctx);
    ctx.set_style_of(Theme::Light, style(Palette::LIGHT, false));
    ctx.set_style_of(Theme::Dark, style(Palette::DARK, true));
}

/// Installs the theme the first time it's called for `ctx`, for test
/// harnesses that have no creation hook.
#[cfg(test)]
pub fn install_once(ctx: &Context) {
    let id = egui::Id::unique("earmark-theme-installed");
    if !ctx.data(|data| data.get_temp::<bool>(id).unwrap_or(false)) {
        install(ctx);
        ctx.data_mut(|data| data.insert_temp(id, true));
    }
}

fn install_fonts(ctx: &Context) {
    let fonts_dir = std::env::var_os("WINDIR")
        .map(|dir| std::path::PathBuf::from(dir).join("Fonts"))
        .unwrap_or_default();
    let load = |file: &str| {
        fs::read(fonts_dir.join(file))
            .ok()
            .map(FontData::from_owned)
    };

    // Semibold first, so the named family exists even if only fallbacks load.
    let semibold_family = FontFamily::Name(SEMIBOLD.into());
    let icons_family = FontFamily::Name(ICONS.into());
    let mut inserts = Vec::new();
    if let Some(data) = load("segoeui.ttf") {
        inserts.push(FontInsert::new(
            "Segoe UI",
            data,
            vec![InsertFontFamily {
                family: FontFamily::Proportional,
                priority: egui::epaint::text::FontPriority::Highest,
            }],
        ));
    }
    if let Some(data) = load("seguisb.ttf") {
        inserts.push(FontInsert::new(
            "Segoe UI Semibold",
            data,
            vec![InsertFontFamily {
                family: semibold_family.clone(),
                priority: egui::epaint::text::FontPriority::Highest,
            }],
        ));
    }
    // Symbols such as arrows and ± that Segoe UI lacks.
    if let Some(data) = load("seguisym.ttf") {
        inserts.push(FontInsert::new(
            "Segoe UI Symbol",
            data,
            vec![
                InsertFontFamily {
                    family: FontFamily::Proportional,
                    priority: egui::epaint::text::FontPriority::Lowest,
                },
                InsertFontFamily {
                    family: semibold_family.clone(),
                    priority: egui::epaint::text::FontPriority::Lowest,
                },
            ],
        ));
    }
    let icon_data = load("SegoeIcons.ttf").or_else(|| load("segmdl2.ttf"));
    if let Some(data) = icon_data {
        inserts.push(FontInsert::new(
            "Segoe Fluent Icons",
            data,
            vec![InsertFontFamily {
                family: icons_family.clone(),
                priority: egui::epaint::text::FontPriority::Highest,
            }],
        ));
    }

    // Named families must exist before fonts are inserted into them; seed them
    // with egui's default font so text still renders without Windows fonts.
    let mut definitions = egui::FontDefinitions::default();
    let defaults = definitions
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    definitions
        .families
        .insert(semibold_family, defaults.clone());
    definitions.families.insert(icons_family, defaults);
    ctx.set_fonts(definitions);
    for insert in inserts {
        ctx.add_font(insert);
    }
}

fn style(palette: Palette, dark: bool) -> Style {
    let mut style = Style::default();
    let text_styles: BTreeMap<TextStyle, FontId> = [
        (TextStyle::Small, regular(CAPTION_SIZE)),
        (TextStyle::Body, regular(BODY_SIZE)),
        (TextStyle::Button, regular(BODY_SIZE)),
        (TextStyle::Heading, semibold(TITLE_SIZE)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    style.text_styles = text_styles;

    let spacing = &mut style.spacing;
    spacing.item_spacing = egui::vec2(8.0, 8.0);
    spacing.button_padding = egui::vec2(14.0, 7.0);
    spacing.interact_size.y = 30.0;
    spacing.slider_rail_height = 6.0;
    spacing.combo_width = 200.0;
    spacing.menu_margin = Margin::same(8);

    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let control = CornerRadius::same(CONTROL_RADIUS);
    visuals.panel_fill = palette.background;
    visuals.window_fill = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.border);
    visuals.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    visuals.menu_corner_radius = control;
    visuals.extreme_bg_color = palette.surface_alt;
    visuals.faint_bg_color = palette.surface_alt;
    visuals.text_edit_bg_color = Some(palette.surface_alt);
    visuals.hyperlink_color = palette.accent;
    visuals.warn_fg_color = palette.warning;
    visuals.error_fg_color = palette.error;
    visuals.override_text_color = None;
    visuals.selection.bg_fill = palette.accent;
    visuals.selection.stroke = Stroke::new(1.0, palette.on_accent);
    visuals.slider_trailing_fill = true;
    visuals.collapsing_header_frame = false;
    visuals.indent_has_left_vline = false;
    visuals.striped = false;
    let shadow_alpha = if dark { 90 } else { 28 };
    visuals.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    visuals.window_shadow = visuals.popup_shadow;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = palette.surface;
    widgets.noninteractive.weak_bg_fill = palette.surface;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
    widgets.noninteractive.corner_radius = control;
    for (state, fill, border) in [
        (&mut widgets.inactive, palette.surface_alt, palette.border),
        (&mut widgets.hovered, palette.border, palette.text_muted),
        (&mut widgets.active, palette.border, palette.accent),
        (&mut widgets.open, palette.surface_alt, palette.accent),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, border);
        state.fg_stroke = Stroke::new(1.5, palette.text);
        state.corner_radius = control;
        state.expansion = 0.0;
    }
    widgets.inactive.bg_stroke = Stroke::NONE;
    // Slider rails and unchecked boxes use `inactive.bg_fill`.
    widgets.inactive.bg_fill = palette.border;
    style.visuals = visuals;
    style
}

/// Returns `color` with its alpha scaled by `opacity` (0 to 1).
#[must_use]
pub fn fade(color: Color32, opacity: f32) -> Color32 {
    color.gamma_multiply(opacity.clamp(0.0, 1.0))
}
