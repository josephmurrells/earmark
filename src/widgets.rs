//! Custom widgets in the app's visual style.

use egui::epaint::CornerRadius;
use egui::{
    Align2, Color32, InnerResponse, Margin, Pos2, Rect, Response, Role, Sense, Stroke, StrokeKind,
    Ui, Vec2, WidgetInfo,
};

use crate::theme::{self, Palette, icon};

/// Space between cards, in points.
pub const CARD_GAP: f32 = 12.0;

/// Draws a full-width card.
pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let palette = Palette::of(ui);
    let response = egui::Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(theme::CARD_RADIUS))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        });
    ui.add_space(CARD_GAP);
    response
}

/// Draws a screen title with an optional muted subtitle.
pub fn screen_title(ui: &mut Ui, title: &str, subtitle: Option<&str>) {
    let palette = Palette::of(ui);
    ui.label(
        egui::RichText::new(title)
            .font(theme::semibold(theme::DISPLAY_SIZE))
            .color(palette.text),
    );
    if let Some(subtitle) = subtitle {
        ui.label(egui::RichText::new(subtitle).color(palette.text_muted));
    }
    ui.add_space(12.0);
}

/// Draws a card title, with an optional step number in a circle before it.
pub fn card_title(ui: &mut Ui, number: Option<u32>, title: &str) {
    let palette = Palette::of(ui);
    ui.horizontal(|ui| {
        if let Some(number) = number {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 12.0, palette.accent_soft);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                number.to_string(),
                theme::semibold(13.0),
                palette.accent,
            );
            ui.add_space(4.0);
        }
        ui.label(
            egui::RichText::new(title)
                .font(theme::semibold(theme::TITLE_SIZE))
                .color(palette.text),
        );
    });
}

/// Draws muted, smaller text.
pub fn caption(ui: &mut Ui, text: impl Into<String>) -> Response {
    let palette = Palette::of(ui);
    ui.label(
        egui::RichText::new(text.into())
            .size(theme::CAPTION_SIZE)
            .color(palette.text_muted),
    )
}

/// Draws muted body text.
pub fn muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    let palette = Palette::of(ui);
    ui.label(egui::RichText::new(text.into()).color(palette.text_muted))
}

/// How a button is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Filled with the accent colour: the screen's main action.
    Primary,
    /// A quieter filled button.
    Secondary,
    /// No background until hovered.
    Ghost,
}

/// Options for [`button`].
#[derive(Debug, Clone, Copy)]
pub struct ButtonOptions<'a> {
    pub kind: ButtonKind,
    pub icon: Option<&'a str>,
    pub enabled: bool,
    /// Minimum size; the button grows to fit its content.
    pub min_size: Vec2,
    pub text_size: f32,
}

impl Default for ButtonOptions<'_> {
    fn default() -> Self {
        Self {
            kind: ButtonKind::Secondary,
            icon: None,
            enabled: true,
            min_size: Vec2::new(0.0, 36.0),
            text_size: theme::BODY_SIZE,
        }
    }
}

