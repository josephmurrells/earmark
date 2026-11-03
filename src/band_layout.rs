//! Translates the EQ into settings for fixed-band EQs (Spotify, headphone apps,
//! graphic equalisers) so the user can copy them by hand.

use serde::{Deserialize, Serialize};

use crate::eq_design::EarEq;
use crate::freq_point;

/// Points sampled across each band when averaging the EQ curve.
const SAMPLES_PER_BAND: usize = 24;
const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 20_000.0;

/// A family of fixed-band equalisers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BandLayout {
    /// Spotify's built-in equaliser.
    #[default]
    Spotify,
    /// Sony Headphones Connect / Sound Connect.
    Sony,
    /// The common ISO octave 10-band graphic EQ.
    TenBand,
    /// Frequencies typed in by the user.
    Custom,
}

impl BandLayout {
    /// Every option.
    pub const ALL: [BandLayout; 4] = [
        BandLayout::Spotify,
        BandLayout::Sony,
        BandLayout::TenBand,
        BandLayout::Custom,
    ];

    /// Returns a human-readable name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            BandLayout::Spotify => "Spotify (6 bands)",
            BandLayout::Sony => "Sony Headphones Connect (5 bands)",
            BandLayout::TenBand => "10-band graphic EQ (31 Hz to 16 kHz)",
            BandLayout::Custom => "Custom bands",
        }
    }

    /// Returns the band frequencies and slider limits, or `None` for custom
    /// bands, which come from the user.
    #[must_use]
    pub fn spec(self) -> Option<SliderSpec> {
        let (bands, limit_db, step_db) = match self {
            BandLayout::Spotify => (vec![60.0, 150.0, 400.0, 1000.0, 2400.0, 15000.0], 12.0, 0.5),
            BandLayout::Sony => (vec![400.0, 1000.0, 2500.0, 6300.0, 16000.0], 10.0, 1.0),
            BandLayout::TenBand => (
                vec![
                    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
                ],
                12.0,
                0.5,
            ),
            BandLayout::Custom => return None,
        };
        Some(SliderSpec {
            bands,
            limit_db,
            step_db,
        })
    }
}

/// The bands of a fixed-band EQ and the range of its sliders.
#[derive(Debug, Clone, PartialEq)]
pub struct SliderSpec {
    /// Band centre frequencies in Hz, ascending.
    pub bands: Vec<f32>,
    /// Sliders go from `-limit_db` to `+limit_db`.
    pub limit_db: f32,
    /// Slider resolution in dB.
    pub step_db: f32,
}

impl SliderSpec {
    /// Creates a spec for user-typed frequencies with typical slider limits.
    #[must_use]
    pub fn custom(bands: Vec<f32>) -> Self {
        Self {
            bands,
            limit_db: 12.0,
            step_db: 0.5,
        }
    }
}

/// What to set one slider to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandSetting {
    pub hz: f32,
    pub gain_db: f32,
    /// The ideal value was beyond the slider's range and has been limited.
    pub limited: bool,
}

/// Returns a setting for each band of `spec` that approximates `eq`.
///
/// Each band gets the average of the EQ curve over the part of the spectrum
/// it covers (halfway, on a log scale, to its neighbours). With `headroom`, all
/// bands are shifted down so none is above 0 dB, which avoids distortion in
/// apps that have no preamp.
#[must_use]
pub fn band_settings(eq: &EarEq, spec: &SliderSpec, headroom: bool) -> Vec<BandSetting> {
    let bands = &spec.bands;
    let averages: Vec<f32> = (0..bands.len())
        .map(|i| {
            let (low, high) = band_edges(bands, i);
            let samples = freq_point::log_spaced(low, high, SAMPLES_PER_BAND);
            let curve = eq.response_curve_db(&samples);
            curve.iter().sum::<f32>() / curve.len() as f32
        })
        .collect();
    let shift = if headroom {
        averages.iter().copied().fold(0.0, f32::max)
    } else {
        0.0
    };
    bands
        .iter()
        .zip(averages)
        .map(|(&hz, average)| {
            let ideal = average - shift;
            let stepped = (ideal / spec.step_db).round() * spec.step_db;
            let gain_db = stepped.clamp(-spec.limit_db, spec.limit_db);
            BandSetting {
                hz,
                // Avoid showing "-0.0".
                gain_db: if gain_db == 0.0 { 0.0 } else { gain_db },
                limited: ideal.abs() > spec.limit_db + spec.step_db / 2.0,
            }
        })
        .collect()
}

