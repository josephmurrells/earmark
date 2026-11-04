//! The Prepare screen: headphones, volume, left/right check, and test options.

use egui::{Align, Layout, Ui};

use super::App;
use crate::hearing_test::{Ear, EarMode, TestLength};
use crate::theme::icon;
use crate::widgets::{self, Tone};

impl App {
    pub(super) fn prepare_screen(&mut self, ui: &mut Ui) {
        widgets::screen_title(
            ui,
            "Let's get you set up",
            Some("Three quick checks, then a short listening test."),
        );

        if let Err(e) = &self.engine {
            let text = e.clone();
            if widgets::banner(ui, Tone::Error, &text, Some("Reconnect")) {
                self.reconnect_audio();
            }
        }
        if let Ok(engine) = &self.engine
            && engine.channels() < 2
        {
            widgets::banner(
                ui,
                Tone::Warning,
                "This output is mono, so each ear can't be tested separately.",
                None,
            );
        }

        widgets::card(ui, |ui| {
            widgets::card_title(ui, Some(1), "Headphones on, quiet room");
            widgets::muted(
                ui,
                "Use the headphones you want to tune. Turn off other EQs, spatial sound and \
                 audio enhancements so you hear them as they are.",
            );
        });

        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 150.0);
                    widgets::card_title(ui, Some(2), "Set a comfortable volume");
                    widgets::muted(
                        ui,
                        "Hold the button to play a tone, and raise the Windows volume until \
                         it's clear but comfortable, like a normal conversation. Let go and \
                         it stops at once. Then leave the volume there.",
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (glyph, label) = if self.calibrating {
                        (icon::VOLUME, "Playing")
                    } else {
                        (icon::PLAY, "Hold to play")
                    };
                    let held = widgets::secondary_button(ui, Some(glyph), label)
                        .on_hover_text("The tone plays only while you hold the button.")
                        .is_pointer_button_down_on();
                    self.set_calibrating(held);
                    if held {
                        // Keep checking, so a release is never missed.
                        ui.ctx().request_repaint();
                    }
                });
            });
        });

        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 220.0);
                    widgets::card_title(ui, Some(3), "Check left and right");
                    widgets::muted(ui, "Each beep should come from that side only.");
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // Right to left, so add the right ear first.
                    if widgets::secondary_button(ui, None, "Right").clicked() {
                        self.beep_in(Ear::Right);
                    }
                    if widgets::secondary_button(ui, None, "Left").clicked() {
                        self.beep_in(Ear::Left);
                    }
                });
            });
        });

        widgets::card(ui, |ui| {
            widgets::card_title(ui, Some(4), "Choose your test");
            ui.add_space(4.0);
            let width = (ui.available_width() - 12.0) / 2.0;
            ui.horizontal(|ui| {
                let separate = widgets::choice_card(
                    ui,
                    self.saved.ear_mode == EarMode::Separate,
                    "One ear at a time",
                    "A separate EQ for each ear, and fixes left/right imbalance.",
                    Some("Recommended"),
                    width,
                );
                if separate.clicked() {
                    self.saved.ear_mode = EarMode::Separate;
                }
                let together = widgets::choice_card(
                    ui,
                    self.saved.ear_mode == EarMode::Together,
                    "Both ears together",
                    "Twice as fast. One EQ for both ears; best if your ears are similar.",
                    None,
                    width,
                );
                if together.clicked() {
                    self.saved.ear_mode = EarMode::Together;
                }
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                widgets::muted(ui, "Length");
                ui.add_space(8.0);
                let options: Vec<(TestLength, String)> = TestLength::ALL
                    .iter()
                    .map(|&length| {
                        let minutes = length.minutes(self.saved.ear_mode);
                        (length, format!("{} · {minutes} min", length.name()))
                    })
                    .collect();
                widgets::segmented(ui, &mut self.saved.test_length, &options);
            });
            let bands = self.saved.test_length.frequencies().len();
            widgets::caption(
                ui,
                format!(
                    "{bands} pitches from deep bass to high treble. You can pause at any time."
                ),
            );
        });

        widgets::note(
            ui,
            "Earmark tunes how your headphones sound to you. It isn't a medical hearing \
             test and can't diagnose hearing loss. If you've noticed a change in your \
             hearing, see a doctor or audiologist.",
        );
    }

    /// The pinned bottom bar: output device and the Start button.
    pub(super) fn prepare_actions(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let device = match &self.engine {
                Ok(engine) => format!(
                    "Output: {} · {} kHz",
                    engine.device_name(),
                    engine.sample_rate() / 1000
                ),
                Err(_) => "No audio output".to_owned(),
            };
            widgets::caption(ui, device);
            if widgets::icon_button(
                ui,
                icon::SYNC,
                "Reconnect audio (after switching headphones)",
            )
            .clicked()
            {
                self.reconnect_audio();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::primary_button(ui, "Start test", self.engine.is_ok()).clicked() {
                    self.start_test();
                }
            });
        });
    }
}