/// Draws a button with an optional icon before its text.
pub fn button(ui: &mut Ui, text: &str, options: ButtonOptions<'_>) -> Response {
    let palette = Palette::of(ui);
    let text_color = match options.kind {
        ButtonKind::Primary => palette.on_accent,
        ButtonKind::Secondary | ButtonKind::Ghost => palette.text,
    };
    let font = if options.kind == ButtonKind::Primary {
        theme::semibold(options.text_size)
    } else {
        theme::regular(options.text_size)
    };
    let painter = ui.painter();
    let text_galley = painter.layout_no_wrap(text.to_owned(), font, text_color);
    let icon_galley = options.icon.map(|glyph| {
        painter.layout_no_wrap(
            glyph.to_owned(),
            theme::icons(options.text_size + 1.0),
            text_color,
        )
    });
    let padding = Vec2::new(if text.is_empty() { 10.0 } else { 18.0 }, 8.0);
    let gap = if icon_galley.is_some() && !text.is_empty() {
        8.0
    } else {
        0.0
    };
    let icon_width = icon_galley.as_ref().map_or(0.0, |g| g.size().x);
    let content = Vec2::new(
        icon_width + gap + text_galley.size().x,
        text_galley
            .size()
            .y
            .max(icon_galley.as_ref().map_or(0.0, |g| g.size().y)),
    );
    let size = (content + padding * 2.0).max(options.min_size);
    let sense = if options.enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, options.enabled, text));

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() && options.enabled;
        let pressed = response.is_pointer_button_down_on() && options.enabled;
        let fill = match options.kind {
            ButtonKind::Primary if pressed => palette.accent_hover,
            ButtonKind::Primary if hovered => palette.accent_hover,
            ButtonKind::Primary => palette.accent,
            ButtonKind::Secondary if hovered || pressed => palette.border,
            ButtonKind::Secondary => palette.surface_alt,
            ButtonKind::Ghost if hovered || pressed => palette.surface_alt,
            ButtonKind::Ghost => Color32::TRANSPARENT,
        };
        let opacity = if options.enabled { 1.0 } else { 0.45 };
        let radius = CornerRadius::same(theme::CONTROL_RADIUS + 2);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, theme::fade(fill, opacity));
        if response.has_focus() {
            painter.rect_stroke(
                rect.expand(2.0),
                radius,
                Stroke::new(2.0, palette.accent),
                StrokeKind::Outside,
            );
        }
        let mut x = rect.center().x - content.x / 2.0;
        if let Some(galley) = icon_galley {
            let pos = Pos2::new(x, rect.center().y - galley.size().y / 2.0);
            let width = galley.size().x;
            painter.galley(pos, galley, theme::fade(text_color, opacity));
            x += width + gap;
        }
        let pos = Pos2::new(x, rect.center().y - text_galley.size().y / 2.0);
        painter.galley(pos, text_galley, theme::fade(text_color, opacity));
    }
    response
}

/// Draws the screen's main action.
pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    button(
        ui,
        text,
        ButtonOptions {
            kind: ButtonKind::Primary,
            enabled,
            min_size: Vec2::new(140.0, 44.0),
            text_size: 15.0,
            ..Default::default()
        },
    )
}

/// Draws a secondary button with an optional icon.
pub fn secondary_button(ui: &mut Ui, icon: Option<&str>, text: &str) -> Response {
    button(
        ui,
        text,
        ButtonOptions {
            icon,
            ..Default::default()
        },
    )
}

/// Draws a round, borderless icon button with a tooltip.
pub fn icon_button(ui: &mut Ui, glyph: &str, tooltip: &str) -> Response {
    let palette = Palette::of(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, tooltip));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() || response.has_focus() {
            painter.circle_filled(rect.center(), 18.0, palette.surface_alt);
        }
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            theme::icons(16.0),
            palette.text,
        );
    }
    response.on_hover_text(tooltip)
}

/// Draws a row of mutually exclusive options in a pill, like a segmented
/// control. Returns true if the selection changed.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, options: &[(T, String)]) -> bool {
    let palette = Palette::of(ui);
    let height = 36.0;
    let padding = 3.0;
    let font = theme::regular(theme::BODY_SIZE);
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap(label.clone(), font.clone(), palette.text)
        })
        .collect();
    let segment_width = galleys
        .iter()
        .map(|g| g.size().x + 32.0)
        .fold(0.0, f32::max);
    let total = Vec2::new(segment_width * options.len() as f32 + padding * 2.0, height);
    let (outer, _) = ui.allocate_exact_size(total, Sense::hover());
    ui.painter()
        .rect_filled(outer, CornerRadius::same(10), palette.surface_alt);
    let mut changed = false;
    for (i, ((option, label), galley)) in options.iter().zip(galleys).enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(
                outer.min.x + padding + segment_width * i as f32,
                outer.min.y + padding,
            ),
            Vec2::new(segment_width, height - padding * 2.0),
        );
        let id = ui.scope_id().with(("segment", i));
        let response = ui.interact(rect, id, Sense::click());
        let selected = *value == *option;
        response.widget_info(|| {
            WidgetInfo::selected(Role::RadioButton, true, selected, label.as_str())
        });
        if response.clicked() && !selected {
            *value = *option;
            changed = true;
        }
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, CornerRadius::same(8), palette.surface);
            painter.rect_stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(1.0, palette.border),
                StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_filled(
                rect,
                CornerRadius::same(8),
                theme::fade(palette.surface, 0.5),
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(2.0, palette.accent),
                StrokeKind::Inside,
            );
        }
        let color = if selected {
            palette.text
        } else {
            palette.text_muted
        };
        painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    changed
}

