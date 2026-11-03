//! Parametric EQ filters (RBJ Audio EQ Cookbook) and their real-time state.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

/// The shape of a parametric EQ filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterKind {
    Peak,
    LowShelf,
    HighShelf,
}

impl FilterKind {
    /// Returns a human-readable name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            FilterKind::Peak => "Peak",
            FilterKind::LowShelf => "Low shelf",
            FilterKind::HighShelf => "High shelf",
        }
    }

    /// Returns the type code used by Equalizer APO and AutoEQ files.
    #[must_use]
    pub fn apo_code(self) -> &'static str {
        match self {
            FilterKind::Peak => "PK",
            FilterKind::LowShelf => "LSC",
            FilterKind::HighShelf => "HSC",
        }
    }
}

/// One parametric EQ band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub kind: FilterKind,
    /// Centre or corner frequency in Hz.
    pub hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl Filter {
    /// Returns this filter's gain at `hz`, in dB, when run at `sample_rate`.
    #[must_use]
    pub fn response_db(&self, hz: f32, sample_rate: f32) -> f32 {
        Coefficients::new(self, sample_rate).response_db(&UnitCircle::new(hz, sample_rate))
    }
}

/// A fixed set of frequencies at which filter responses are evaluated often.
///
/// The trigonometry for each frequency is computed once, and each filter's
/// coefficients once per call, instead of both for every filter at every point.
#[derive(Debug, Clone)]
pub struct FrequencyGrid {
    points: Vec<UnitCircle>,
    sample_rate: f32,
}

impl FrequencyGrid {
    /// Creates a grid of `freqs` for filters run at `sample_rate`.
    pub fn new(freqs: &[f32], sample_rate: f32) -> Self {
        Self {
            points: freqs
                .iter()
                .map(|&hz| UnitCircle::new(hz, sample_rate))
                .collect(),
            sample_rate,
        }
    }

    /// Returns the summed gain of `filters` at each grid frequency, in dB.
    #[must_use]
    pub fn response_db(&self, filters: &[Filter]) -> Vec<f32> {
        let coefficients: Vec<Coefficients> = filters
            .iter()
            .map(|filter| Coefficients::new(filter, self.sample_rate))
            .collect();
        self.points
            .iter()
            .map(|point| coefficients.iter().map(|c| c.response_db(point)).sum())
            .collect()
    }
}

/// The point e^{jw} on the unit circle for one frequency, as cos/sin of w and 2w.
#[derive(Debug, Clone, Copy)]
struct UnitCircle {
    cos1: f64,
    sin1: f64,
    cos2: f64,
    sin2: f64,
}

impl UnitCircle {
    fn new(hz: f32, sample_rate: f32) -> Self {
        let w = 2.0 * PI * f64::from(hz) / f64::from(sample_rate);
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        Self {
            cos1,
            sin1,
            cos2,
            sin2,
        }
    }
}

/// Normalised biquad coefficients (a0 = 1).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Coefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coefficients {
    fn new(filter: &Filter, sample_rate: f32) -> Self {
        let sample_rate = f64::from(sample_rate);
        let hz = f64::from(filter.hz).min(sample_rate * 0.49);
        let a = 10f64.powf(f64::from(filter.gain_db) / 40.0);
        let w0 = 2.0 * PI * hz / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * f64::from(filter.q).max(0.01));
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = match filter.kind {
            FilterKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha),
                (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha,
            ),
            FilterKind::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha),
                (a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha,
            ),
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn response_db(&self, point: &UnitCircle) -> f32 {
        // Evaluate H(z) at z = e^{jw}, with z^-1 = cos w - j sin w.
        let UnitCircle {
            cos1,
            sin1,
            cos2,
            sin2,
        } = *point;
        let num_re = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let num_im = -(self.b1 * sin1 + self.b2 * sin2);
        let den_re = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let den_im = -(self.a1 * sin1 + self.a2 * sin2);
        let power = (num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im);
        (10.0 * power.log10()) as f32
    }
}

/// A filter with its running state, for processing one channel of audio.
#[derive(Debug, Clone)]
pub struct Biquad {
    coefficients: Coefficients,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// Creates a filter for audio at `sample_rate`.
    pub fn new(filter: &Filter, sample_rate: f32) -> Self {
        Self {
            coefficients: Coefficients::new(filter, sample_rate),
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Filters one sample (transposed direct form II).
    pub fn process(&mut self, input: f32) -> f32 {
        let c = &self.coefficients;
        let x = f64::from(input);
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn filter(kind: FilterKind, gain_db: f32) -> Filter {
        Filter {
            kind,
            hz: 1000.0,
            gain_db,
            q: 0.707,
        }
    }

    #[test]
    fn peak_has_full_gain_at_centre_and_none_far_away() {
        let peak = filter(FilterKind::Peak, 6.0);
        assert!((peak.response_db(1000.0, RATE) - 6.0).abs() < 0.01);
        assert!(peak.response_db(20.0, RATE).abs() < 0.1);
        assert!(peak.response_db(20_000.0, RATE).abs() < 0.2);
    }

    #[test]
    fn shelves_have_half_gain_at_corner() {
        let low = filter(FilterKind::LowShelf, 8.0);
        assert!((low.response_db(20.0, RATE) - 8.0).abs() < 0.1);
        assert!((low.response_db(1000.0, RATE) - 4.0).abs() < 0.01);
        assert!(low.response_db(15_000.0, RATE).abs() < 0.1);

        let high = filter(FilterKind::HighShelf, -8.0);
        assert!((high.response_db(18_000.0, RATE) + 8.0).abs() < 0.2);
        assert!((high.response_db(1000.0, RATE) + 4.0).abs() < 0.01);
    }

    #[test]
    fn grid_matches_single_point_responses() {
        let filters = [
            filter(FilterKind::Peak, 6.0),
            filter(FilterKind::HighShelf, -3.0),
        ];
        let freqs = [50.0, 1000.0, 9000.0];
        let grid = FrequencyGrid::new(&freqs, RATE);
        for (hz, db) in freqs.iter().zip(grid.response_db(&filters)) {
            let expected: f32 = filters.iter().map(|f| f.response_db(*hz, RATE)).sum();
            assert!((db - expected).abs() < 1e-4, "{hz} Hz: {db} vs {expected}");
        }
    }

    #[test]
    fn processing_matches_response() {
        let peak = filter(FilterKind::Peak, 6.0);
        let mut biquad = Biquad::new(&peak, RATE);
        let mut peak_out: f32 = 0.0;
        for n in 0..48_000 {
            let input = (2.0 * std::f32::consts::PI * 1000.0 * n as f32 / RATE).sin();
            let output = biquad.process(input);
            if n > 24_000 {
                peak_out = peak_out.max(output.abs());
            }
        }
        let gain_db = 20.0 * peak_out.log10();
        assert!((gain_db - 6.0).abs() < 0.05, "measured {gain_db} dB");
    }
}
