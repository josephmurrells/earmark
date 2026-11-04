//! The Use screen: preview the EQ, then apply or copy it wherever you listen.

use egui::epaint::CornerRadius;
use egui::{Align, Key, Layout, RichText, Slider, Ui, Vec2};

use super::{App, Destination, SourceKind};
use crate::audio_engine::Command;
use crate::band_layout::{self, BandLayout, SliderSpec};
use crate::charts;
use crate::eq_design::Channels;
use crate::export;
use crate::theme::{self, Palette, icon};
use crate::track;
use crate::widgets::{self, ButtonKind, ButtonOptions};

impl App {
    pub(super) fn use_screen(&mut self, ui: &mut Ui) {
        if self.design.is_empty() {
            widgets::screen_title(ui, "Use your EQ", Some("Take the test first."));
            return;
        }
        widgets::screen_title(
            ui,
            "Use your EQ",
            Some("Try it out, then set it up wherever you listen."),
        );
        self.player_card(ui);
        for destination in Destination::ALL {
            self.destination_card(ui, destination);
        }
    }

    fn player_card(&mut self, ui: &mut Ui) {
        widgets::card(ui, |ui| {
            let palette = Palette::of(ui);
            // While scrubbing, show where the bar is, not where playback is.
            let position = self
                .song_position()
                .map(|(now, length)| (self.preview.scrub_secs.unwrap_or(now), length));
            ui.horizontal(|ui| {
                let (glyph, label) = if self.preview.playing {
                    (icon::PAUSE, "Pause (Space)")
                } else {
                    (icon::PLAY, "Play (Space)")
                };
                let play = widgets::button(
                    ui,
                    "",
                    ButtonOptions {
                        kind: ButtonKind::Primary,
                        icon: Some(glyph),
                        min_size: Vec2::splat(48.0),
                        text_size: 18.0,
                        ..Default::default()
                    },
                )
                .on_hover_text(label);
                let space = ui.input(|input| input.key_pressed(Key::Space))
                    && !ui.ctx().egui_wants_keyboard_input();
                if play.clicked() || space {
                    self.toggle_preview();
                }
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    let (title, detail) = self.now_playing(position);
                    ui.label(
                        RichText::new(title)
                            .font(theme::semibold(15.0))
                            .color(palette.text),
                    );
                    widgets::caption(ui, detail);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let toggled = widgets::switch(ui, &mut self.preview.eq_enabled, "EQ (B)")
                        .on_hover_text(
                            "Switch the EQ on and off to compare. Both play at the same level.",
                        )
                        .clicked();
                    let key = ui.input(|input| input.key_pressed(Key::B))
                        && !ui.ctx().egui_wants_keyboard_input();
                    if key {
                        self.preview.eq_enabled = !self.preview.eq_enabled;
                    }
                    if toggled || key {
                        self.send(Command::SetEqEnabled(self.preview.eq_enabled));
                    }
                });
            });

            if let Some((now, length)) = position {
                ui.add_space(8.0);
                self.seek_bar(ui, now, length);
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let mut source = self.preview.source;
                let options = [
                    (SourceKind::PinkNoise, "Pink noise".to_owned()),
                    (SourceKind::Music, "My music".to_owned()),
                ];
                if widgets::segmented(ui, &mut source, &options) {
                    self.set_preview_source(source);
                }
                if widgets::secondary_button(ui, Some(icon::ADD), "Add music").clicked() {
                    self.add_music_from_dialog();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().slider_width = 120.0;
                    let volume = ui
                        .add(
                            Slider::new(&mut self.saved.preview_gain_db, -40.0..=0.0)
                                .show_value(false),
                        )
                        .on_hover_text(format!(
                            "Preview volume: {:.0} dB",
                            self.saved.preview_gain_db
                        ));
                    let label = ui.label(
                        RichText::new(icon::VOLUME)
                            .font(theme::icons(16.0))
                            .color(palette.text_muted),
                    );
                    let volume = volume.labelled_by(label.id);
                    if volume.changed() {
                        self.send_preview_gain();
                    }
                });
            });

            if self.preview.source == SourceKind::Music {
                ui.add_space(12.0);
                self.music_list(ui);
            }
        });
    }

    /// Returns the player's title and detail line.
    fn now_playing(&self, position: Option<(f64, f64)>) -> (String, String) {
        if self.preview.source == SourceKind::PinkNoise {
            return (
                "Pink noise".to_owned(),
                "A neutral test signal. Switch to My music to hear your own songs.".to_owned(),
            );
        }
        let selected = self.preview.selected.and_then(|i| self.saved.music.get(i));
        match (selected, position) {
            (Some(path), Some((now, length))) => (
                track::display_name(path),
                format!("{} / {}", format_time(now), format_time(length)),
            ),
            (Some(path), None) if self.preview.loading.is_some() => {
                (track::display_name(path), "Loading...".to_owned())
            }
            (Some(path), None) => (
                track::display_name(path),
                "Couldn't load this song".to_owned(),
            ),
            (None, _) => (
                "My music".to_owned(),
                "Add a song, or drag files onto the window.".to_owned(),
            ),
        }
    }

    /// Draws the seek bar. Dragging it mutes the preview until it's released,
    /// so scrubbing doesn't play stuttering fragments.
    fn seek_bar(&mut self, ui: &mut Ui, now: f64, length: f64) {
        ui.spacing_mut().slider_width = ui.available_width();
        let mut position = now;
        let response = ui
            .add(
                Slider::new(&mut position, 0.0..=length.max(0.1))
                    .show_value(false)
                    .clamping(egui::SliderClamping::Always),
            )
            .on_hover_text("Drag to jump to another part of the song");
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Song position"));
        let held = response.is_pointer_button_down_on() || response.dragged();
        if held {
            if self.preview.scrub_secs.is_none() {
                self.send(Command::SetMuted(true));
            }
            self.preview.scrub_secs = Some(position);
        } else if let Some(target) = self.preview.scrub_secs.take() {
            // Released: jump once, then fade back in.
            let target = if response.changed() { position } else { target };
            self.seek_to(target);
            self.send(Command::SetMuted(false));
        } else if response.changed() {
            // Keyboard arrows on the focused bar.
            self.seek_to(position);
        }
    }

    fn music_list(&mut self, ui: &mut Ui) {
        if self.saved.music.is_empty() {
            widgets::caption(
                ui,
                "Add songs with Add music or drag them onto the window. MP3, FLAC, M4A, \
                 OGG and WAV all work. Songs stay in this list for next time.",
            );
            return;
        }
        let mut clicked = None;
        let mut removed = None;
        egui::ScrollArea::vertical()
            .id_salt("music_list")
            .max_height(190.0)
            .show(ui, |ui| {
                for (index, path) in self.saved.music.iter().enumerate() {
                    let status = if self.preview.failed.contains(path) {
                        widgets::RowStatus::Failed
                    } else if self
                        .preview
                        .loading
                        .as_ref()
                        .is_some_and(|l| &l.path == path)
                    {
                        widgets::RowStatus::Loading
                    } else {
                        widgets::RowStatus::Ready
                    };
                    let selected = self.preview.selected == Some(index);
                    let row = widgets::list_row(ui, &track::display_name(path), selected, status);
                    if row.clicked {
                        clicked = Some(index);
                    }
                    if row.removed {
                        removed = Some(index);
                    }
                }
            });
        if let Some(index) = removed {
            self.remove_song(index);
        } else if let Some(index) = clicked {
            if self.preview.selected == Some(index) && self.selected_song_ready() {
                self.toggle_preview();
            } else {
                if self.preview.playing {
                    self.preview.play_when_loaded = true;
                }
                self.select_song(index);
            }
        }
    }

    fn destination_card(&mut self, ui: &mut Ui, destination: Destination) {
        let (glyph, title, subtitle) = match destination {
            Destination::AppEq => (
                icon::PHONE,
                "Spotify or headphone app",
                "Slider values to copy into an EQ with fixed bands.",
            ),
            Destination::Parametric => (
                icon::SLIDERS,
                "Parametric EQ apps",
                "Filters for Wavelet, Poweramp, Qudelix, Peace and similar.",
            ),
            Destination::Files => (
                icon::DOWNLOAD,
                "Export files",
                "AutoEQ-style text files, Edifier presets and your test results.",
            ),
        };
        widgets::card(ui, |ui| {
            let open = self.open_destination == Some(destination);
            if widgets::accordion_header(ui, glyph, title, subtitle, open).clicked() {
                self.open_destination = if open { None } else { Some(destination) };
            }
            if self.open_destination != Some(destination) {
                return;
            }
            ui.add_space(12.0);
            match destination {
                Destination::AppEq => self.app_eq_section(ui),
                Destination::Parametric => self.parametric_section(ui),
                Destination::Files => self.files_section(ui),
            }
        });
    }

    fn app_eq_section(&mut self, ui: &mut Ui) {
        let palette = Palette::of(ui);
        egui::Grid::new("app_eq_options")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                let label = ui.label("My EQ");
                egui::ComboBox::from_id_salt("manual_layout")
                    .width(280.0)
                    .selected_text(self.saved.manual_layout.label())
                    .show_ui(ui, |ui| {
                        for layout in BandLayout::ALL {
                            ui.selectable_value(
                                &mut self.saved.manual_layout,
                                layout,
                                layout.label(),
                            );
                        }
                    })
                    .response
                    .labelled_by(label.id);
                ui.end_row();
                if self.saved.manual_layout == BandLayout::Custom {
                    let label = ui.label("Bands");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.saved.custom_bands)
                            .desired_width(280.0)
                            .hint_text("60, 250, 1k, 4k, 12k"),
                    )
                    .labelled_by(label.id);
                    ui.end_row();
                }
                if !self.ears_together() {
                    let label = ui.label("Ear");
                    channels_combo(ui, "manual_channels", &mut self.saved.export_channels)
                        .labelled_by(label.id);
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        widgets::switch(
            ui,
            &mut self.saved.manual_headroom,
            "Keep every band at or below 0 dB",
        )
        .on_hover_text(
            "Most apps have no preamp, so boosts can distort loud music. This shifts \
             everything down instead; turn your volume up a little to make up for it.",
        );
        ui.add_space(12.0);

        let spec = match self.saved.manual_layout.spec() {
            Some(spec) => Ok(spec),
            None => {
                band_layout::parse_frequencies(&self.saved.custom_bands).map(SliderSpec::custom)
            }
        };
        let spec = match spec {
            Ok(spec) => spec,
            Err(e) => {
                widgets::caption(ui, e);
                return;
            }
        };
        let eq = self.design.channels(self.saved.export_channels);
        let settings = band_layout::band_settings(eq, &spec, self.saved.manual_headroom);
        egui::Frame::new()
            .fill(palette.surface_alt)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new("manual_bands")
                    .num_columns(3)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        for setting in &settings {
                            ui.label(
                                RichText::new(charts::format_hz(f64::from(setting.hz)))
                                    .color(palette.text_muted),
                            );
                            ui.label(
                                RichText::new(format_gain(setting.gain_db))
                                    .font(theme::semibold(theme::BODY_SIZE))
                                    .color(palette.text),
                            );
                            gain_bar(ui, setting.gain_db, spec.limit_db);
                            ui.end_row();
                        }
                    });
            });
        if settings.iter().any(|s| s.limited) {
            widgets::caption(
                ui,
                format!(
                    "Some bands need more than ±{:.0} dB, so they're set to the limit.",
                    spec.limit_db
                ),
            );
        }
        ui.add_space(8.0);
        if widgets::secondary_button(ui, Some(icon::COPY), "Copy values").clicked() {
            let mut text = format!("{}:\n", self.saved.manual_layout.label());
            for setting in &settings {
                let hz = charts::format_hz(f64::from(setting.hz));
                text.push_str(&format!("{hz}: {}\n", format_gain(setting.gain_db)));
            }
            ui.ctx().copy_text(text);
            self.info("Copied the EQ values.");
        }
    }

    fn parametric_section(&mut self, ui: &mut Ui) {
        let palette = Palette::of(ui);
        if !self.ears_together() {
            ui.horizontal(|ui| {
                let label = ui.label("Ear");
                channels_combo(ui, "parametric_channels", &mut self.saved.export_channels)
                    .labelled_by(label.id);
            });
            ui.add_space(4.0);
        }
        let eq = self.design.channels(self.saved.export_channels);
        let preamp = (-eq.peak_gain_db().max(0.0) * 10.0).floor() / 10.0;
        widgets::muted(
            ui,
            format!(
                "Add these filters, with a preamp of {preamp:.1} dB if the app has one. Shelves \
                 may be called LS/HS or bass/treble shelf."
            ),
        );
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(palette.surface_alt)
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new("parametric_filters")
                    .num_columns(4)
                    .spacing([28.0, 8.0])
                    .show(ui, |ui| {
                        for heading in ["Type", "Frequency", "Gain", "Q"] {
                            ui.label(RichText::new(heading).color(palette.text_muted));
                        }
                        ui.end_row();
                        for filter in &eq.filters {
                            ui.label(filter.kind.label());
                            ui.label(charts::format_hz(f64::from(filter.hz)));
                            ui.label(
                                RichText::new(format_gain(filter.gain_db))
                                    .font(theme::semibold(theme::BODY_SIZE)),
                            );
                            ui.label(format!("{:.2}", filter.q));
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(8.0);
        if widgets::secondary_button(ui, Some(icon::COPY), "Copy as text").clicked() {
            ui.ctx().copy_text(export::parametric_eq(eq));
            self.info("Copied the filters in AutoEQ format.");
        }
    }

    fn files_section(&mut self, ui: &mut Ui) {
        if !self.ears_together() {
            ui.horizontal(|ui| {
                let label = ui.label("Single-curve files use");
                channels_combo(ui, "export_channels", &mut self.saved.export_channels)
                    .labelled_by(label.id);
            });
            ui.add_space(8.0);
        }
        // Only the file that's asked for is generated.
        let mut export = None;
        subsection(
            ui,
            "EQ files",
            "For Equalizer APO, Peace, Poweramp, Wavelet and other apps that import AutoEQ files.",
        );
        ui.horizontal_wrapped(|ui| {
            if widgets::secondary_button(ui, Some(icon::SAVE), "Per-ear EQ")
                .on_hover_text(
                    "Separate left and right filters, in the config format Equalizer APO and \
                     Peace read",
                )
                .clicked()
            {
                export = Some(ExportFile::PerEar);
            }
            if widgets::secondary_button(ui, Some(icon::SAVE), "ParametricEQ.txt")
                .on_hover_text("AutoEQ format: Equalizer APO, Peace, Poweramp and many others")
                .clicked()
            {
                export = Some(ExportFile::Parametric);
            }
            if widgets::secondary_button(ui, Some(icon::SAVE), "GraphicEQ.txt")
                .on_hover_text("AutoEQ graphic format, for Wavelet on Android")
                .clicked()
            {
                export = Some(ExportFile::Graphic);
            }
        });

        subsection_gap(ui);
        subsection(
            ui,
            "Edifier / Hecate",
            "A preset for the app's 10-band EQ, fitted to your curve. Import it from the \
             app's EQ screen.",
        );
        ui.horizontal(|ui| {
            let label = ui.label("Device name");
            ui.add(
                egui::TextEdit::singleline(&mut self.saved.edifier_device)
                    .desired_width(140.0)
                    .hint_text(export::EDIFIER_DEFAULT_DEVICE),
            )
            .labelled_by(label.id)
            .on_hover_text(
                "The \"device_name\" in a preset exported from the app. It must match your \
                 headphones, or the app may refuse the import.",
            );
            ui.add_space(8.0);
            if widgets::secondary_button(ui, Some(icon::SAVE), "Earmark.eq.json")
                .on_hover_text("Save the preset")
                .clicked()
            {
                export = Some(ExportFile::Edifier);
            }
        });
        if let Some(file) = export {
            let single = self.design.channels(self.saved.export_channels);
            let (name, kind, contents) = match file {
                ExportFile::PerEar => (
                    "earmark.txt",
                    "per-ear EQ",
                    export::equalizer_apo(&self.design),
                ),
                ExportFile::Parametric => (
                    "ParametricEQ.txt",
                    "Parametric EQ",
                    export::parametric_eq(single),
                ),
                ExportFile::Graphic => ("GraphicEQ.txt", "Graphic EQ", export::graphic_eq(single)),
                ExportFile::Edifier => {
                    let (json, limited) = export::edifier_eq(single, &self.saved.edifier_device);
                    if limited {
                        self.info(
                            "Some bands needed more than ±12 dB, so they're set to the limit.",
                        );
                    }
                    ("Earmark.eq.json", "Edifier preset", json)
                }
            };
            self.save_text(name, kind, contents);
        }
        subsection_gap(ui);
        subsection(
            ui,
            "Test results",
            "Keep your results, or load them on another PC to get the same EQ.",
        );
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui, Some(icon::SAVE), "Save test results").clicked() {
                self.save_results();
            }
            if widgets::secondary_button(ui, Some(icon::OPEN_FILE), "Load test results").clicked() {
                self.load_results();
            }
        });
    }
}

