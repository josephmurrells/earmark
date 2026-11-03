//! The application window: state, navigation and the logic behind each screen.
//!
//! Each screen's layout lives in a child module.

mod prepare_screen;
mod test_screen;
mod tune_screen;
mod use_screen;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use egui::Ui;
use serde::{Deserialize, Serialize};

use crate::audio_engine::{AudioEngine, Command, EqProcessor, PreviewSource, Tone, TonePattern};
use crate::band_layout::BandLayout;
use crate::eq_design::{Channels, EqDesign, EqSettings};
use crate::hearing_test::{self, EarMode, HearingProfile, HearingTest, Stimulus, TestLength};
use crate::staircase::Response;
use crate::theme::{self, Palette};
use crate::title_bar::TitleBar;
use crate::toast::{ToastKind, Toasts};
use crate::track::{self, Track, TrackError};
use crate::widgets::{self, Step, StepState};

const STORAGE_KEY: &str = "earmark";
/// The key settings were saved under before the rename.
const LEGACY_STORAGE_KEY: &str = "ear-eq";
const PIPS_PER_TONE: u32 = 3;
/// Range of the random pause before each tone, in seconds.
const PAUSE_SECS: (f32, f32) = (0.5, 1.2);
/// Level of the left/right check tones, in dBFS.
const EAR_CHECK_LEVEL_DB: f32 = -30.0;
/// Largest results file accepted, in bytes; real ones are a few kilobytes.
const MAX_RESULTS_FILE_BYTES: u64 = 1024 * 1024;
/// Widest the content column gets, in points.
const CONTENT_WIDTH: f32 = 760.0;
/// The window's preferred inner size, which shows a whole screen without scrolling.
pub const PREFERRED_SIZE: egui::Vec2 = egui::vec2(1000.0, 1060.0);
/// Room left for the taskbar and a gap when fitting the window to the monitor.
const TASKBAR_ALLOWANCE: f32 = 64.0;
/// Most songs kept in the music list.
const MAX_LIBRARY: usize = 200;

/// Settings and results kept between runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    profile: Option<HearingProfile>,
    settings: EqSettings,
    test_length: TestLength,
    ear_mode: EarMode,
    export_channels: Channels,
    preview_gain_db: f32,
    manual_layout: BandLayout,
    custom_bands: String,
    /// Shift hand-copied settings so no band is above 0 dB.
    manual_headroom: bool,
    /// Music files added for previewing, in the order they were added.
    music: Vec<PathBuf>,
}

impl Saved {
    /// Returns the settings with anything out of range from disk corrected.
    fn sanitized(self) -> Self {
        let preview_gain_db = if self.preview_gain_db.is_finite() {
            self.preview_gain_db.clamp(-40.0, 0.0)
        } else {
            Self::default().preview_gain_db
        };
        // Forget files that have been moved or deleted since last time.
        let mut music: Vec<PathBuf> = Vec::new();
        for path in self.music {
            if track::is_supported(&path) && path.is_file() && !music.contains(&path) {
                music.push(path);
            }
        }
        music.truncate(MAX_LIBRARY);
        Self {
            profile: self.profile.and_then(HearingProfile::sanitized),
            settings: self.settings.sanitized(),
            preview_gain_db,
            music,
            ..self
        }
    }
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            profile: None,
            settings: EqSettings::default(),
            test_length: TestLength::default(),
            ear_mode: EarMode::default(),
            export_channels: Channels::default(),
            preview_gain_db: -6.0,
            manual_layout: BandLayout::default(),
            custom_bands: "32, 64, 125, 250, 500, 1k, 2k, 4k, 8k, 16k".to_owned(),
            manual_headroom: true,
            music: Vec::new(),
        }
    }
}

/// A step of the flow, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Prepare,
    Test,
    Tune,
    Use,
}

impl Page {
    const ALL: [Page; 4] = [Page::Prepare, Page::Test, Page::Tune, Page::Use];

    fn label(self) -> &'static str {
        match self {
            Page::Prepare => "Prepare",
            Page::Test => "Test",
            Page::Tune => "Tune",
            Page::Use => "Use",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Silence before the next tone, so its timing can't be predicted.
    Pause { until: Instant },
    /// The stimulus is playing; answers are accepted.
    Playing { until: Instant },
    /// Waiting for an answer.
    Answering,
}

struct TestRun {
    test: HearingTest,
    phase: Phase,
    paused: bool,
    /// When the last answer was given (or the test started or resumed).
    last_answer_at: Instant,
    /// Total time between answers, for estimating the time left.
    timed_secs: f32,
    timed_trials: u32,
}

impl TestRun {
    /// Returns the expected time left, using this listener's own pace once known.
    fn time_left(&self) -> Duration {
        let secs_per_trial = if self.timed_trials >= 5 {
            self.timed_secs / self.timed_trials as f32
        } else {
            hearing_test::SECS_PER_TRIAL
        };
        Duration::from_secs_f32(self.test.trials_left_estimate() * secs_per_trial)
    }
}

/// Describes a remaining duration in whole minutes.
fn describe_time_left(left: Duration) -> String {
    let minutes = (left.as_secs_f32() / 60.0).ceil();
    if minutes <= 1.0 {
        "Less than a minute left".to_owned()
    } else {
        format!("About {minutes:.0} min left")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    PinkNoise,
    Music,
}

/// What the audio engine is currently set to play, so it's only told (and the
/// song only rewinds) when this changes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EngineSource {
    PinkNoise,
    Track(PathBuf),
}

/// A song decoded in memory.
struct LoadedTrack {
    path: PathBuf,
    track: Arc<Track>,
}

/// A song being decoded on a background thread.
struct Loading {
    path: PathBuf,
    result: Receiver<Result<Track, TrackError>>,
}

struct Preview {
    playing: bool,
    source: SourceKind,
    /// The selected song, as an index into `Saved::music`.
    selected: Option<usize>,
    track: Option<LoadedTrack>,
    loading: Option<Loading>,
    /// Songs that couldn't be decoded this session.
    failed: Vec<PathBuf>,
    /// Where the seek bar is being dragged to, in seconds; the preview is
    /// muted until it's released.
    scrub_secs: Option<f64>,
    /// Start playing once the selected song has loaded.
    play_when_loaded: bool,
    engine_source: Option<EngineSource>,
    eq_enabled: bool,
}

/// Where the EQ can be used, shown as expandable cards on the Use screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Destination {
    AppEq,
    Parametric,
    Files,
}

