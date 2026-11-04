//! The Tune screen: the EQ curve, a strength slider, and advanced settings.

use egui::{Align, Layout, RichText, Slider, Ui};

use super::{App, Page};
use crate::charts;
use crate::eq_design::EqSettings;
use crate::hearing_score::HearingScore;
use crate::hearing_test::{Ear, HearingProfile, TestLength};
use crate::staircase::Threshold;
use crate::theme::{self, Palette, icon};
use crate::widgets::{self, Tone};

/// A left/right difference at least this large gets a gentle note, in dB.
const EAR_DIFFERENCE_NOTICE_DB: f32 = 10.0;

impl App {
    pub(super) fn tune_screen(&mut self, ui: &mut Ui) {
        let palette = Palette::of(ui);
        let Some(profile) = self.saved.profile.clone() else {
            widgets::screen_title(ui, "Your EQ", Some("Take the test first."));
            return;
        };
        let together = profile.ears_together;
        widgets::screen_title(ui, "Your EQ", Some(&self.design_summary));

        if profile.is_unreliable() {
            widgets::banner(
                ui,
                Tone::Warning,
                &format!(
                    "You heard {} of {} silent checks, so these results may not be reliable.",
                    profile.false_alarms, profile.catch_trials
                ),
                None,
            );
        }

        if let Some((weaker, difference)) = profile.ear_difference()
            && difference >= EAR_DIFFERENCE_NOTICE_DB
        {
            widgets::banner(
                ui,
                Tone::Info,
                &format!(
                    "Your {} ear needed tones about {difference:.0} dB louder than your other ear. \
                     That can come from how the headphones sit, but if it's new to you, \
                     consider getting your hearing checked by a doctor or audiologist.",
                    weaker.label().to_lowercase()
                ),
                None,
            );
        }

        if let Some(score) = HearingScore::new(&profile) {
            score_card(ui, score, &profile);
        }

        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::card_title(ui, None, "EQ curve");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if together {
                        widgets::chip(ui, true, "Both ears", palette.accent);
                        return;
                    }
                    // Right to left, so add the right ear first.
                    for ear in [Ear::Right, Ear::Left] {
                        let index = ear as usize;
                        let color = charts::ear_color(&palette, Some(ear));
                        let visible = self.visible_ears[index];
                        if widgets::chip(ui, visible, ear.label(), color).clicked() {
                            // Keep at least one curve on screen.
                            let other = self.visible_ears[1 - index];
                            if !visible || other {
                                self.visible_ears[index] = !visible;
                            }
                        }
                    }
                });
            });
            ui.add_space(4.0);
            charts::eq_chart(ui, &self.design, together, self.visible_ears);
            widgets::caption(
                ui,
                format!(
                    "Includes a {:.1} dB preamp so boosts can't clip. Hover the chart to read values.",
                    self.design.preamp_db
                ),
            );
        });

        widgets::card(ui, |ui| {
            let settings = &mut self.saved.settings;
            let label = ui
                .horizontal(|ui| {
                    let label = ui.label(
                        RichText::new("Strength")
                            .font(theme::semibold(theme::TITLE_SIZE))
                            .color(palette.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{:.0}%", settings.strength * 100.0))
                                .font(theme::semibold(theme::TITLE_SIZE))
                                .color(palette.accent),
                        );
                    });
                    label
                })
                .inner;
            ui.spacing_mut().slider_width = ui.available_width();
            ui.add(
                Slider::new(&mut settings.strength, 0.0..=1.0)
                    .show_value(false)
                    .step_by(0.05),
            )
            .labelled_by(label.id);
            ui.horizontal(|ui| {
                widgets::caption(ui, "Subtle");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::caption(ui, "Full correction");
                });
            });
            ui.add_space(4.0);
            widgets::muted(
                ui,
                "The test measures the quietest sounds you can hear. At normal volume those \
                 differences shrink, so 40–60% usually sounds most natural.",
            );
        });

        widgets::card(ui, |ui| {
            let header = widgets::accordion_header(
                ui,
                icon::SLIDERS,
                "Advanced",
                "Limits, smoothing and left/right balance.",
                self.show_advanced,
            );
            if header.clicked() {
                self.show_advanced = !self.show_advanced;
            }
            if self.show_advanced {
                self.advanced_settings(ui, together);
            }
        });

        widgets::card(ui, |ui| {
            let header = widgets::accordion_header(
                ui,
                icon::EAR,
                "What you heard",
                "Your sensitivity at each pitch, compared with your average.",
                self.show_heard,
            );
            if header.clicked() {
                self.show_heard = !self.show_heard;
            }
            if self.show_heard {
                ui.add_space(12.0);
                widgets::muted(
                    ui,
                    "How sensitive you are at each pitch compared with your own average, after \
                     allowing for the normal shape of human hearing. Lower means quieter sounds \
                     were missed. This measures your ears and headphones together.",
                );
                ui.add_space(8.0);
                charts::sensitivity_chart(ui, &profile);
                let missed: Vec<String> = profile
                    .measurements
                    .iter()
                    .filter(|m| m.threshold == Threshold::BeyondMax)
                    .filter(|m| !together || m.ear == Ear::Left)
                    .map(|m| {
                        let hz = charts::format_hz(f64::from(m.hz));
                        if together {
                            hz
                        } else {
                            format!("{hz} ({})", m.ear.label().to_lowercase())
                        }
                    })
                    .collect();
                if !missed.is_empty() {
                    widgets::caption(
                        ui,
                        format!(
                            "Not heard even at the loudest level, so left out of the EQ: {}.",
                            missed.join(", ")
                        ),
                    );
                }
            }
        });
    }

    /// The pinned bottom bar: retake or move on.
    pub(super) fn tune_actions(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui, None, "Retake test").clicked() {
                self.go_to(Page::Prepare);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::primary_button(ui, "Use this EQ", true).clicked() {
                    self.go_to(Page::Use);
                }
            });
        });
    }

    fn advanced_settings(&mut self, ui: &mut Ui, together: bool) {
        ui.add_space(8.0);
        let settings = &mut self.saved.settings;
        egui::Grid::new("advanced_grid")
            .num_columns(2)
            .spacing([24.0, 12.0])
            .show(ui, |ui| {
                let label = ui.label("Max boost");
                ui.add(Slider::new(&mut settings.max_boost_db, 0.0..=20.0).suffix(" dB"))
                    .labelled_by(label.id);
                ui.end_row();
                let label = ui.label("Max cut");
                ui.add(Slider::new(&mut settings.max_cut_db, 0.0..=20.0).suffix(" dB"))
                    .labelled_by(label.id);
                ui.end_row();
                let label = ui.label("Smoothing");
                ui.add(
                    Slider::new(&mut settings.smoothing, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                        .custom_parser(|s| {
                            s.trim_end_matches('%')
                                .trim()
                                .parse::<f64>()
                                .ok()
                                .map(|v| v / 100.0)
                        }),
                )
                .labelled_by(label.id)
                .on_hover_text(
                    "Evens out jumps between neighbouring pitches caused by test noise.",
                );
                ui.end_row();
            });
        if !together {
            ui.add_space(4.0);
            widgets::switch(ui, &mut settings.balance_ears, "Balance left and right")
                .on_hover_text("Also make the weaker ear louder overall.");
        }
        ui.add_space(8.0);
        if widgets::secondary_button(ui, None, "Reset to defaults").clicked() {
            let strength = self.saved.settings.strength;
            self.saved.settings = EqSettings {
                strength,
                ..EqSettings::default()
            };
        }
    }
}