/// Returns the frequency range band `i` covers: halfway (on a log scale) to
/// each neighbour, and out to the edge of hearing for the outermost bands.
fn band_edges(bands: &[f32], i: usize) -> (f32, f32) {
    let low = match i.checked_sub(1) {
        Some(previous) => (bands[previous] * bands[i]).sqrt(),
        None => LOW_HZ.min(bands[i]),
    };
    let high = match bands.get(i + 1) {
        Some(next) => (bands[i] * next).sqrt(),
        None => HIGH_HZ.max(bands[i]),
    };
    (low, high)
}

/// Parses a list of frequencies such as "60, 150, 400, 1k, 2.4k, 15k".
///
/// Returns them sorted and deduplicated, or an error naming the first entry
/// that isn't a frequency between 10 Hz and 24 kHz.
pub fn parse_frequencies(text: &str) -> Result<Vec<f32>, String> {
    let mut bands = Vec::new();
    for entry in text.split([',', ';', ' ']).filter(|e| !e.trim().is_empty()) {
        let lower = entry.trim().to_ascii_lowercase();
        let lower = lower.trim_end_matches("hz").trim();
        let (number, scale) = match lower.strip_suffix('k') {
            Some(number) => (number, 1000.0),
            None => (lower, 1.0),
        };
        let hz = number
            .trim()
            .parse::<f32>()
            .ok()
            .map(|n| n * scale)
            .filter(|hz| (10.0..=24_000.0).contains(hz))
            .ok_or_else(|| {
                format!(
                    "\"{}\" isn't a frequency between 10 Hz and 24 kHz",
                    entry.trim()
                )
            })?;
        bands.push(hz);
    }
    if bands.is_empty() {
        return Err("Enter your EQ's band frequencies, such as 60, 250, 1k, 4k, 12k.".to_owned());
    }
    bands.sort_by(f32::total_cmp);
    bands.dedup();
    Ok(bands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biquad::{Filter, FilterKind};

    fn treble_boost() -> EarEq {
        EarEq {
            target: Vec::new(),
            filters: vec![Filter {
                kind: FilterKind::HighShelf,
                hz: 4000.0,
                gain_db: 6.0,
                q: 0.707,
            }],
        }
    }

    #[test]
    fn treble_boost_raises_treble_bands() {
        let spec = BandLayout::Spotify.spec().expect("fixed layout");
        let settings = band_settings(&treble_boost(), &spec, false);
        assert_eq!(settings.len(), 6);
        assert_eq!(settings[0].gain_db, 0.0, "{settings:?}");
        assert!(settings[5].gain_db > 5.0, "{settings:?}");
        assert!(
            settings
                .windows(2)
                .all(|pair| pair[0].gain_db <= pair[1].gain_db)
        );
        assert!(settings.iter().all(|s| !s.limited));
    }

    #[test]
    fn headroom_keeps_every_band_at_or_below_zero() {
        let spec = BandLayout::TenBand.spec().expect("fixed layout");
        let settings = band_settings(&treble_boost(), &spec, true);
        assert!(settings.iter().all(|s| s.gain_db <= 0.0), "{settings:?}");
        assert!(settings[0].gain_db < -5.0, "{settings:?}");
        assert_eq!(settings[9].gain_db, 0.0);
    }

    #[test]
    fn values_snap_to_steps_and_are_limited() {
        let huge = EarEq {
            target: Vec::new(),
            filters: vec![Filter {
                kind: FilterKind::Peak,
                hz: 1000.0,
                gain_db: 25.0,
                q: 0.5,
            }],
        };
        let spec = BandLayout::Sony.spec().expect("fixed layout");
        let settings = band_settings(&huge, &spec, false);
        let one_k = settings[1];
        assert_eq!(one_k.gain_db, 10.0);
        assert!(one_k.limited);
        assert!(
            settings.iter().all(|s| s.gain_db.fract() == 0.0),
            "{settings:?}"
        );
    }

    #[test]
    fn parses_frequency_lists() {
        assert_eq!(
            parse_frequencies("60, 150Hz 400; 1k 2.4kHz, 15K").expect("valid list"),
            vec![60.0, 150.0, 400.0, 1000.0, 2400.0, 15000.0]
        );
        assert!(parse_frequencies("60, loud").is_err());
        assert!(parse_frequencies("   ").is_err());
        assert!(parse_frequencies("5").is_err());
    }
}
