//! Real-time audio output: test tones and the EQ preview.
//!
//! The UI thread sends [`Command`]s over a channel; the audio callback drains it
//! without blocking at the start of every buffer. Anything the callback replaces
//! (a finished song, old filters) is sent back to be freed on the UI thread, so
//! the audio thread never frees large allocations.

use std::f64::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, OutputCallbackInfo, SampleFormat, SizedSample, Stream, StreamConfig};

use crate::biquad::{Biquad, Filter};
use crate::hearing_test::Ear;
use crate::pink_noise::PinkNoise;
use crate::track::Track;

/// Length of each pip in a pulsed tone, in seconds.
const PIP_SECS: f64 = 0.18;
/// Silence between pips, in seconds.
const PIP_GAP_SECS: f64 = 0.12;
/// Fade in and out time that keeps tones free of clicks, in seconds.
const RAMP_SECS: f64 = 0.02;
/// Fade time when the preview starts or pauses, in seconds.
const PREVIEW_FADE_SECS: f32 = 0.015;

/// How a test tone is played.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TonePattern {
    /// Short beeps, which are easier to notice near threshold than a steady tone.
    Pulsed { pips: u32 },
    /// Plays until [`Command::StopTone`].
    Continuous,
}

impl TonePattern {
    /// Returns how long the tone lasts, in seconds, or `None` if it doesn't end.
    #[must_use]
    pub fn duration_secs(self) -> Option<f64> {
        match self {
            TonePattern::Pulsed { pips } => {
                Some(f64::from(pips) * (PIP_SECS + PIP_GAP_SECS) - PIP_GAP_SECS)
            }
            TonePattern::Continuous => None,
        }
    }
}

/// A sine tone to play.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    /// The ear to play in, or both.
    pub ear: Option<Ear>,
    pub hz: f32,
    /// Peak level in dBFS.
    pub level_db: f32,
    pub pattern: TonePattern,
}

/// What the EQ preview plays.
#[derive(Debug, Clone)]
pub enum PreviewSource {
    PinkNoise,
    Track(Arc<Track>),
}

/// Filters for both channels, built for the engine's sample rate.
#[derive(Debug, Clone)]
pub struct EqProcessor {
    left: Vec<Biquad>,
    right: Vec<Biquad>,
    preamp: f32,
}

impl EqProcessor {
    /// Builds the filters for audio at `sample_rate`.
    pub fn new(left: &[Filter], right: &[Filter], preamp_db: f32, sample_rate: u32) -> Self {
        let rate = sample_rate as f32;
        Self {
            left: left.iter().map(|f| Biquad::new(f, rate)).collect(),
            right: right.iter().map(|f| Biquad::new(f, rate)).collect(),
            preamp: db_to_gain(preamp_db),
        }
    }

    fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let run = |filters: &mut [Biquad], sample: f32| {
            filters
                .iter_mut()
                .fold(sample, |sample, filter| filter.process(sample))
        };
        (
            run(&mut self.left, left * self.preamp),
            run(&mut self.right, right * self.preamp),
        )
    }
}

/// A request to the audio thread.
#[derive(Debug)]
pub enum Command {
    PlayTone(Tone),
    /// Fades out the current tone.
    StopTone,
    /// Replaces the preview's source and rewinds to its start. Doesn't change
    /// whether the preview is playing.
    SetSource(Option<PreviewSource>),
    /// Plays or pauses the preview, keeping the position.
    SetPlaying(bool),
    /// Moves the preview to a frame of the current track. It fades out,
    /// jumps, and fades back in, so the jump doesn't click.
    Seek(usize),
    /// Silences the preview and holds its position (while scrubbing).
    SetMuted(bool),
    SetEq(Box<EqProcessor>),
    /// Turns the EQ filters on or off. The preamp always applies, so switching
    /// compares the sound at matched levels.
    SetEqEnabled(bool),
    /// Preview volume in dB.
    SetPreviewGain(f32),
}