/// A file the Export card can save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFile {
    PerEar,
    Parametric,
    Graphic,
    Edifier,
}

/// Draws a subsection heading with a one-line description under it.
fn subsection(ui: &mut Ui, title: &str, description: &str) {
    let palette = Palette::of(ui);
    ui.label(
        RichText::new(title)
            .font(theme::semibold(theme::BODY_SIZE))
            .color(palette.text),
    );
    widgets::caption(ui, description);
    ui.add_space(8.0);
}

/// Separates two subsections with a divider line.
fn subsection_gap(ui: &mut Ui) {
    ui.add_space(14.0);
    ui.separator();
    ui.add_space(10.0);
}

/// Formats seconds as m:ss.
fn format_time(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Formats a gain with its sign, such as "+2.5 dB", "-1.0 dB" or "0.0 dB".
fn format_gain(gain_db: f32) -> String {
    if gain_db.abs() < 0.05 {
        "0.0 dB".to_owned()
    } else {
        format!("{gain_db:+.1} dB")
    }
}

/// Shows a combo box choosing which ear's EQ a single-curve output uses.
fn channels_combo(ui: &mut Ui, id: &str, channels: &mut Channels) -> egui::Response {
    egui::ComboBox::from_id_salt(id)
        .width(280.0)
        .selected_text(channels.label())
        .show_ui(ui, |ui| {
            for option in Channels::ALL {
                ui.selectable_value(channels, option, option.label());
            }
        })
        .response
}

/// Draws a small horizontal bar for a gain, centred on 0 dB.
fn gain_bar(ui: &mut Ui, gain_db: f32, limit_db: f32) {
    let palette = Palette::of(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(200.0, 8.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), palette.border);
    let centre = rect.center().x;
    let end = centre + (gain_db / limit_db).clamp(-1.0, 1.0) * rect.width() / 2.0;
    let bar = egui::Rect::from_x_y_ranges(centre.min(end)..=centre.max(end), rect.y_range());
    painter.rect_filled(bar, CornerRadius::same(4), palette.accent);
    painter.vline(
        centre,
        rect.expand(3.0).y_range(),
        egui::Stroke::new(1.5, palette.text_muted),
    );
}