impl Destination {
    const ALL: [Destination; 3] = [
        Destination::AppEq,
        Destination::Parametric,
        Destination::Files,
    ];
}

/// The Earmark application.
pub struct App {
    saved: Saved,
    engine: Result<AudioEngine, String>,
    page: Page,
    run: Option<TestRun>,
    design: EqDesign,
    design_inputs: Option<(HearingProfile, EqSettings)>,
    /// One sentence describing `design`, recomputed with it.
    design_summary: String,
    /// The audio engine's filters are out of date (new design or new device).
    engine_needs_eq: bool,
    calibrating: bool,
    preview: Preview,
    toasts: Toasts,
    /// Which ears' curves the Tune chart shows: [left, right].
    visible_ears: [bool; 2],
    open_destination: Option<Destination>,
    /// Whether the Tune screen's expandable cards are open.
    show_advanced: bool,
    show_heard: bool,
    title_bar: TitleBar,
    /// Frames left in which to try fitting the window to the monitor; 0 once done.
    fit_attempts: u32,
    rng: fastrand::Rng,
}

impl App {
    /// Creates the app, restoring saved results and settings.
    pub fn new(creation: &eframe::CreationContext<'_>) -> Self {
        theme::install(&creation.egui_ctx);
        let saved: Saved = creation
            .storage
            .and_then(|storage| {
                eframe::get_value(storage, STORAGE_KEY)
                    .or_else(|| eframe::get_value(storage, LEGACY_STORAGE_KEY))
            })
            .unwrap_or_default();
        Self::from_saved(saved.sanitized(), AudioEngine::start())
    }

    fn from_saved(saved: Saved, engine: Result<AudioEngine, String>) -> Self {
        let page = if saved.profile.is_some() {
            Page::Tune
        } else {
            Page::Prepare
        };
        let app = Self {
            saved,
            engine,
            page,
            run: None,
            design: EqDesign::default(),
            design_inputs: None,
            design_summary: String::new(),
            engine_needs_eq: true,
            calibrating: false,
            preview: Preview {
                playing: false,
                source: SourceKind::PinkNoise,
                selected: None,
                track: None,
                loading: None,
                failed: Vec::new(),
                scrub_secs: None,
                play_when_loaded: false,
                engine_source: None,
                eq_enabled: true,
            },
            toasts: Toasts::default(),
            visible_ears: [true, true],
            open_destination: Some(Destination::AppEq),
            show_advanced: false,
            show_heard: false,
            title_bar: TitleBar::default(),
            fit_attempts: 10,
            rng: fastrand::Rng::new(),
        };
        app.send_preview_gain();
        app
    }

