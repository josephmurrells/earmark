//! Music files decoded for previewing the EQ.
//!
//! A file is decoded with symphonia, mixed to stereo, resampled once to the
//! output device's rate with rubato, and kept in memory as 16-bit samples.

use std::fmt;
use std::fs::File;
use std::io;
use std::path::Path;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// File extensions offered in the file picker and accepted when dropped.
pub const EXTENSIONS: [&str; 8] = ["mp3", "flac", "m4a", "mp4", "aac", "ogg", "oga", "wav"];
/// Longest track accepted, in seconds; keeps memory bounded (~230 MB at 48 kHz).
const MAX_SECONDS: u64 = 20 * 60;
/// Sample rates accepted, in Hz.
const SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=384_000;
/// Frames per resampler chunk.
const RESAMPLE_CHUNK: usize = 1024;

/// Why a file couldn't be loaded.
#[derive(Debug)]
pub enum TrackError {
    Open(io::Error),
    /// The format or codec isn't supported, or the file isn't audio.
    Unsupported(String),
    /// The file is damaged or decoding failed.
    Decode(String),
    TooLong,
    Empty,
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrackError::Open(e) => write!(f, "couldn't open it ({e})"),
            TrackError::Unsupported(what) => write!(f, "it isn't a supported audio file ({what})"),
            TrackError::Decode(what) => write!(f, "it couldn't be decoded ({what})"),
            TrackError::TooLong => write!(f, "it's longer than {} minutes", MAX_SECONDS / 60),
            TrackError::Empty => write!(f, "it contains no audio"),
        }
    }
}

impl std::error::Error for TrackError {}

/// Returns true if `path` has an extension Earmark can try to play.
#[must_use]
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Returns a file's display name: its name without the extension.
#[must_use]
pub fn display_name(path: &Path) -> String {
    path.file_stem().or_else(|| path.file_name()).map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A stereo recording at the output device's sample rate.
#[derive(Debug)]
pub struct Track {
    /// Interleaved left/right samples.
    samples: Vec<i16>,
    sample_rate: u32,
}

impl Track {
    /// Decodes an audio file and converts it to stereo at `sample_rate`.
    ///
    /// This can take a second or two for a long file, so call it off the UI thread.
    pub fn load(path: &Path, sample_rate: u32) -> Result<Self, TrackError> {
        if !SAMPLE_RATES.contains(&sample_rate) {
            return Err(TrackError::Unsupported(format!("{sample_rate} Hz output")));
        }
        let (stereo, file_rate) = decode_stereo(path)?;
        let resampled = resample(&stereo, file_rate, sample_rate)?;
        Ok(Self {
            samples: resampled.iter().map(|&s| to_i16(s)).collect(),
            sample_rate,
        })
    }

    /// Creates a track from a ramp of samples, for tests elsewhere.
    #[cfg(test)]
    pub fn from_frames_for_test(frames: usize, sample_rate: u32) -> Self {
        let samples = (0..frames)
            .flat_map(|i| {
                let value = (i % 30_000) as i16;
                [value, -value]
            })
            .collect();
        Self {
            samples,
            sample_rate,
        }
    }

    /// Returns the number of stereo frames.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    /// Returns the sample rate the track was converted to, in Hz.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Returns the length in seconds.
    #[must_use]
    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / f64::from(self.sample_rate)
    }

    /// Returns the left and right samples of `frame`.
    #[must_use]
    pub fn frame(&self, frame: usize) -> (f32, f32) {
        let scale = 1.0 / 32_768.0;
        (
            f32::from(self.samples[frame * 2]) * scale,
            f32::from(self.samples[frame * 2 + 1]) * scale,
        )
    }
}

/// Decodes the file's default audio track to interleaved stereo. Returns the
/// samples and their sample rate.
fn decode_stereo(path: &Path) -> Result<(Vec<f32>, u32), TrackError> {
    let file = File::open(path).map_err(TrackError::Open)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| TrackError::Unsupported(e.to_string()))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| TrackError::Unsupported("no audio track".to_owned()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| TrackError::Unsupported("unknown codec".to_owned()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| TrackError::Unsupported(e.to_string()))?;

    let mut stereo = Vec::new();
    let mut rate = None;
    let mut scratch: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // A truncated file still plays up to the damage.
            Err(SymphoniaError::IoError(_)) if !stereo.is_empty() => break,
            Err(e) => return Err(TrackError::Decode(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            // Skip a damaged packet rather than give up on the whole file.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(TrackError::Decode(e.to_string())),
        };
        let spec = buffer.spec();
        let buffer_rate = spec.rate();
        let channels = spec.channels().count();
        if channels == 0 || !SAMPLE_RATES.contains(&buffer_rate) {
            return Err(TrackError::Unsupported(format!("{buffer_rate} Hz")));
        }
        if *rate.get_or_insert(buffer_rate) != buffer_rate {
            return Err(TrackError::Unsupported(
                "sample rate changes mid-file".to_owned(),
            ));
        }
        scratch.resize(buffer.samples_interleaved(), 0.0);
        buffer.copy_to_slice_interleaved(&mut scratch);
        for frame in scratch.chunks_exact(channels) {
            // Mono plays in both ears; anything wider keeps front left and right.
            let right = if channels > 1 { frame[1] } else { frame[0] };
            stereo.push(frame[0]);
            stereo.push(right);
        }
        if (stereo.len() / 2) as u64 > MAX_SECONDS * u64::from(buffer_rate) {
            return Err(TrackError::TooLong);
        }
    }
    match rate {
        Some(rate) if !stereo.is_empty() => Ok((stereo, rate)),
        _ => Err(TrackError::Empty),
    }
}