/// Draws a large selectable card with a title, subtitle and optional badge.
pub fn choice_card(
    ui: &mut Ui,
    selected: bool,
    title: &str,
    subtitle: &str,
    badge: Option<&str>,
    width: f32,
) -> Response {
    let palette = Palette::of(ui);
    let title_galley = ui.painter().layout(
        title.to_owned(),
        theme::semibold(15.0),
        palette.text,
        width - 64.0,
    );
    let subtitle_galley = ui.painter().layout(
        subtitle.to_owned(),
        theme::regular(theme::CAPTION_SIZE),
        palette.text_muted,
        width - 64.0,
    );
    let height = 16.0 + title_galley.size().y + 4.0 + subtitle_galley.size().y + 16.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, title));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(10);
        let fill = if selected {
            palette.accent_soft
        } else if response.hovered() {
            palette.surface_alt
        } else {
            palette.surface
        };
        painter.rect_filled(rect, radius, fill);
        let (stroke_width, stroke_color) = if selected || response.has_focus() {
            (1.5, palette.accent)
        } else {
            (1.0, palette.border)
        };
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(stroke_width, stroke_color),
            StrokeKind::Inside,
        );
        // Radio dot.
        let dot = Pos2::new(
            rect.min.x + 24.0,
            rect.min.y + 16.0 + title_galley.size().y / 2.0,
        );
        painter.circle_stroke(
            dot,
            8.0,
            Stroke::new(
                1.5,
                if selected {
                    palette.accent
                } else {
                    palette.text_muted
                },
            ),
        );
        if selected {
            painter.circle_filled(dot, 4.5, palette.accent);
        }
        let text_x = rect.min.x + 44.0;
        let title_height = title_galley.size().y;
        let title_width = title_galley.size().x;
        painter.galley(
            Pos2::new(text_x, rect.min.y + 16.0),
            title_galley,
            palette.text,
        );
        painter.galley(
            Pos2::new(text_x, rect.min.y + 20.0 + title_height),
            subtitle_galley,
            palette.text_muted,
        );
        if let Some(badge) = badge {
            let badge_galley =
                painter.layout_no_wrap(badge.to_owned(), theme::semibold(11.0), palette.accent);
            let badge_rect = Rect::from_min_size(
                Pos2::new(text_x + title_width + 8.0, rect.min.y + 16.0),
                badge_galley.size() + Vec2::new(12.0, 4.0),
            );
            painter.rect_filled(badge_rect, CornerRadius::same(8), palette.accent_soft);
            painter.rect_stroke(
                badge_rect,
                CornerRadius::same(8),
                Stroke::new(1.0, theme::fade(palette.accent, 0.4)),
                StrokeKind::Inside,
            );
            painter.galley(
                badge_rect.min + Vec2::new(6.0, 2.0),
                badge_galley,
                palette.accent,
            );
        }
    }
    response
}

/// Draws a toggleable pill with a coloured dot. Returns its response; the
/// caller flips the state on click.
pub fn chip(ui: &mut Ui, selected: bool, label: &str, color: Color32) -> Response {
    let palette = Palette::of(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::regular(13.0), palette.text);
    let size = Vec2::new(galley.size().x + 40.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, selected, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(15);
        let fill = if selected {
            palette.surface_alt
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, radius, fill);
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(
                1.0,
                if response.hovered() || response.has_focus() {
                    palette.text_muted
                } else {
                    palette.border
                },
            ),
            StrokeKind::Inside,
        );
        let dot = Pos2::new(rect.min.x + 16.0, rect.center().y);
        if selected {
            painter.circle_filled(dot, 5.0, color);
        } else {
            painter.circle_stroke(dot, 4.5, Stroke::new(1.5, color));
        }
        let text_color = if selected {
            palette.text
        } else {
            palette.text_muted
        };
        painter.galley(
            Pos2::new(rect.min.x + 28.0, rect.center().y - galley.size().y / 2.0),
            galley,
            text_color,
        );
    }
    response
}

