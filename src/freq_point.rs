//! A gain or level value at a frequency, and interpolation between such points.

use serde::{Deserialize, Serialize};

/// A value in dB at a frequency in Hz.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FreqPoint {
    /// Frequency in Hz.
    pub hz: f32,
    /// Value in dB.
    pub db: f32,
}

impl FreqPoint {
    /// Creates a point.
    pub const fn new(hz: f32, db: f32) -> Self {
        Self { hz, db }
    }
}

/// Interpolates linearly on a log-frequency axis between `points`, which must be
/// sorted by frequency. Values are held flat beyond the first and last point.
///
/// Returns 0 dB when `points` is empty.
#[must_use]
pub fn interpolate(points: &[FreqPoint], hz: f32) -> f32 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return 0.0;
    };
    if hz <= first.hz {
        return first.db;
    }
    if hz >= last.hz {
        return last.db;
    }
    let upper = points.partition_point(|p| p.hz < hz);
    let (low, high) = (points[upper - 1], points[upper]);
    let t = (hz / low.hz).log2() / (high.hz / low.hz).log2();
    low.db + t * (high.db - low.db)
}

/// Returns `count` frequencies spaced evenly on a log axis from `low_hz` to `high_hz`.
#[must_use]
pub fn log_spaced(low_hz: f32, high_hz: f32, count: usize) -> Vec<f32> {
    let steps = count.saturating_sub(1).max(1) as f32;
    let ratio = (high_hz / low_hz).ln();
    (0..count)
        .map(|i| low_hz * (ratio * i as f32 / steps).exp())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_on_log_axis() {
        let points = [FreqPoint::new(100.0, 0.0), FreqPoint::new(400.0, 10.0)];
        assert!((interpolate(&points, 200.0) - 5.0).abs() < 1e-4);
        assert_eq!(interpolate(&points, 50.0), 0.0);
        assert_eq!(interpolate(&points, 1000.0), 10.0);
        assert_eq!(interpolate(&[], 1000.0), 0.0);
    }

    #[test]
    fn log_spaced_hits_both_ends() {
        let freqs = log_spaced(20.0, 20_000.0, 4);
        assert_eq!(freqs.len(), 4);
        assert!((freqs[0] - 20.0).abs() < 1e-3);
        assert!((freqs[1] - 200.0).abs() < 0.1);
        assert!((freqs[3] - 20_000.0).abs() < 1.0);
    }
}