    fn send(&self, command: Command) {
        if let Ok(engine) = &self.engine {
            engine.send(command);
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.toasts.push(ToastKind::Info, text);
    }

    fn error(&mut self, text: impl Into<String>) {
        self.toasts.push(ToastKind::Error, text);
    }

    /// Recomputes the EQ when the results or settings change, and keeps the
    /// preview's filters in sync.
    fn update_design(&mut self) {
        // Compare by reference; only clone the inputs when they've changed.
        let current = self
            .saved
            .profile
            .as_ref()
            .map(|profile| (profile, &self.saved.settings));
        let unchanged = current == self.design_inputs.as_ref().map(|(p, s)| (p, s));
        if !unchanged {
            self.design = match current {
                Some((profile, settings)) => EqDesign::new(profile, settings),
                None => EqDesign::default(),
            };
            self.design_summary = self.design.summary(self.ears_together());
            self.design_inputs = current.map(|(p, s)| (p.clone(), *s));
            self.engine_needs_eq = true;
        }
        if self.engine_needs_eq {
            if let Ok(engine) = &self.engine {
                let processor = EqProcessor::new(
                    &self.design.left.filters,
                    &self.design.right.filters,
                    self.design.preamp_db,
                    engine.sample_rate(),
                );
                engine.send(Command::SetEq(Box::new(processor)));
            }
            self.engine_needs_eq = false;
        }
    }

    fn ears_together(&self) -> bool {
        self.saved.profile.as_ref().is_some_and(|p| p.ears_together)
    }

    fn send_preview_gain(&self) {
        self.send(Command::SetPreviewGain(self.saved.preview_gain_db));
    }

    fn stop_all_audio(&mut self) {
        self.calibrating = false;
        self.set_preview_playing(false);
        self.send(Command::StopTone);
    }

    fn reconnect_audio(&mut self) {
        // Drop the old stream before opening a new one.
        self.engine = Err("Reconnecting...".to_owned());
        self.engine = AudioEngine::start();
        self.engine_needs_eq = true;
        self.preview.playing = false;
        self.preview.engine_source = None;
        self.calibrating = false;
        self.send_preview_gain();
        // The new device may run at a different rate, so decode the song again.
        if self.preview.track.take().is_some()
            && let Some(index) = self.preview.selected
        {
            self.preview.selected = None;
            self.select_song(index);
        }
        match &self.engine {
            Ok(engine) => {
                let text = format!("Connected to {}.", engine.device_name());
                self.info(text);
            }
            Err(e) => {
                let text = e.clone();
                self.error(text);
            }
        }
    }

    fn go_to(&mut self, page: Page) {
        if page == self.page {
            return;
        }
        if let Some(run) = &mut self.run
            && page != Page::Test
        {
            run.paused = true;
            self.send(Command::StopTone);
        }
        if self.calibrating {
            self.calibrating = false;
            self.send(Command::StopTone);
        }
        self.page = page;
    }

    // ---- Prepare -----------------------------------------------------------

    fn toggle_calibration(&mut self) {
        self.calibrating = !self.calibrating;
        if self.calibrating {
            self.set_preview_playing(false);
            self.send(Command::PlayTone(Tone {
                ear: None,
                hz: 1000.0,
                level_db: hearing_test::CALIBRATION_LEVEL_DB,
                pattern: TonePattern::Continuous,
            }));
        } else {
            self.send(Command::StopTone);
        }
    }

    fn beep_in(&mut self, ear: hearing_test::Ear) {
        self.calibrating = false;
        self.send(Command::PlayTone(Tone {
            ear: Some(ear),
            hz: 1000.0,
            level_db: EAR_CHECK_LEVEL_DB,
            pattern: TonePattern::Pulsed { pips: 3 },
        }));
    }

    // ---- Test --------------------------------------------------------------

    fn start_test(&mut self) {
        self.stop_all_audio();
        self.run = Some(TestRun {
            test: HearingTest::new(self.saved.test_length, self.saved.ear_mode),
            phase: Phase::Pause {
                until: Instant::now() + Duration::from_secs(1),
            },
            paused: false,
            last_answer_at: Instant::now(),
            timed_secs: 0.0,
            timed_trials: 0,
        });
        self.page = Page::Test;
    }

    fn random_pause(&mut self) -> Duration {
        let (low, high) = PAUSE_SECS;
        Duration::from_secs_f32(low + self.rng.f32() * (high - low))
    }

    /// Moves the test along in time: starts tones and ends their playback.
    fn advance_test(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let Some(run) = &mut self.run else {
            return;
        };
        if run.paused {
            return;
        }
        match run.phase {
            Phase::Pause { until } if now >= until => self.play_current_trial(),
            Phase::Playing { until } if now >= until => run.phase = Phase::Answering,
            Phase::Pause { until } | Phase::Playing { until } => {
                ctx.request_repaint_after(until - now);
            }
            Phase::Answering => {}
        }
    }

    /// Plays the trial awaiting an answer, or finishes the test if none is left.
    ///
    /// Replaying doesn't change the trial: a silent check stays silent.
    fn play_current_trial(&mut self) {
        let Some(run) = &mut self.run else {
            return;
        };
        let Some(trial) = run.test.trial() else {
            self.finish_test();
            return;
        };
        let pattern = TonePattern::Pulsed {
            pips: PIPS_PER_TONE,
        };
        if let Stimulus::Tone { ear, hz, level_db } = trial.stimulus
            && let Ok(engine) = &self.engine
        {
            engine.send(Command::PlayTone(Tone {
                ear,
                hz,
                level_db,
                pattern,
            }));
        }
        let secs = pattern.duration_secs().unwrap_or(1.0);
        run.phase = Phase::Playing {
            until: Instant::now() + Duration::from_secs_f64(secs),
        };
    }

    fn replay(&mut self) {
        if let Some(run) = &self.run
            && run.phase == Phase::Answering
            && !run.paused
        {
            self.play_current_trial();
        }
    }

    fn answer(&mut self, response: Response) {
        let pause = self.random_pause();
        let Some(run) = &mut self.run else {
            return;
        };
        if !matches!(run.phase, Phase::Playing { .. } | Phase::Answering) || run.paused {
            return;
        }
        run.test.respond(response);
        let now = Instant::now();
        let since_last = now - run.last_answer_at;
        // Long gaps mean the listener stepped away; they'd skew the estimate.
        if since_last < Duration::from_secs(15) {
            run.timed_secs += since_last.as_secs_f32();
            run.timed_trials += 1;
        }
        run.last_answer_at = now;
        run.phase = Phase::Pause { until: now + pause };
        if run.test.is_finished() {
            self.finish_test();
        }
    }

    fn toggle_pause(&mut self) {
        if let Some(run) = &mut self.run {
            run.paused = !run.paused;
            run.last_answer_at = Instant::now();
            run.phase = Phase::Pause {
                until: Instant::now() + Duration::from_secs(1),
            };
        }
        self.send(Command::StopTone);
    }

    fn cancel_test(&mut self) {
        self.run = None;
        self.send(Command::StopTone);
        self.page = Page::Prepare;
        self.info("Test stopped. Your previous results are unchanged.");
    }

    fn finish_test(&mut self) {
        let Some(run) = self.run.take() else {
            return;
        };
        self.send(Command::StopTone);
        let profile = run.test.profile();
        if profile.is_unreliable() {
            self.error(
                "You reported hearing several silent checks, so these results may be off. \
                 Consider retaking the test somewhere quieter.",
            );
        } else {
            self.info("Test complete. Here's your EQ.");
        }
        self.saved.profile = Some(profile);
        self.page = Page::Tune;
    }

    // ---- Use ---------------------------------------------------------------

    fn toggle_preview(&mut self) {
        if self.preview.playing {
            self.set_preview_playing(false);
            return;
        }
        if self.preview.source == SourceKind::Music && !self.selected_song_ready() {
            match self.preview.selected {
                Some(_) if self.preview.loading.is_some() => self.preview.play_when_loaded = true,
                Some(index) => {
                    self.preview.play_when_loaded = true;
                    self.preview.selected = None;
                    self.select_song(index);
                }
                None if self.saved.music.is_empty() => self.add_music_from_dialog(),
                None => {
                    self.preview.play_when_loaded = true;
                    self.select_song(0);
                }
            }
            return;
        }
        self.sync_preview_source();
        self.set_preview_playing(true);
    }

    /// Plays or pauses the preview, keeping its position.
    fn set_preview_playing(&mut self, playing: bool) {
        if playing {
            self.calibrating = false;
            self.send(Command::StopTone);
            self.send(Command::SetEqEnabled(self.preview.eq_enabled));
        }
        self.preview.playing = playing;
        self.send(Command::SetPlaying(playing));
    }

    /// Switches between pink noise and music.
    fn set_preview_source(&mut self, source: SourceKind) {
        self.preview.source = source;
        if source == SourceKind::Music && !self.selected_song_ready() {
            if self.preview.playing {
                self.set_preview_playing(false);
                self.preview.play_when_loaded = true;
            }
            if self.preview.selected.is_none() && !self.saved.music.is_empty() {
                self.select_song(0);
            }
        }
        self.sync_preview_source();
    }

    fn selected_song_ready(&self) -> bool {
        let selected = self.preview.selected.and_then(|i| self.saved.music.get(i));
        matches!((&self.preview.track, selected), (Some(t), Some(p)) if &t.path == p)
    }

    /// Tells the engine what to play, if that has changed.
    fn sync_preview_source(&mut self) {
        let wanted = match self.preview.source {
            SourceKind::PinkNoise => Some(EngineSource::PinkNoise),
            SourceKind::Music if self.selected_song_ready() => self
                .preview
                .track
                .as_ref()
                .map(|t| EngineSource::Track(t.path.clone())),
            SourceKind::Music => None,
        };
        if wanted == self.preview.engine_source {
            return;
        }
        let source = match &wanted {
            Some(EngineSource::PinkNoise) => Some(PreviewSource::PinkNoise),
            Some(EngineSource::Track(_)) => self
                .preview
                .track
                .as_ref()
                .map(|t| PreviewSource::Track(Arc::clone(&t.track))),
            None => None,
        };
        self.send(Command::SetSource(source));
        self.preview.engine_source = wanted;
    }

    fn add_music_from_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Audio", &track::EXTENSIONS)
            .pick_files()
        {
            self.add_music(paths);
        }
    }