/// Draws an on/off switch with a label to its right. Returns its response;
/// `on` is flipped on click.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let palette = Palette::of(ui);
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        theme::regular(theme::BODY_SIZE),
        palette.text,
    );
    let track = Vec2::new(40.0, 22.0);
    let size = Vec2::new(
        track.x + 10.0 + galley.size().x,
        track.y.max(galley.size().y),
    );
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let selected = *on;
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, selected, label));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let track_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, rect.center().y - track.y / 2.0),
            track,
        );
        let painter = ui.painter();
        let off_color = palette.border;
        let fill = lerp_color(off_color, palette.accent, t);
        painter.rect_filled(track_rect, CornerRadius::same(11), fill);
        if response.has_focus() {
            painter.rect_stroke(
                track_rect.expand(2.0),
                CornerRadius::same(13),
                Stroke::new(2.0, palette.accent),
                StrokeKind::Outside,
            );
        }
        let knob_x = egui::lerp((track_rect.min.x + 11.0)..=(track_rect.max.x - 11.0), t);
        painter.circle_filled(
            Pos2::new(knob_x, track_rect.center().y),
            8.0,
            Color32::WHITE,
        );
        painter.galley(
            Pos2::new(
                track_rect.max.x + 10.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            palette.text,
        );
    }
    response
}

/// Draws a thin rounded progress bar across the available width.
pub fn progress_bar(ui: &mut Ui, fraction: f32) {
    let palette = Palette::of(ui);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    response.widget_info(|| {
        let mut info = WidgetInfo::labeled(Role::ProgressIndicator, true, "Test progress");
        info.value = Some(f64::from(fraction));
        info
    });
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), palette.surface_alt);
    let filled = Rect::from_min_size(
        rect.min,
        Vec2::new(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    painter.rect_filled(filled, CornerRadius::same(3), palette.accent);
}

/// Where a step stands in the flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    Done,
    Current,
    Upcoming,
}

/// One step in [`stepper`].
#[derive(Debug, Clone, Copy)]
pub struct Step<'a> {
    pub label: &'a str,
    pub state: StepState,
    /// Whether clicking the step goes to it.
    pub enabled: bool,
}

/// Draws the step header, centred. Returns the index of a clicked step.
pub fn stepper(ui: &mut Ui, steps: &[Step<'_>]) -> Option<usize> {
    let palette = Palette::of(ui);
    let circle = 26.0;
    let connector = 40.0;
    let font = theme::semibold(theme::BODY_SIZE);
    let galleys: Vec<_> = steps
        .iter()
        .map(|step| {
            let color = match step.state {
                StepState::Current => palette.text,
                StepState::Done | StepState::Upcoming => palette.text_muted,
            };
            ui.painter()
                .layout_no_wrap(step.label.to_owned(), font.clone(), color)
        })
        .collect();
    let widths: Vec<f32> = galleys
        .iter()
        .map(|g| circle + 8.0 + g.size().x + 12.0)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + connector * (steps.len() - 1) as f32;
    let (outer, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), circle + 12.0),
        Sense::hover(),
    );
    let mut x = outer.center().x - total / 2.0;
    let mut clicked = None;
    for (i, (step, galley)) in steps.iter().zip(galleys).enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(x - 6.0, outer.min.y),
            Vec2::new(widths[i], outer.height()),
        );
        let sense = if step.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(rect, ui.scope_id().with(("step", i)), sense);
        let label = format!("Step {}: {}", i + 1, step.label);
        response.widget_info(|| {
            WidgetInfo::selected(
                Role::Button,
                step.enabled,
                step.state == StepState::Current,
                &label,
            )
        });
        if response.clicked() {
            clicked = Some(i);
        }
        let painter = ui.painter();
        if step.enabled && (response.hovered() || response.has_focus()) {
            painter.rect_filled(rect, CornerRadius::same(16), palette.surface_alt);
        }
        let centre = Pos2::new(x + circle / 2.0, outer.center().y);
        match step.state {
            StepState::Done => {
                painter.circle_filled(centre, circle / 2.0, palette.accent_soft);
                painter.text(
                    centre,
                    Align2::CENTER_CENTER,
                    icon::CHECK,
                    theme::icons(12.0),
                    palette.accent,
                );
            }
            StepState::Current => {
                painter.circle_filled(centre, circle / 2.0, palette.accent);
                painter.text(
                    centre,
                    Align2::CENTER_CENTER,
                    (i + 1).to_string(),
                    theme::semibold(13.0),
                    palette.on_accent,
                );
            }
            StepState::Upcoming => {
                painter.circle_stroke(
                    centre,
                    circle / 2.0 - 0.75,
                    Stroke::new(1.5, palette.border),
                );
                painter.text(
                    centre,
                    Align2::CENTER_CENTER,
                    (i + 1).to_string(),
                    theme::semibold(13.0),
                    palette.text_muted,
                );
            }
        }
        let galley_size = galley.size();
        painter.galley(
            Pos2::new(x + circle + 8.0, outer.center().y - galley_size.y / 2.0),
            galley,
            palette.text,
        );
        x += widths[i];
        if i + 1 < steps.len() {
            let y = outer.center().y;
            painter.line_segment(
                [Pos2::new(x - 4.0, y), Pos2::new(x + connector - 10.0, y)],
                Stroke::new(1.5, palette.border),
            );
            x += connector;
        }
    }
    clicked
}

