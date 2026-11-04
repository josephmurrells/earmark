//! A just-for-fun hearing score from 0 to 100.
//!
//! The test can't measure absolute sensitivity: each listener sets their own
//! volume, and headphones differ. So the score compares each pitch with the
//! listener's own midrange instead, after removing the normal-hearing curve.
//! A perfectly even ear scores 100; pitches that needed louder tones than the
//! midrange (typically high treble, which fades with age and noise exposure)
//! and differences between the ears pull it down. That makes scores
//! comparable between people on different headphones and volumes, as long as
//! they took the same test length.

use crate::hearing_test::{HearingProfile, MAX_LEVEL_DB};
use crate::reference;
use crate::staircase::Threshold;

/// The midrange the other pitches are compared with, in Hz.
const MIDRANGE_HZ: (f32, f32) = (500.0, 2000.0);
/// Differences up to this are measurement noise and cost nothing, in dB.
const TOLERANCE_DB: f32 = 5.0;
/// How far beyond the loudest tone an unheard pitch counts as, in dB.
const BEYOND_MAX_PENALTY_DB: f32 = 10.0;
/// Points lost per dB of average shortfall.
const POINTS_PER_DB: f32 = 2.0;

/// A fun summary of a hearing profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HearingScore {
    /// From 0 to 100; higher is better.
    pub points: u32,
    /// The highest pitch heard in either ear, in Hz.
    pub highest_heard_hz: Option<f32>,
}

impl HearingScore {
    /// Scores a profile, or returns `None` if it has no midrange to compare with.
    #[must_use]
    pub fn new(profile: &HearingProfile) -> Option<Self> {
        // Each threshold relative to a normal ear, in dB; positive is worse.
        let relative: Vec<(f32, f32)> = profile
            .measurements
            .iter()
            .map(|m| {
                let level = match m.threshold {
                    Threshold::Level(level) => level,
                    Threshold::BeyondMax => MAX_LEVEL_DB + BEYOND_MAX_PENALTY_DB,
                };
                (m.hz, level - reference::threshold_db(m.hz))
            })
            .collect();
        let mut midrange: Vec<f32> = relative
            .iter()
            .filter(|(hz, _)| (MIDRANGE_HZ.0..=MIDRANGE_HZ.1).contains(hz))
            .map(|&(_, level)| level)
            .collect();
        if midrange.is_empty() {
            return None;
        }
        // Both ears share one baseline, so an imbalance between them counts.
        midrange.sort_by(f32::total_cmp);
        let middle = midrange.len() / 2;
        let baseline = if midrange.len().is_multiple_of(2) {
            (midrange[middle - 1] + midrange[middle]) / 2.0
        } else {
            midrange[middle]
        };
        let shortfall = relative
            .iter()
            .map(|&(_, level)| (level - baseline - TOLERANCE_DB).max(0.0))
            .sum::<f32>()
            / relative.len() as f32;
        let points = (100.0 - shortfall * POINTS_PER_DB)
            .round()
            .clamp(0.0, 100.0) as u32;
        let highest_heard_hz = profile
            .measurements
            .iter()
            .filter(|m| matches!(m.threshold, Threshold::Level(_)))
            .map(|m| m.hz)
            .max_by(f32::total_cmp);
        Some(Self {
            points,
            highest_heard_hz,
        })
    }

    /// Returns a one-word verdict for the score.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self.points {
            95.. => "Exceptional",
            85..=94 => "Excellent",
            70..=84 => "Good",
            50..=69 => "Fair",
            _ => "Uneven",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hearing_test::{Ear, Measurement, TestLength};

    /// A profile where each ear's threshold is the normal curve plus `loss(hz)`
    /// and an overall `offset` standing in for the volume setting.
    fn profile(offset: f32, loss: impl Fn(Ear, f32) -> Option<f32>) -> HearingProfile {
        let mut measurements = Vec::new();
        for ear in Ear::BOTH {
            for hz in TestLength::Standard.frequencies() {
                let threshold = match loss(ear, hz) {
                    Some(loss) => {
                        Threshold::Level(reference::threshold_db(hz) - 80.0 + offset + loss)
                    }
                    None => Threshold::BeyondMax,
                };
                measurements.push(Measurement { ear, hz, threshold });
            }
        }
        HearingProfile {
            measurements,
            ..Default::default()
        }
    }

    /// Typical age-related loss: none up to 2 kHz, then rising into the treble.
    fn treble_loss(hz: f32) -> f32 {
        match hz {
            ..=2000.0 => 0.0,
            ..=4000.0 => 15.0,
            ..=6000.0 => 25.0,
            ..=8000.0 => 30.0,
            _ => 45.0,
        }
    }

    fn score(profile: &HearingProfile) -> HearingScore {
        HearingScore::new(profile).expect("profile has a midrange")
    }

    #[test]
    fn even_hearing_scores_full_marks_at_any_volume() {
        for offset in [-15.0, 0.0, 20.0] {
            let even = score(&profile(offset, |_, _| Some(0.0)));
            assert_eq!(even.points, 100, "offset {offset}");
            assert_eq!(even.highest_heard_hz, Some(12000.0));
            assert_eq!(even.label(), "Exceptional");
        }
    }

    #[test]
    fn small_noise_costs_nothing() {
        let noisy = profile(0.0, |ear, hz| {
            Some(if (ear == Ear::Left) == (hz > 1000.0) {
                4.0
            } else {
                -3.0
            })
        });
        assert_eq!(score(&noisy).points, 100);
    }

    #[test]
    fn treble_loss_lowers_the_score() {
        let aged = score(&profile(0.0, |_, hz| Some(treble_loss(hz))));
        assert!(
            (65..=85).contains(&aged.points),
            "typical treble loss scored {}",
            aged.points
        );
        let deaf_above_8k = profile(0.0, |_, hz| (hz <= 8000.0).then(|| treble_loss(hz)));
        let worse = score(&deaf_above_8k);
        assert!(worse.points < aged.points, "{worse:?} vs {aged:?}");
        assert_eq!(worse.highest_heard_hz, Some(8000.0));
    }

    #[test]
    fn a_weaker_ear_lowers_the_score() {
        let one_sided = profile(0.0, |ear, _| {
            Some(if ear == Ear::Right { 15.0 } else { 0.0 })
        });
        let points = score(&one_sided).points;
        assert!(
            (70..100).contains(&points),
            "one weaker ear scored {points}"
        );
    }

    #[test]
    fn needs_a_midrange() {
        let mut bass_only = profile(0.0, |_, _| Some(0.0));
        bass_only.measurements.retain(|m| m.hz < 400.0);
        assert_eq!(HearingScore::new(&bass_only), None);
    }
}