    /// Adds files to the music list and selects the first new one.
    fn add_music(&mut self, paths: Vec<PathBuf>) {
        let mut first_new = None;
        let mut skipped = 0;
        for path in paths {
            if !track::is_supported(&path) {
                skipped += 1;
                continue;
            }
            let index = match self.saved.music.iter().position(|p| *p == path) {
                Some(existing) => existing,
                None if self.saved.music.len() >= MAX_LIBRARY => {
                    skipped += 1;
                    continue;
                }
                None => {
                    self.saved.music.push(path);
                    self.saved.music.len() - 1
                }
            };
            first_new.get_or_insert(index);
        }
        if skipped > 0 {
            self.error(format!(
                "Skipped {skipped} file{} that Earmark can't play.",
                if skipped == 1 { "" } else { "s" }
            ));
        }
        if let Some(index) = first_new {
            self.preview.source = SourceKind::Music;
            if self.preview.playing {
                self.preview.play_when_loaded = true;
            }
            self.select_song(index);
        }
    }

    /// Selects a song and starts decoding it in the background.
    fn select_song(&mut self, index: usize) {
        let Some(path) = self.saved.music.get(index).cloned() else {
            return;
        };
        if self.preview.selected == Some(index)
            && (self.selected_song_ready()
                || self
                    .preview
                    .loading
                    .as_ref()
                    .is_some_and(|l| l.path == path))
        {
            return;
        }
        self.preview.selected = Some(index);
        self.preview.failed.retain(|p| *p != path);
        if self.preview.playing && self.preview.source == SourceKind::Music {
            // Keep playing once the new song is ready.
            self.set_preview_playing(false);
            self.preview.play_when_loaded = true;
        }
        let Ok(engine) = &self.engine else {
            return;
        };
        let sample_rate = engine.sample_rate();
        let (sender, result) = mpsc::channel();
        let thread_path = path.clone();
        let spawned = std::thread::Builder::new()
            .name("decode".to_owned())
            .spawn(move || {
                let _ = sender.send(Track::load(&thread_path, sample_rate));
            });
        match spawned {
            Ok(_) => self.preview.loading = Some(Loading { path, result }),
            Err(e) => self.error(format!("Couldn't start loading the song: {e}")),
        }
        self.sync_preview_source();
    }