/// An open output stream on the default device.
pub struct AudioEngine {
    _stream: Stream,
    commands: Sender<Command>,
    errors: Receiver<String>,
    /// Things the audio thread replaced, to be freed here.
    garbage: Receiver<Garbage>,
    /// The preview's current frame in its track, written by the audio thread.
    position: Arc<AtomicUsize>,
    sample_rate: u32,
    channels: u16,
    device_name: String,
}

impl AudioEngine {
    /// Opens the default output device and starts a silent stream.
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No audio output device found.")?;
        let device_name = device
            .description()
            .map_or_else(|_| "Unknown device".to_owned(), |d| d.name().to_owned());
        let supported = device
            .default_output_config()
            .map_err(|e| format!("Couldn't read the output format: {e}"))?;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let (commands, command_receiver) = mpsc::channel();
        let (error_sender, errors) = mpsc::channel();
        let (garbage_sender, garbage) = mpsc::channel();
        let position = Arc::new(AtomicUsize::new(0));
        let renderer = Renderer::new(
            &config,
            command_receiver,
            garbage_sender,
            Arc::clone(&position),
        );
        let stream = match format {
            SampleFormat::I16 => build::<i16>(&device, &config, renderer, error_sender),
            SampleFormat::I32 => build::<i32>(&device, &config, renderer, error_sender),
            SampleFormat::U16 => build::<u16>(&device, &config, renderer, error_sender),
            SampleFormat::F64 => build::<f64>(&device, &config, renderer, error_sender),
            _ => build::<f32>(&device, &config, renderer, error_sender),
        }?;
        stream
            .play()
            .map_err(|e| format!("Couldn't start audio: {e}"))?;
        Ok(Self {
            _stream: stream,
            commands,
            errors,
            garbage,
            position,
            sample_rate: config.sample_rate,
            channels: config.channels,
            device_name,
        })
    }

    /// Sends a command to the audio thread.
    pub fn send(&self, command: Command) {
        // The receiver lives as long as the stream, which `self` owns.
        let _ = self.commands.send(command);
    }

    /// Returns the preview's position in its track, in frames.
    #[must_use]
    pub fn position_frames(&self) -> usize {
        self.position.load(Ordering::Relaxed)
    }

    /// Frees whatever the audio thread has replaced since the last call.
    pub fn collect_garbage(&self) {
        for garbage in self.garbage.try_iter() {
            drop(garbage);
        }
    }

    /// Returns the most recent stream error, if any occurred since the last call.
    pub fn take_error(&self) -> Option<String> {
        self.errors.try_iter().last()
    }

    /// Returns the output sample rate in Hz.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Returns the number of output channels.
    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Returns the output device's name.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut renderer: Renderer,
    errors: Sender<String>,
) -> Result<Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _: &OutputCallbackInfo| renderer.render(data),
            move |error| {
                let _ = errors.send(format!("Audio error: {error}"));
            },
            None,
        )
        .map_err(|e| format!("Couldn't open the audio device: {e}"))
}

#[derive(Debug)]
struct ToneVoice {
    tone: Tone,
    /// Peak amplitude from `tone.level_db`, computed once.
    amplitude: f64,
    phase: f64,
    elapsed: u64,
    released_at: Option<u64>,
}

/// Something the audio thread no longer needs, sent back to be freed.
#[derive(Debug)]
enum Garbage {
    // Held only so they're dropped on the UI thread.
    #[cfg_attr(not(test), expect(dead_code, reason = "only dropped"))]
    Source(PreviewSource),
    #[expect(dead_code, reason = "only dropped")]
    Eq(Box<EqProcessor>),
}

struct Renderer {
    sample_rate: f64,
    channels: usize,
    commands: Receiver<Command>,
    garbage: Sender<Garbage>,
    tone: Option<ToneVoice>,
    source: Option<PreviewSource>,
    track_position: usize,
    position: Arc<AtomicUsize>,
    playing: bool,
    muted: bool,
    /// A jump waiting for the fade-out to finish.
    pending_seek: Option<usize>,
    /// Preview fade level, 0 (silent) to 1, moving towards whether it should
    /// be heard.
    fade: f32,
    fade_step: f32,
    pink: [PinkNoise; 2],
    eq: Option<Box<EqProcessor>>,
    eq_enabled: bool,
    preview_gain: f32,
}