/// Shows the just-for-fun hearing score.
fn score_card(ui: &mut Ui, score: HearingScore, profile: &HearingProfile) {
    let palette = Palette::of(ui);
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(score.points.to_string())
                    .font(theme::semibold(40.0))
                    .color(palette.accent),
            );
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("Hearing score · {}", score.label()))
                        .font(theme::semibold(theme::TITLE_SIZE))
                        .color(palette.text),
                );
                let highest = match score.highest_heard_hz {
                    Some(hz) if hz % 1000.0 == 0.0 => format!("{:.0} kHz", hz / 1000.0),
                    Some(hz) if hz >= 1000.0 => format!("{:.1} kHz", hz / 1000.0),
                    Some(hz) => format!("{hz:.0} Hz"),
                    None => "none".to_owned(),
                };
                widgets::muted(ui, format!("Highest pitch heard: {highest}"));
            });
        });
        ui.add_space(4.0);
        let length = test_length(profile)
            .map_or_else(String::new, |length| format!(" ({} test)", length.name()));
        widgets::caption(
            ui,
            format!(
                "Just for fun: out of 100, for how evenly you hear across pitches and between \
                 your ears. Scores compare fairly across headphones and volumes, but only \
                 between tests of the same length{length}. Not a medical test."
            ),
        );
    });
}

/// Returns the test length the profile was measured with, if it matches one.
fn test_length(profile: &HearingProfile) -> Option<TestLength> {
    let mut frequencies: Vec<f32> = profile.measurements.iter().map(|m| m.hz).collect();
    frequencies.sort_by(f32::total_cmp);
    frequencies.dedup();
    TestLength::ALL
        .into_iter()
        .find(|length| length.frequencies() == frequencies)
}