/// What the listening indicator shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseState {
    /// Between tones.
    Idle,
    /// A tone (or a silent check) is playing.
    Playing,
    /// Waiting for an answer.
    Waiting,
}

/// Draws the large circular listening indicator, centred in the available width.
pub fn pulse(ui: &mut Ui, state: PulseState) {
    let palette = Palette::of(ui);
    let size = 200.0;
    let (outer, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), size), Sense::hover());
    let centre = outer.center();
    let radius = 56.0;
    let painter = ui.painter();
    if state == PulseState::Playing {
        let time = ui.input(|input| input.time) as f32;
        for ring in 0..3 {
            let phase = (time * 0.9 + ring as f32 / 3.0).fract();
            let ring_radius = radius + phase * 40.0;
            painter.circle_stroke(
                centre,
                ring_radius,
                Stroke::new(2.0, theme::fade(palette.accent, 1.0 - phase)),
            );
        }
        ui.ctx().request_repaint();
    }
    let (fill, glyph_color) = match state {
        PulseState::Idle => (palette.surface_alt, palette.text_muted),
        PulseState::Playing => (palette.accent, palette.on_accent),
        PulseState::Waiting => (palette.accent_soft, palette.accent),
    };
    painter.circle_filled(centre, radius, fill);
    if state == PulseState::Waiting {
        painter.circle_stroke(centre, radius, Stroke::new(2.0, palette.accent));
    }
    painter.text(
        centre,
        Align2::CENTER_CENTER,
        icon::EAR,
        theme::icons(44.0),
        glyph_color,
    );
}

/// The tone of a [`banner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Warning,
    Error,
}

/// Draws a small muted note with an info icon, for fine print.
pub fn note(ui: &mut Ui, text: &str) {
    let palette = Palette::of(ui);
    ui.horizontal_top(|ui| {
        ui.label(
            egui::RichText::new(icon::INFO)
                .font(theme::icons(13.0))
                .color(palette.text_muted),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .size(theme::CAPTION_SIZE)
                    .color(palette.text_muted),
            )
            .wrap(),
        );
    });
}

/// Draws a tinted message row with an optional action button. Returns true
/// if the action was clicked.
pub fn banner(ui: &mut Ui, tone: Tone, text: &str, action: Option<&str>) -> bool {
    let palette = Palette::of(ui);
    let (color, glyph) = match tone {
        Tone::Info => (palette.accent, icon::INFO),
        Tone::Warning => (palette.warning, icon::WARNING),
        Tone::Error => (palette.error, icon::WARNING),
    };
    let mut clicked = false;
    egui::Frame::new()
        .fill(theme::fade(color, 0.12))
        .stroke(Stroke::new(1.0, theme::fade(color, 0.35)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(glyph)
                        .font(theme::icons(16.0))
                        .color(color),
                );
                // Leave room for the action button so the text wraps before it.
                let reserved = if action.is_some() { 130.0 } else { 0.0 };
                let text_width = (ui.available_width() - reserved).max(100.0);
                ui.allocate_ui(Vec2::new(text_width, 0.0), |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(text).color(palette.text)).wrap());
                });
                if let Some(action) = action {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        clicked = secondary_button(ui, None, action).clicked();
                    });
                }
            });
        });
    ui.add_space(CARD_GAP);
    clicked
}

