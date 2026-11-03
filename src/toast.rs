//! Short messages that appear in the bottom-right corner and fade out.

use egui::epaint::CornerRadius;
use egui::{Align2, Context, Id, Margin, RichText, Sense, Stroke};

use crate::theme::{self, Palette, icon};

/// Seconds an informational toast stays up.
const INFO_SECS: f64 = 4.0;
/// Seconds an error toast stays up.
const ERROR_SECS: f64 = 9.0;
/// Seconds spent fading out at the end.
const FADE_SECS: f64 = 0.4;

/// Whether a toast reports success or a problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Debug, Clone)]
struct Toast {
    text: String,
    kind: ToastKind,
    /// Context time when first shown; set on the first frame.
    shown_at: Option<f64>,
}

/// The toasts currently on screen.
#[derive(Debug, Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    /// Shows a message.
    pub fn push(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.items.push(Toast {
            text: text.into(),
            kind,
            shown_at: None,
        });
        // Keep the stack short.
        if self.items.len() > 3 {
            self.items.remove(0);
        }
    }

    /// Returns the newest message, if any (used by tests).
    #[cfg(test)]
    #[must_use]
    pub fn latest(&self) -> Option<(ToastKind, &str)> {
        self.items.last().map(|t| (t.kind, t.text.as_str()))
    }

    /// Draws the toasts and removes expired ones.
    pub fn show(&mut self, ctx: &Context) {
        let now = ctx.input(|input| input.time);
        self.items.retain_mut(|toast| {
            let shown_at = *toast.shown_at.get_or_insert(now);
            now - shown_at < lifetime(toast.kind)
        });
        if self.items.is_empty() {
            return;
        }
        let palette = if ctx.global_style().visuals.dark_mode {
            Palette::DARK
        } else {
            Palette::LIGHT
        };
        let mut dismissed = None;
        egui::Area::new(Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                for (index, toast) in self.items.iter().enumerate() {
                    let age = now - toast.shown_at.unwrap_or(now);
                    let remaining = lifetime(toast.kind) - age;
                    let opacity = (remaining / FADE_SECS).clamp(0.0, 1.0) as f32;
                    let (accent, glyph) = match toast.kind {
                        ToastKind::Info => (palette.success, icon::COMPLETED),
                        ToastKind::Error => (palette.error, icon::WARNING),
                    };
                    let frame = egui::Frame::new()
                        .fill(theme::fade(palette.surface, opacity))
                        .stroke(Stroke::new(1.0, theme::fade(palette.border, opacity)))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(14, 10))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 4],
                            blur: 16,
                            spread: 0,
                            color: egui::Color32::from_black_alpha((40.0 * opacity) as u8),
                        });
                    let response = frame
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(glyph)
                                        .font(theme::icons(16.0))
                                        .color(theme::fade(accent, opacity)),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&toast.text)
                                            .color(theme::fade(palette.text, opacity)),
                                    )
                                    .wrap(),
                                );
                            });
                        })
                        .response
                        .interact(Sense::click())
                        .on_hover_text("Click to dismiss");
                    if response.clicked() {
                        dismissed = Some(index);
                    }
                    ui.add_space(8.0);
                }
            });
        if let Some(index) = dismissed {
            self.items.remove(index);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn lifetime(kind: ToastKind) -> f64 {
    match kind {
        ToastKind::Info => INFO_SECS,
        ToastKind::Error => ERROR_SECS,
    }
}