impl Renderer {
    fn new(
        config: &StreamConfig,
        commands: Receiver<Command>,
        garbage: Sender<Garbage>,
        position: Arc<AtomicUsize>,
    ) -> Self {
        let sample_rate = config.sample_rate as f32;
        Self {
            sample_rate: f64::from(config.sample_rate),
            channels: usize::from(config.channels),
            commands,
            garbage,
            tone: None,
            source: None,
            track_position: 0,
            position,
            playing: false,
            muted: false,
            pending_seek: None,
            fade: 0.0,
            fade_step: 1.0 / (PREVIEW_FADE_SECS * sample_rate).max(1.0),
            pink: [PinkNoise::new(1), PinkNoise::new(2)],
            eq: None,
            eq_enabled: true,
            preview_gain: 1.0,
        }
    }

    fn render<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T]) {
        self.handle_commands();
        for frame in data.chunks_mut(self.channels) {
            let (left, right) = self.next_frame();
            if frame.len() == 1 {
                frame[0] = T::from_sample(left + right);
                continue;
            }
            for (channel, sample) in frame.iter_mut().enumerate() {
                let value = match channel {
                    0 => left,
                    1 => right,
                    _ => 0.0,
                };
                *sample = T::from_sample(value);
            }
        }
        self.position.store(self.track_position, Ordering::Relaxed);
    }

    fn handle_commands(&mut self) {
        while let Ok(command) = self.commands.try_recv() {
            match command {
                Command::PlayTone(tone) => {
                    self.tone = Some(ToneVoice {
                        tone,
                        amplitude: f64::from(db_to_gain(tone.level_db)),
                        phase: 0.0,
                        elapsed: 0,
                        released_at: None,
                    });
                }
                Command::StopTone => {
                    if let Some(voice) = &mut self.tone {
                        voice.released_at.get_or_insert(voice.elapsed);
                    }
                }
                Command::SetSource(source) => {
                    if let Some(old) = std::mem::replace(&mut self.source, source) {
                        let _ = self.garbage.send(Garbage::Source(old));
                    }
                    self.track_position = 0;
                }
                Command::SetPlaying(playing) => self.playing = playing,
                Command::Seek(frame) => self.pending_seek = Some(frame),
                Command::SetMuted(muted) => self.muted = muted,
                Command::SetEq(eq) => {
                    if let Some(old) = self.eq.replace(eq) {
                        let _ = self.garbage.send(Garbage::Eq(old));
                    }
                }
                Command::SetEqEnabled(enabled) => self.eq_enabled = enabled,
                Command::SetPreviewGain(db) => self.preview_gain = db_to_gain(db),
            }
        }
    }

    fn next_frame(&mut self) -> (f32, f32) {
        let (mut left, mut right) = self.preview_frame();
        if let Some(voice) = &mut self.tone {
            match voice_sample(voice, self.sample_rate) {
                Some(sample) => match voice.tone.ear {
                    Some(Ear::Left) => left += sample,
                    Some(Ear::Right) => right += sample,
                    None => {
                        left += sample;
                        right += sample;
                    }
                },
                None => self.tone = None,
            }
        }
        (left, right)
    }

    fn preview_frame(&mut self) -> (f32, f32) {
        // Fade rather than cut, so play/pause, muting and jumps don't click.
        let audible = self.playing && !self.muted && self.pending_seek.is_none();
        let target = if audible { 1.0 } else { 0.0 };
        if self.fade < target {
            self.fade = (self.fade + self.fade_step).min(1.0);
        } else if self.fade > target {
            self.fade = (self.fade - self.fade_step).max(0.0);
        }
        if self.fade == 0.0 {
            // Silent: jump now if asked, and hold the position.
            if let Some(frame) = self.pending_seek.take() {
                self.track_position = frame;
            }
            return (0.0, 0.0);
        }
        let (left, right) = match &self.source {
            None => return (0.0, 0.0),
            Some(PreviewSource::PinkNoise) => {
                (self.pink[0].next_sample(), self.pink[1].next_sample())
            }
            Some(PreviewSource::Track(track)) => {
                if track.frames() == 0 {
                    return (0.0, 0.0);
                }
                if self.track_position >= track.frames() {
                    self.track_position = 0;
                }
                let frame = track.frame(self.track_position);
                self.track_position += 1;
                frame
            }
        };
        let gain = self.preview_gain * self.fade;
        let (left, right) = (left * gain, right * gain);
        match &mut self.eq {
            Some(eq) => {
                // Keep the filters running even when bypassed, so switching
                // them back on doesn't start from a stale state and click.
                let filtered = eq.process(left, right);
                if self.eq_enabled {
                    filtered
                } else {
                    (left * eq.preamp, right * eq.preamp)
                }
            }
            None => (left, right),
        }
    }
}