/// Draws the clickable header of an expandable card: an icon in a tinted
/// square, a title and subtitle, and a chevron. The caller toggles `open`.
pub fn accordion_header(
    ui: &mut Ui,
    glyph: &str,
    title: &str,
    subtitle: &str,
    open: bool,
) -> Response {
    let palette = Palette::of(ui);
    let width = ui.available_width();
    let title_galley =
        ui.painter()
            .layout_no_wrap(title.to_owned(), theme::semibold(15.0), palette.text);
    let subtitle_galley = ui.painter().layout(
        subtitle.to_owned(),
        theme::regular(theme::CAPTION_SIZE),
        palette.text_muted,
        width - 110.0,
    );
    let height = (title_galley.size().y + 2.0 + subtitle_galley.size().y).max(40.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::DisclosureTriangle, true, open, title));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let tile = Rect::from_min_size(
            Pos2::new(rect.min.x, rect.center().y - 20.0),
            Vec2::splat(40.0),
        );
        painter.rect_filled(tile, CornerRadius::same(10), palette.accent_soft);
        painter.text(
            tile.center(),
            Align2::CENTER_CENTER,
            glyph,
            theme::icons(18.0),
            palette.accent,
        );
        let text_x = tile.max.x + 14.0;
        let text_top =
            rect.center().y - (title_galley.size().y + 2.0 + subtitle_galley.size().y) / 2.0;
        let title_height = title_galley.size().y;
        painter.galley(Pos2::new(text_x, text_top), title_galley, palette.text);
        painter.galley(
            Pos2::new(text_x, text_top + title_height + 2.0),
            subtitle_galley,
            palette.text_muted,
        );
        let chevron = if open {
            icon::CHEVRON_DOWN
        } else {
            icon::CHEVRON_RIGHT
        };
        let chevron_color = if response.hovered() || response.has_focus() {
            palette.text
        } else {
            palette.text_muted
        };
        painter.text(
            Pos2::new(rect.max.x - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            chevron,
            theme::icons(14.0),
            chevron_color,
        );
    }
    response
}

/// Whether a list row's item is usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStatus {
    Ready,
    Loading,
    Failed,
}

/// What happened to a [`list_row`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RowResponse {
    pub clicked: bool,
    /// The row's remove button was clicked.
    pub removed: bool,
}

/// Draws a full-width selectable row for a song, with a remove button.
pub fn list_row(ui: &mut Ui, label: &str, selected: bool, status: RowStatus) -> RowResponse {
    let palette = Palette::of(ui);
    let height = 36.0;
    let remove_width = 36.0;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - remove_width - 4.0, height),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if selected {
            palette.accent_soft
        } else if response.hovered() || response.has_focus() {
            palette.surface_alt
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, CornerRadius::same(8), fill);
        let (glyph, glyph_color) = match status {
            RowStatus::Failed => (icon::WARNING, palette.error),
            RowStatus::Ready | RowStatus::Loading if selected => (icon::MUSIC, palette.accent),
            RowStatus::Ready | RowStatus::Loading => (icon::MUSIC, palette.text_muted),
        };
        painter.text(
            Pos2::new(rect.min.x + 14.0, rect.center().y),
            Align2::LEFT_CENTER,
            glyph,
            theme::icons(14.0),
            glyph_color,
        );
        let text_color = if status == RowStatus::Failed {
            palette.text_muted
        } else {
            palette.text
        };
        let galley = painter.layout(
            label.to_owned(),
            theme::regular(theme::BODY_SIZE),
            text_color,
            rect.width() - 80.0,
        );
        let text_pos = Pos2::new(rect.min.x + 40.0, rect.center().y - galley.size().y / 2.0);
        painter.galley(text_pos, galley, text_color);
        if status == RowStatus::Loading {
            let spinner = Rect::from_center_size(
                Pos2::new(rect.max.x - 20.0, rect.center().y),
                Vec2::splat(14.0),
            );
            egui::Spinner::new().paint_at(ui, spinner);
        }
    }
    let mut removed = false;
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(Rect::from_min_size(
            Pos2::new(rect.max.x + 4.0, rect.min.y),
            Vec2::new(remove_width, height),
        )),
        |ui| {
            removed = icon_button(ui, icon::CLOSE, &format!("Remove {label}")).clicked();
        },
    );
    RowResponse {
        clicked: response.clicked(),
        removed,
    }
}

fn lerp_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
    )
}