/// Resamples interleaved stereo with a band-limited FFT resampler.
fn resample(stereo: &[f32], from_rate: u32, to_rate: u32) -> Result<Vec<f32>, TrackError> {
    if from_rate == to_rate {
        return Ok(stereo.to_vec());
    }
    let frames = stereo.len() / 2;
    let input = InterleavedSlice::new(stereo, 2, frames).map_err(resample_error)?;
    let mut resampler = Fft::<f32>::new(
        from_rate as usize,
        to_rate as usize,
        RESAMPLE_CHUNK,
        2,
        FixedSync::Both,
    )
    .map_err(resample_error)?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(resample_error)?;
    Ok(output.take_data())
}

fn resample_error(error: impl fmt::Display) -> TrackError {
    TrackError::Decode(format!("resampling: {error}"))
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str, extension: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "earmark-test-{name}-{}.{extension}",
            std::process::id()
        ))
    }

    fn write_wav(name: &str, sample_rate: u32, channels: u16, frames: u32) -> std::path::PathBuf {
        let path = temp_path(name, "wav");
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("create wav");
        for n in 0..frames {
            let t = n as f32 / sample_rate as f32;
            let sample = ((2.0 * std::f32::consts::PI * 1000.0 * t).sin() * 16_384.0) as i16;
            for _ in 0..channels {
                writer.write_sample(sample).expect("write sample");
            }
        }
        writer.finalize().expect("finalize");
        path
    }

    #[test]
    fn loads_mono_wav_as_stereo_at_the_same_rate() {
        let path = write_wav("mono", 48_000, 1, 4_800);
        let track = Track::load(&path, 48_000).expect("load");
        assert_eq!(track.frames(), 4_800);
        let (left, right) = track.frame(12);
        assert_eq!(left, right, "mono plays in both ears");
        assert!((track.duration_secs() - 0.1).abs() < 1e-6);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn resamples_to_the_device_rate_without_losing_level() {
        let path = write_wav("resample", 44_100, 2, 44_100);
        let track = Track::load(&path, 48_000).expect("load");
        assert!(
            track.frames().abs_diff(48_000) < 50,
            "{} frames",
            track.frames()
        );
        // The 1 kHz tone keeps its amplitude (0.5) through resampling.
        let peak = (1000..47_000)
            .map(|i| track.frame(i).0.abs())
            .fold(0.0f32, f32::max);
        assert!((peak - 0.5).abs() < 0.01, "peak {peak}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_files_that_are_not_audio_or_have_a_bad_header() {
        let junk = temp_path("junk", "mp3");
        std::fs::write(&junk, b"definitely not audio").expect("write junk");
        assert!(Track::load(&junk, 48_000).is_err());
        let _ = std::fs::remove_file(junk);

        // A 16-bit mono header claiming a 0 Hz sample rate, with two samples.
        let zero_rate = temp_path("rate0", "wav");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // channels
        bytes.extend_from_slice(&0u32.to_le_bytes()); // sample rate
        bytes.extend_from_slice(&0u32.to_le_bytes()); // byte rate
        bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
        bytes.extend_from_slice(&16u16.to_le_bytes()); // bits
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        std::fs::write(&zero_rate, bytes).expect("write wav");
        assert!(
            Track::load(&zero_rate, 48_000).is_err(),
            "0 Hz must be rejected"
        );
        let _ = std::fs::remove_file(zero_rate);

        let missing = Path::new("C:/definitely/missing/song.flac");
        assert!(matches!(
            Track::load(missing, 48_000),
            Err(TrackError::Open(_))
        ));
    }

    #[test]
    fn recognises_supported_extensions() {
        assert!(is_supported(Path::new("a/b/Song.FLAC")));
        assert!(is_supported(Path::new("x.m4a")));
        assert!(!is_supported(Path::new("notes.txt")));
        assert!(!is_supported(Path::new("noextension")));
        assert_eq!(display_name(Path::new("C:/Music/My Song.mp3")), "My Song");
    }
}
