//! Earmark: builds a personal headphone EQ from a tone-based hearing test.

#![windows_subsystem = "windows"]

mod app;
mod audio_engine;
mod band_layout;
mod biquad;
mod charts;
mod crash_report;
mod eq_design;
mod export;
mod freq_point;
mod hearing_test;
mod pink_noise;
mod reference;
mod staircase;
mod theme;
mod title_bar;
mod toast;
mod track;
mod widgets;

/// The app's name, also used for its settings folder (`%APPDATA%\Earmark`).
const APP_NAME: &str = "Earmark";
/// The name the app had before, whose settings folder is carried over.
const LEGACY_APP_NAME: &str = "ear-eq";

fn main() -> eframe::Result {
    crash_report::install(eframe::storage_dir(APP_NAME));
    migrate_settings();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size(app::PREFERRED_SIZE)
            .with_min_inner_size([640.0, 480.0])
            .with_icon(std::sync::Arc::new(title_bar::app_icon())),
        // Window geometry restore misbehaves after closing while minimised.
        persist_window: false,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

/// Copies saved results and settings from the folder used before the rename,
/// on the first run under the new name. The old folder is left untouched.
fn migrate_settings() {
    let (Some(old), Some(new)) = (
        eframe::storage_dir(LEGACY_APP_NAME),
        eframe::storage_dir(APP_NAME),
    ) else {
        return;
    };
    let old_file = old.join("app.ron");
    if new.exists() || !old_file.is_file() {
        return;
    }
    // Best effort: if this fails the app just starts fresh.
    if std::fs::create_dir_all(&new).is_ok() {
        let _ = std::fs::copy(old_file, new.join("app.ron"));
    }
}