    /// Picks up a song that has finished decoding.
    fn poll_loading(&mut self, ctx: &egui::Context) {
        let Some(loading) = &self.preview.loading else {
            return;
        };
        let result = match loading.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(50));
                return;
            }
            Err(TryRecvError::Disconnected) => Err(TrackError::Empty),
        };
        let path = loading.path.clone();
        self.preview.loading = None;
        let still_selected =
            self.preview.selected.and_then(|i| self.saved.music.get(i)) == Some(&path);
        match result {
            Ok(track) if still_selected => {
                self.preview.track = Some(LoadedTrack {
                    path,
                    track: Arc::new(track),
                });
                self.sync_preview_source();
                if std::mem::take(&mut self.preview.play_when_loaded) {
                    self.set_preview_playing(true);
                }
            }
            Ok(_) => {}
            Err(e) => {
                let name = track::display_name(&path);
                self.preview.failed.push(path);
                self.preview.play_when_loaded = false;
                self.error(format!("Couldn't play {name}: {e}."));
            }
        }
    }

    /// Removes a song from the music list.
    fn remove_song(&mut self, index: usize) {
        if index >= self.saved.music.len() {
            return;
        }
        let path = self.saved.music.remove(index);
        match self.preview.selected {
            Some(selected) if selected == index => {
                self.preview.selected = None;
                self.preview.track = None;
                self.preview.loading = None;
                if self.preview.source == SourceKind::Music && self.preview.playing {
                    self.set_preview_playing(false);
                }
                self.sync_preview_source();
            }
            Some(selected) if selected > index => self.preview.selected = Some(selected - 1),
            _ => {}
        }
        self.preview.failed.retain(|p| *p != path);
    }

    /// Returns the current song's position and length in seconds, if it's ready.
    fn song_position(&self) -> Option<(f64, f64)> {
        let loaded = self.preview.track.as_ref()?;
        if self.preview.source != SourceKind::Music || !self.selected_song_ready() {
            return None;
        }
        let frames = self.engine.as_ref().map_or(0, AudioEngine::position_frames);
        let position = frames as f64 / f64::from(loaded.track.sample_rate());
        let length = loaded.track.duration_secs();
        Some((position.min(length), length))
    }

    fn seek_to(&mut self, secs: f64) {
        if let Ok(engine) = &self.engine {
            let frame = (secs.max(0.0) * f64::from(engine.sample_rate())) as usize;
            engine.send(Command::Seek(frame));
        }
    }

    /// Adds audio files dropped onto the window, and returns true while files
    /// are being dragged over it.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) -> bool {
        let (dropped, hovering) = ctx.input(|input| {
            let dropped: Vec<PathBuf> = input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect();
            (dropped, !input.raw.hovered_files.is_empty())
        });
        if !dropped.is_empty() {
            self.add_music(dropped);
            if !self.design.is_empty() {
                self.go_to(Page::Use);
            }
        }
        hovering
    }

    fn save_text(&mut self, file_name: &str, kind: &str, contents: String) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(file_name)
            .add_filter("Text", &["txt"])
            .save_file()
        else {
            return;
        };
        match fs::write(&path, contents) {
            Ok(()) => self.info(format!("Saved {kind} to {}.", path.display())),
            Err(e) => self.error(format!("Couldn't save {}: {e}", path.display())),
        }
    }

    fn save_results(&mut self) {
        let Some(profile) = &self.saved.profile else {
            return;
        };
        let json = match serde_json::to_string_pretty(profile) {
            Ok(json) => json,
            Err(e) => {
                self.error(format!("Couldn't encode results: {e}"));
                return;
            }
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("hearing-test.json")
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            return;
        };
        match fs::write(&path, json) {
            Ok(()) => self.info(format!("Saved results to {}.", path.display())),
            Err(e) => self.error(format!("Couldn't save {}: {e}", path.display())),
        }
    }

    fn load_results(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        else {
            return;
        };
        let too_big = fs::metadata(&path).is_ok_and(|m| m.len() > MAX_RESULTS_FILE_BYTES);
        let loaded = if too_big {
            Err("it's too large to be a results file".to_owned())
        } else {
            fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    serde_json::from_str::<HearingProfile>(&text).map_err(|e| e.to_string())
                })
                .and_then(|profile| {
                    profile
                        .sanitized()
                        .ok_or_else(|| "it has no valid measurements".to_owned())
                })
        };
        match loaded {
            Ok(profile) => {
                self.saved.profile = Some(profile);
                self.page = Page::Tune;
                self.info(format!("Loaded results from {}.", path.display()));
            }
            Err(e) => self.error(format!("Couldn't load {}: {e}", path.display())),
        }
    }

    // ---- Frame -------------------------------------------------------------

    /// On the first frames, shrinks the window if its preferred size doesn't
    /// fit the monitor (such as a laptop at 125% scaling), keeping it centred.
    fn fit_window_to_monitor(&mut self, ctx: &egui::Context) {
        if self.fit_attempts == 0 {
            return;
        }
        self.fit_attempts -= 1;
        let (monitor, outer, inner) = ctx.input(|input| {
            let viewport = input.viewport();
            (
                viewport.monitor_size,
                viewport.outer_rect,
                viewport.inner_rect,
            )
        });
        let (Some(monitor), Some(outer), Some(inner)) = (monitor, outer, inner) else {
            // Not known yet; try again next frame.
            ctx.request_repaint();
            return;
        };
        self.fit_attempts = 0;
        let chrome = outer.size() - inner.size();
        let Some(size) = fitted_inner_size(PREFERRED_SIZE, monitor, chrome) else {
            return;
        };
        // eframe centred the window on its monitor, so the monitor's top-left
        // is the window's centre minus half the monitor size.
        let monitor_min = outer.center() - monitor / 2.0;
        let outer_size = size + chrome;
        let work_height = monitor.y - TASKBAR_ALLOWANCE;
        let position = egui::pos2(
            monitor_min.x + (monitor.x - outer_size.x) / 2.0,
            monitor_min.y + ((work_height - outer_size.y) / 2.0).max(0.0),
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
    }

    /// Draws the whole window: the step header, the current screen, and toasts.
    fn show(&mut self, ui: &mut Ui) {
        let palette = Palette::of(ui);
        let focus_mode = self.page == Page::Test && self.run.is_some();
        let has_actions = match self.page {
            Page::Prepare => true,
            Page::Tune => self.saved.profile.is_some(),
            Page::Test => !focus_mode,
            Page::Use => false,
        };
        if has_actions {
            // The screen's main buttons stay visible however small the window is.
            egui::Panel::bottom("actions")
                .frame(
                    egui::Frame::new()
                        .fill(palette.surface)
                        .inner_margin(egui::Margin::symmetric(0, 12)),
                )
                .show(ui, |ui| {
                    self.content_column(ui, |app, ui| match app.page {
                        Page::Prepare | Page::Test => app.prepare_actions(ui),
                        Page::Tune => app.tune_actions(ui),
                        Page::Use => {}
                    });
                });
        }
        if !focus_mode {
            egui::Panel::top("steps")
                .frame(
                    egui::Frame::new()
                        .fill(palette.surface)
                        .inner_margin(egui::Margin::symmetric(16, 10)),
                )
                .show(ui, |ui| self.step_header(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.background))
            .show(ui, |ui| {
                if focus_mode {
                    self.content_column(ui, |app, ui| app.test_screen(ui));
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add_space(28.0);
                        self.content_column(ui, |app, ui| match app.page {
                            Page::Prepare => app.prepare_screen(ui),
                            Page::Test => app.test_intro(ui),
                            Page::Tune => app.tune_screen(ui),
                            Page::Use => app.use_screen(ui),
                        });
                        ui.add_space(28.0);
                    });
            });
        if self.handle_dropped_files(ui.ctx()) {
            drop_overlay(ui.ctx(), &palette);
        }
        self.toasts.show(ui.ctx());
    }

    /// Lays out `add_contents` in a centred column of at most [`CONTENT_WIDTH`].
    fn content_column(&mut self, ui: &mut Ui, add_contents: impl FnOnce(&mut Self, &mut Ui)) {
        let width = (ui.available_width() - 48.0).clamp(280.0, CONTENT_WIDTH);
        let margin = ((ui.available_width() - width) / 2.0).max(0.0);
        ui.horizontal_top(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                add_contents(self, ui);
            });
        });
    }

    fn step_header(&mut self, ui: &mut Ui) {
        let has_results = self.saved.profile.is_some();
        let current = Page::ALL.iter().position(|&p| p == self.page).unwrap_or(0);
        let steps: Vec<Step<'_>> = Page::ALL
            .iter()
            .enumerate()
            .map(|(i, &page)| {
                // Ticks mark the steps before where you are, so the header
                // always reads as your position in the flow.
                let state = match i.cmp(&current) {
                    std::cmp::Ordering::Less => StepState::Done,
                    std::cmp::Ordering::Equal => StepState::Current,
                    std::cmp::Ordering::Greater => StepState::Upcoming,
                };
                let enabled = match page {
                    Page::Prepare | Page::Test => true,
                    Page::Tune | Page::Use => has_results,
                };
                Step {
                    label: page.label(),
                    state,
                    enabled,
                }
            })
            .collect();
        if let Some(index) = widgets::stepper(ui, &steps) {
            self.go_to(Page::ALL[index]);
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(engine) = &self.engine
            && let Some(error) = engine.take_error()
        {
            self.error(format!("{error} Try \"Reconnect\" on the Prepare screen."));
        }
        if let Ok(engine) = &self.engine {
            engine.collect_garbage();
        }
        self.poll_loading(ctx);
        if self.preview.playing && self.song_position().is_some() {
            // Keep the seek bar moving.
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        self.advance_test(ctx);
        self.update_design();
        self.fit_window_to_monitor(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let focus = self.page == Page::Test && self.run.is_some();
        self.title_bar.update(frame, ui.visuals().dark_mode, focus);
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.saved);
    }
}

/// Dims the window and invites a drop while files are dragged over it.
fn drop_overlay(ctx: &egui::Context, palette: &Palette) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("drop_overlay"),
    ));
    painter.rect_filled(screen, 0.0, theme::fade(palette.background, 0.85));
    painter.rect_stroke(
        screen.shrink(16.0),
        egui::CornerRadius::same(theme::CARD_RADIUS),
        egui::Stroke::new(2.0, palette.accent),
        egui::StrokeKind::Inside,
    );
    painter.text(
        screen.center(),
        egui::Align2::CENTER_CENTER,
        "Drop music to preview your EQ with it",
        theme::semibold(theme::TITLE_SIZE),
        palette.text,
    );
}