/// Returns the voice's next sample, or `None` once it has finished.
fn voice_sample(voice: &mut ToneVoice, sample_rate: f64) -> Option<f32> {
    let t = voice.elapsed as f64 / sample_rate;
    let mut envelope = match voice.tone.pattern {
        TonePattern::Pulsed { pips } => {
            let cycle = PIP_SECS + PIP_GAP_SECS;
            let pip = (t / cycle).floor();
            if pip >= f64::from(pips) {
                return None;
            }
            let within = t - pip * cycle;
            if within > PIP_SECS {
                0.0
            } else {
                ramp(within) * ramp(PIP_SECS - within)
            }
        }
        TonePattern::Continuous => ramp(t),
    };
    if let Some(released_at) = voice.released_at {
        let since_release = (voice.elapsed - released_at) as f64 / sample_rate;
        if since_release >= RAMP_SECS {
            return None;
        }
        envelope *= ramp(RAMP_SECS - since_release);
    }
    let sample = voice.amplitude * envelope * voice.phase.sin();
    voice.phase = (voice.phase + TAU * f64::from(voice.tone.hz) / sample_rate) % TAU;
    voice.elapsed += 1;
    Some(sample as f32)
}

/// A raised-cosine ramp from 0 to 1 over [`RAMP_SECS`].
fn ramp(t: f64) -> f64 {
    if t >= RAMP_SECS {
        1.0
    } else if t <= 0.0 {
        0.0
    } else {
        0.5 - 0.5 * (std::f64::consts::PI * t / RAMP_SECS).cos()
    }
}

