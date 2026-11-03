//! The Test screen: a focused view with a listening indicator and two answers.

use egui::{Align, Key, Layout, RichText, Ui, Vec2};

use super::{App, Page, Phase, describe_time_left};
use crate::hearing_test::EarMode;
use crate::staircase::Response;
use crate::theme::{self, Palette, icon};
use crate::widgets::{self, ButtonKind, ButtonOptions, PulseState};

const ANSWER_SIZE: Vec2 = Vec2::new(190.0, 52.0);

impl App {
    /// The Test step before a test has started: what's about to happen, with
    /// Start in the bottom bar.
    pub(super) fn test_intro(&mut self, ui: &mut Ui) {
        let palette = Palette::of(ui);
        widgets::screen_title(
            ui,
            "Ready for the hearing test",
            Some("Make sure you've done the checks on the Prepare step first."),
        );
        widgets::card(ui, |ui| {
            let mode = match self.saved.ear_mode {
                EarMode::Separate => "One ear at a time",
                EarMode::Together => "Both ears together",
            };
            let length = self.saved.test_length;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{mode} · {} · about {} min",
                        length.name(),
                        length.minutes(self.saved.ear_mode)
                    ))
                    .font(theme::semibold(15.0))
                    .color(palette.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::secondary_button(ui, None, "Change").clicked() {
                        self.go_to(Page::Prepare);
                    }
                });
            });
            ui.add_space(8.0);
            for line in [
                "You'll hear short beeps at different pitches, getting quieter until you can't \
                 hear them.",
                "After each one, answer whether you heard it. Some checks are silent on \
                 purpose, so answer honestly.",
                "Keys: Y or Space = yes, N = no, R = replay, P = pause.",
            ] {
                widgets::muted(ui, format!("•  {line}"));
            }
        });
        if self.saved.profile.is_some() {
            widgets::caption(
                ui,
                "Your current results stay until the new test finishes; stopping part-way \
                 keeps them.",
            );
        }
    }

    pub(super) fn test_screen(&mut self, ui: &mut Ui) {
        let palette = Palette::of(ui);
        let Some(run) = &self.run else {
            return;
        };
        let (done, total) = run.test.progress();
        let fraction = run.test.fraction_done();
        let time_left = run.time_left();
        let phase = run.phase;
        let paused = run.paused;

        // Top bar: title, progress, pause and stop.
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Hearing test")
                    .font(theme::semibold(theme::TITLE_SIZE))
                    .color(palette.text),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, icon::CLOSE, "Stop test").clicked() {
                    self.cancel_test();
                }
                let (glyph, tip) = if paused {
                    (icon::PLAY, "Resume (P)")
                } else {
                    (icon::PAUSE, "Pause (P)")
                };
                if widgets::icon_button(ui, glyph, tip).clicked() {
                    self.toggle_pause();
                }
            });
        });
        if self.run.is_none() {
            return;
        }
        ui.add_space(8.0);
        widgets::progress_bar(ui, fraction);
        ui.horizontal(|ui| {
            widgets::caption(ui, format!("{done} of {total} pitches done"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                widgets::caption(ui, describe_time_left(time_left));
            });
        });

        ui.add_space((ui.available_height() * 0.08).max(16.0));
        let pulse_state = match phase {
            _ if paused => PulseState::Idle,
            Phase::Pause { .. } => PulseState::Idle,
            Phase::Playing { .. } => PulseState::Playing,
            Phase::Answering => PulseState::Waiting,
        };
        widgets::pulse(ui, pulse_state);

        let (status, hint) = match phase {
            _ if paused => ("Paused", "Take a break; nothing plays until you resume."),
            Phase::Pause { .. } => ("Get ready", "The next tone is coming."),
            Phase::Playing { .. } => (
                "Listen",
                "Some tones are very quiet, and some checks are silent.",
            ),
            Phase::Answering => ("Did you hear it?", "Answer honestly, or replay the tone."),
        };
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(status)
                    .font(theme::semibold(26.0))
                    .color(palette.text),
            );
            widgets::muted(ui, hint);
        });
        ui.add_space(24.0);

        if paused {
            centred_row(ui, 160.0, |ui| {
                if widgets::primary_button(ui, "Resume", true).clicked() {
                    self.toggle_pause();
                }
            });
        } else {
            let can_answer = matches!(phase, Phase::Playing { .. } | Phase::Answering);
            let row_width = ANSWER_SIZE.x * 2.0 + ui.spacing().item_spacing.x;
            centred_row(ui, row_width, |ui| {
                let answer = |kind| ButtonOptions {
                    kind,
                    enabled: can_answer,
                    min_size: ANSWER_SIZE,
                    text_size: 16.0,
                    ..Default::default()
                };
                // Both answers look the same so neither feels like the "right" one.
                if widgets::button(ui, "Yes, I heard it", answer(ButtonKind::Secondary)).clicked() {
                    self.answer(Response::Heard);
                }
                if widgets::button(ui, "No", answer(ButtonKind::Secondary)).clicked() {
                    self.answer(Response::NotHeard);
                }
            });
            ui.add_space(8.0);
            centred_row(ui, 120.0, |ui| {
                let replay = widgets::button(
                    ui,
                    "Replay",
                    ButtonOptions {
                        kind: ButtonKind::Ghost,
                        icon: Some(icon::REPLAY),
                        enabled: phase == Phase::Answering,
                        min_size: Vec2::new(120.0, 36.0),
                        ..Default::default()
                    },
                );
                if replay.clicked() {
                    self.replay();
                }
            });
        }

        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            widgets::caption(
                ui,
                "Keys:  Y or Space = yes   ·   N = no   ·   R = replay   ·   P = pause",
            );
        });

        let (yes, no, replay, pause) = ui.input(|input| {
            (
                input.key_pressed(Key::Y) || input.key_pressed(Key::Space),
                input.key_pressed(Key::N),
                input.key_pressed(Key::R),
                input.key_pressed(Key::P),
            )
        });
        if pause {
            self.toggle_pause();
        } else if replay {
            self.replay();
        } else if yes {
            self.answer(Response::Heard);
        } else if no {
            self.answer(Response::NotHeard);
        }
    }
}

/// Lays out a row of known width centred horizontally.
fn centred_row(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        add_contents(ui);
    });
}