/// Returns a smaller inner size if `preferred` plus the window `chrome`
/// doesn't fit on `monitor` above the taskbar, or `None` if it fits.
fn fitted_inner_size(
    preferred: egui::Vec2,
    monitor: egui::Vec2,
    chrome: egui::Vec2,
) -> Option<egui::Vec2> {
    let available = egui::vec2(monitor.x - 32.0, monitor.y - TASKBAR_ALLOWANCE) - chrome;
    let fitted = preferred.min(available);
    (fitted != preferred).then_some(fitted.max(egui::vec2(640.0, 520.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hearing_test::{Ear, Measurement};
    use crate::reference;
    use crate::staircase::Threshold;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// A listener with a mild high-frequency loss, worse in the right ear, who
    /// can't hear 14 kHz on the right at all.
    fn simulated_profile() -> HearingProfile {
        let mut measurements = Vec::new();
        for ear in Ear::BOTH {
            for hz in TestLength::Standard.frequencies() {
                let loss = if hz >= 3000.0 {
                    (hz / 3000.0).log2() * 8.0
                } else {
                    0.0
                };
                let extra = if ear == Ear::Right { 4.0 } else { 0.0 };
                let threshold = if ear == Ear::Right && hz > 12_000.0 {
                    Threshold::BeyondMax
                } else {
                    let level = -75.0 + reference::threshold_db(hz) + loss + extra;
                    Threshold::Level((level / 5.0).round() * 5.0)
                };
                measurements.push(Measurement { ear, hz, threshold });
            }
        }
        HearingProfile {
            measurements,
            catch_trials: 5,
            false_alarms: 0,
            ears_together: false,
        }
    }

    fn together_profile() -> HearingProfile {
        let mut profile = simulated_profile();
        // Both ears hold the left ear's results, as a together test records.
        let left: Vec<Measurement> = profile.ear(Ear::Left).copied().collect();
        profile.measurements = left
            .iter()
            .copied()
            .chain(left.iter().map(|m| Measurement {
                ear: Ear::Right,
                ..*m
            }))
            .collect();
        profile.ears_together = true;
        profile
    }

    fn app_with(profile: Option<HearingProfile>) -> App {
        let saved = Saved {
            profile,
            ..Default::default()
        };
        App::from_saved(saved, Err("No audio in tests.".to_owned()))
    }

    fn harness(app: App, size: egui::Vec2, dark: bool) -> Harness<'static, App> {
        let theme = if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        Harness::builder()
            .with_size(size)
            .with_theme(theme)
            .build_ui_state(
                |ui, app: &mut App| {
                    theme::install_once(ui.ctx());
                    app.update_design();
                    app.show(ui);
                },
                app,
            )
    }

    fn screenshot(profile: Option<&HearingProfile>, page: Page, name: &str, height: f32) {
        for dark in [false, true] {
            let mut app = app_with(profile.cloned());
            app.page = page;
            if page == Page::Use {
                app.open_destination = Some(Destination::AppEq);
                // A short music list with the second song loaded.
                app.saved.music = [
                    "Daft Punk - Digital Love",
                    "Bonobo - Kerala",
                    "Nils Frahm - Says",
                ]
                .iter()
                .map(|name| PathBuf::from(format!("C:/Music/{name}.flac")))
                .collect();
                app.preview.source = SourceKind::Music;
                app.preview.selected = Some(1);
                app.preview.track = Some(LoadedTrack {
                    path: app.saved.music[1].clone(),
                    track: Arc::new(Track::from_frames_for_test(48_000 * 225, 48_000)),
                });
            }
            let mut harness = harness(app, egui::vec2(980.0, height), dark);
            if page == Page::Test && name == "test" {
                harness.state_mut().start_test();
                let run = harness.state_mut().run.as_mut().expect("test started");
                for _ in 0..70 {
                    let Some(trial) = run.test.trial() else { break };
                    let response = match trial.stimulus {
                        Stimulus::Tone { level_db, .. } if level_db > -70.0 => Response::Heard,
                        _ => Response::NotHeard,
                    };
                    run.test.respond(response);
                }
                run.phase = Phase::Answering;
            }
            harness.run_steps(4);
            let mode = if dark { "dark" } else { "light" };
            let out = std::env::temp_dir().join(format!("earmark-{name}-{mode}.png"));
            harness
                .render()
                .expect("render with wgpu")
                .save(&out)
                .expect("save screenshot");
        }
    }

    #[test]
    #[ignore = "writes screenshots for manual inspection"]
    fn screenshots() {
        let separate = simulated_profile();
        let together = together_profile();
        screenshot(None, Page::Prepare, "prepare", 1000.0);
        screenshot(None, Page::Test, "test", 720.0);
        screenshot(Some(&separate), Page::Test, "test-intro", 800.0);
        screenshot(Some(&separate), Page::Tune, "tune", 1000.0);
        screenshot(Some(&together), Page::Tune, "tune-together", 1000.0);
        screenshot(Some(&separate), Page::Use, "use", 1200.0);
    }

    #[test]
    fn window_shrinks_only_when_it_doesnt_fit() {
        let chrome = egui::vec2(2.0, 32.0);
        assert_eq!(
            fitted_inner_size(PREFERRED_SIZE, egui::vec2(1920.0, 1200.0), chrome),
            None
        );
        // 1080p at 125% scaling: 1536 x 864 points.
        let fitted = fitted_inner_size(PREFERRED_SIZE, egui::vec2(1536.0, 864.0), chrome)
            .expect("too tall for this monitor");
        assert_eq!(fitted, egui::vec2(1000.0, 864.0 - TASKBAR_ALLOWANCE - 32.0));
    }

    #[test]
    fn replay_repeats_the_same_trial_without_answering() {
        let mut app = app_with(None);
        app.start_test();
        let run = app.run.as_mut().expect("test started");
        let trial = run.test.trial();
        run.phase = Phase::Answering;

        app.replay();
        let run = app.run.as_mut().expect("still running");
        assert!(
            matches!(run.phase, Phase::Playing { .. }),
            "{:?}",
            run.phase
        );
        assert_eq!(run.test.trial(), trial);
        assert_eq!(run.test.fraction_done(), 0.0);

        // Replaying mid-tone or while paused does nothing.
        let phase = run.phase;
        app.replay();
        assert_eq!(app.run.as_ref().expect("still running").phase, phase);
    }

    #[test]
    fn music_list_skips_non_audio_and_duplicates() {
        let mut app = app_with(Some(simulated_profile()));
        app.add_music(vec![
            PathBuf::from("C:/Music/One.mp3"),
            PathBuf::from("C:/Music/notes.txt"),
            PathBuf::from("C:/Music/Two.flac"),
        ]);
        app.add_music(vec![PathBuf::from("C:/Music/One.mp3")]);
        assert_eq!(app.saved.music.len(), 2);
        assert_eq!(app.preview.source, SourceKind::Music);
        // Re-adding a song selects it rather than duplicating it.
        assert_eq!(app.preview.selected, Some(0));
        assert_eq!(app.toasts.latest().map(|t| t.0), Some(ToastKind::Error));
    }

    #[test]
    fn removing_a_song_keeps_the_right_one_selected() {
        let mut app = app_with(Some(simulated_profile()));
        app.add_music(
            ["A.mp3", "B.mp3", "C.mp3"]
                .iter()
                .map(|name| PathBuf::from(format!("C:/Music/{name}")))
                .collect(),
        );
        app.select_song(2);
        app.remove_song(0);
        assert_eq!(app.preview.selected, Some(1), "still C");
        assert_eq!(app.saved.music[1], PathBuf::from("C:/Music/C.mp3"));
        app.remove_song(1);
        assert_eq!(app.preview.selected, None, "removed the selected song");
        assert!(!app.preview.playing);
    }

    #[test]
    fn stopping_the_test_keeps_previous_results() {
        let mut app = app_with(Some(simulated_profile()));
        app.start_test();
        assert_eq!(app.page, Page::Test);
        app.cancel_test();
        assert!(app.run.is_none());
        assert!(app.saved.profile.is_some());
        assert_eq!(app.page, Page::Prepare);
        assert_eq!(app.toasts.latest().map(|t| t.0), Some(ToastKind::Info));
    }

    #[test]
    fn test_step_opens_the_intro_without_starting() {
        let mut harness = harness(
            app_with(Some(simulated_profile())),
            egui::vec2(980.0, 1000.0),
            false,
        );
        harness.run();
        harness.get_by_label("Step 2: Test").click();
        harness.run();
        assert_eq!(harness.state().page, Page::Test);
        assert!(harness.state().run.is_none(), "no test started yet");
        // Steps after the current one aren't ticked, even with results.
        harness.get_by_label("Step 1: Prepare").click();
        harness.run();
        assert_eq!(harness.state().page, Page::Prepare);
    }

    #[test]
    fn steps_need_results_before_tune_and_use() {
        let mut harness = harness(app_with(None), egui::vec2(980.0, 900.0), false);
        harness.run();
        harness.get_by_label("Step 3: Tune").click();
        harness.run();
        assert_eq!(harness.state().page, Page::Prepare);

        let mut harness = harness_with_results();
        harness.get_by_label("Step 4: Use").click();
        harness.run();
        assert_eq!(harness.state().page, Page::Use);
        harness.get_by_label("Step 1: Prepare").click();
        harness.run();
        assert_eq!(harness.state().page, Page::Prepare);
    }

    fn harness_with_results() -> Harness<'static, App> {
        let mut harness = harness(
            app_with(Some(simulated_profile())),
            egui::vec2(980.0, 1200.0),
            false,
        );
        harness.run();
        harness
    }

    #[test]
    fn test_options_are_selectable() {
        let mut harness = harness(app_with(None), egui::vec2(980.0, 1000.0), false);
        harness.run();
        harness.get_by_label("Both ears together").click();
        harness.run();
        assert_eq!(harness.state().saved.ear_mode, EarMode::Together);
        harness
            .get_by_label(&format!(
                "Detailed · {} min",
                TestLength::Detailed.minutes(EarMode::Together)
            ))
            .click();
        harness.run();
        assert_eq!(harness.state().saved.test_length, TestLength::Detailed);
    }

    #[test]
    fn destinations_expand_one_at_a_time() {
        let mut harness = harness_with_results();
        harness.state_mut().page = Page::Use;
        harness.state_mut().open_destination = None;
        harness.run();
        harness.get_by_label("Spotify or headphone app").click();
        harness.run();
        assert_eq!(harness.state().open_destination, Some(Destination::AppEq));
        harness.get_by_label("Export files").click();
        harness.run();
        assert_eq!(harness.state().open_destination, Some(Destination::Files));
        harness.get_by_label("Export files").click();
        harness.run();
        assert_eq!(harness.state().open_destination, None);
    }
}