/// Converts decibels to a linear amplitude factor.
#[must_use]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> (
        Renderer,
        Sender<Command>,
        Receiver<Garbage>,
        Arc<AtomicUsize>,
    ) {
        let config = StreamConfig {
            channels: 2,
            sample_rate: 48_000,
            buffer_size: cpal::BufferSize::Default,
        };
        let (commands, command_receiver) = mpsc::channel();
        let (garbage_sender, garbage) = mpsc::channel();
        let position = Arc::new(AtomicUsize::new(0));
        let renderer = Renderer::new(
            &config,
            command_receiver,
            garbage_sender,
            Arc::clone(&position),
        );
        (renderer, commands, garbage, position)
    }

    fn render(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
        let mut buffer = vec![0.0f32; frames * 2];
        renderer.render(&mut buffer);
        buffer
    }

    #[test]
    fn pause_keeps_the_position_and_seek_jumps() {
        let (mut renderer, commands, garbage, position) = renderer();
        let track = Arc::new(Track::from_frames_for_test(48_000, 48_000));
        commands
            .send(Command::SetSource(Some(PreviewSource::Track(Arc::clone(
                &track,
            )))))
            .expect("send");
        commands.send(Command::SetPlaying(true)).expect("send");
        render(&mut renderer, 1000);
        assert_eq!(position.load(Ordering::Relaxed), 1000);

        commands.send(Command::SetPlaying(false)).expect("send");
        // The fade-out still advances a few frames, then playback holds still.
        render(&mut renderer, 2000);
        let paused_at = position.load(Ordering::Relaxed);
        let silent = render(&mut renderer, 500);
        assert_eq!(position.load(Ordering::Relaxed), paused_at, "paused");
        assert!(silent.iter().all(|&s| s == 0.0), "paused output is silent");

        commands.send(Command::SetPlaying(true)).expect("send");
        render(&mut renderer, 100);
        assert_eq!(
            position.load(Ordering::Relaxed),
            paused_at + 100,
            "resumed in place"
        );

        commands.send(Command::Seek(30_000)).expect("send");
        // Fades out (at most 720 frames at 15 ms), jumps, then plays on from
        // 30 000: so it ends up past 30 000 by less than the 2000 frames rendered.
        render(&mut renderer, 2000);
        let after_seek = position.load(Ordering::Relaxed);
        assert!(
            (30_000..32_000).contains(&after_seek),
            "jumped to {after_seek}"
        );

        // Replacing the source hands the old one back instead of freeing it here.
        commands
            .send(Command::SetSource(Some(PreviewSource::PinkNoise)))
            .expect("send");
        render(&mut renderer, 10);
        assert!(matches!(
            garbage.try_recv(),
            Ok(Garbage::Source(PreviewSource::Track(_)))
        ));
    }

    #[test]
    fn muting_silences_and_holds_the_position() {
        let (mut renderer, commands, _garbage, position) = renderer();
        let track = Arc::new(Track::from_frames_for_test(48_000, 48_000));
        commands
            .send(Command::SetSource(Some(PreviewSource::Track(track))))
            .and_then(|()| commands.send(Command::SetPlaying(true)))
            .expect("send");
        render(&mut renderer, 1000);
        commands.send(Command::SetMuted(true)).expect("send");
        render(&mut renderer, 1000);
        let held = position.load(Ordering::Relaxed);
        let silent = render(&mut renderer, 1000);
        assert_eq!(position.load(Ordering::Relaxed), held, "position held");
        assert!(silent.iter().all(|&s| s == 0.0), "muted output is silent");

        // Seeking while muted jumps straight away and stays silent.
        commands.send(Command::Seek(10_000)).expect("send");
        let still_silent = render(&mut renderer, 100);
        assert_eq!(position.load(Ordering::Relaxed), 10_000);
        assert!(still_silent.iter().all(|&s| s == 0.0));

        commands.send(Command::SetMuted(false)).expect("send");
        let resumed = render(&mut renderer, 2000);
        assert!(resumed.iter().any(|&s| s != 0.0), "audible again");
        assert_eq!(position.load(Ordering::Relaxed), 12_000);
    }

    #[test]
    fn preview_fades_in_instead_of_clicking() {
        let (mut renderer, commands, _garbage, _position) = renderer();
        let track = Arc::new(Track::from_frames_for_test(48_000, 48_000));
        commands
            .send(Command::Seek(0))
            .and_then(|()| commands.send(Command::SetSource(Some(PreviewSource::Track(track)))))
            .and_then(|()| commands.send(Command::SetPlaying(true)))
            .expect("send");
        // Frame 20_000 of the test ramp is loud; start there.
        commands.send(Command::Seek(20_000)).expect("send");
        let output = render(&mut renderer, 2000);
        let first = output[0].abs();
        let later = output[1990].abs();
        assert!(first < later * 0.1, "first sample {first}, later {later}");
    }

    fn render_tone(tone: Tone, sample_rate: f64) -> Vec<f32> {
        let mut voice = ToneVoice {
            tone,
            amplitude: f64::from(db_to_gain(tone.level_db)),
            phase: 0.0,
            elapsed: 0,
            released_at: None,
        };
        std::iter::from_fn(|| voice_sample(&mut voice, sample_rate)).collect()
    }

    #[test]
    fn pulsed_tone_has_expected_length_and_level() {
        let tone = Tone {
            ear: Some(Ear::Left),
            hz: 1000.0,
            level_db: -20.0,
            pattern: TonePattern::Pulsed { pips: 3 },
        };
        let samples = render_tone(tone, 48_000.0);
        let expected_secs = tone.pattern.duration_secs().expect("pulsed tones end");
        // The voice also renders the silent gap after the last pip.
        let expected = ((expected_secs + PIP_GAP_SECS) * 48_000.0) as usize;
        assert!(
            samples.len().abs_diff(expected) < 2,
            "{} samples",
            samples.len()
        );
        let peak = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!((peak - 0.1).abs() < 0.001, "peak {peak}");
        assert_eq!(samples[0], 0.0, "tones start silent to avoid a click");
    }
}
