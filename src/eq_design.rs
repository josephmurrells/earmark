//! Turns measured hearing thresholds into an EQ curve and parametric filters.

use serde::{Deserialize, Serialize};

use crate::biquad::{Filter, FilterKind, FrequencyGrid};
use crate::freq_point::{self, FreqPoint};
use crate::hearing_test::{Ear, HearingProfile};
use crate::reference;
use crate::staircase::Threshold;

/// Sample rate the filters are fitted at; exports don't depend on it noticeably.
const FIT_SAMPLE_RATE: f32 = 48_000.0;
const FIT_ITERATIONS: usize = 80;
/// Fitting stops once every target is matched this closely, in dB.
const FIT_TOLERANCE_DB: f32 = 0.02;
const FIT_RELAXATION: f32 = 0.6;
const SHELF_Q: f32 = 0.707;
/// Limit on any one filter's gain while fitting, in dB.
const FIT_MAX_GAIN_DB: f32 = 30.0;

/// User-tunable parameters for turning thresholds into an EQ.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqSettings {
    /// Fraction of each threshold difference to compensate, 0 to 1.
    ///
    /// Thresholds describe the quietest audible sound; louder sounds are
    /// perceived more evenly, so compensating fully overcorrects.
    pub strength: f32,
    /// Largest boost allowed, in dB.
    pub max_boost_db: f32,
    /// Largest cut allowed, in dB.
    pub max_cut_db: f32,
    /// How much each band is pulled towards its neighbours, 0 to 1.
    pub smoothing: f32,
    /// Correct loudness differences between the ears as well as within each ear.
    pub balance_ears: bool,
}

impl EqSettings {
    /// Returns the settings with every value in range, replacing anything
    /// non-finite with its default. Use on settings read from disk.
    #[must_use]
    pub fn sanitized(self) -> Self {
        let defaults = Self::default();
        let clamp = |value: f32, low: f32, high: f32, default: f32| {
            if value.is_finite() {
                value.clamp(low, high)
            } else {
                default
            }
        };
        Self {
            strength: clamp(self.strength, 0.0, 1.0, defaults.strength),
            max_boost_db: clamp(self.max_boost_db, 0.0, 20.0, defaults.max_boost_db),
            max_cut_db: clamp(self.max_cut_db, 0.0, 20.0, defaults.max_cut_db),
            smoothing: clamp(self.smoothing, 0.0, 1.0, defaults.smoothing),
            balance_ears: self.balance_ears,
        }
    }
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            strength: 0.5,
            max_boost_db: 10.0,
            max_cut_db: 6.0,
            smoothing: 0.3,
            balance_ears: true,
        }
    }
}

/// Which curve a single-channel export uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Channels {
    /// The mean of both ears.
    #[default]
    Average,
    Left,
    Right,
}

impl Channels {
    /// Every option.
    pub const ALL: [Channels; 3] = [Channels::Average, Channels::Left, Channels::Right];

    /// Returns a human-readable name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Channels::Average => "Average of both ears",
            Channels::Left => "Left ear",
            Channels::Right => "Right ear",
        }
    }
}

/// The EQ for one ear.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EarEq {
    /// Desired gain at each tested frequency, ascending.
    pub target: Vec<FreqPoint>,
    /// Parametric filters approximating `target`.
    pub filters: Vec<Filter>,
}

impl EarEq {
    fn from_target(target: Vec<FreqPoint>) -> Self {
        let filters = fit_filters(&target, FIT_MAX_GAIN_DB);
        Self { target, filters }
    }

    /// Returns the gain of the parametric filters at `hz`, in dB.
    #[must_use]
    pub fn response_db(&self, hz: f32) -> f32 {
        response_db(&self.filters, hz)
    }

    /// Returns the gain of the filters at each of `freqs`, in dB.
    #[must_use]
    pub fn response_curve_db(&self, freqs: &[f32]) -> Vec<f32> {
        FrequencyGrid::new(freqs, FIT_SAMPLE_RATE).response_db(&self.filters)
    }

    /// Returns gains for an EQ whose bands are peaking filters at fixed
    /// frequencies `bands` (ascending), all with the same `q`, such that
    /// together they reproduce this EQ at every band centre.
    ///
    /// Unlike averaging the curve over each band, this accounts for how much
    /// neighbouring bands overlap. Gains are limited to `±limit_db`.
    #[must_use]
    pub fn fixed_band_gains(&self, bands: &[f32], q: f32, limit_db: f32) -> Vec<f32> {
        let desired = self.response_curve_db(bands);
        let target: Vec<FreqPoint> = bands
            .iter()
            .zip(desired)
            .map(|(&hz, db)| FreqPoint::new(hz, db))
            .collect();
        let mut filters: Vec<Filter> = target
            .iter()
            .map(|point| Filter {
                kind: FilterKind::Peak,
                hz: point.hz,
                gain_db: point.db.clamp(-limit_db, limit_db),
                q,
            })
            .collect();
        fit_gains(&mut filters, &target, limit_db);
        filters.iter().map(|filter| filter.gain_db).collect()
    }

    /// Returns the largest gain of the filters across the audible range, in dB.
    #[must_use]
    pub fn peak_gain_db(&self) -> f32 {
        self.response_curve_db(&freq_point::log_spaced(20.0, 20_000.0, 240))
            .into_iter()
            .fold(f32::NEG_INFINITY, f32::max)
    }
}

/// A complete EQ for both ears.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EqDesign {
    pub left: EarEq,
    pub right: EarEq,
    /// The mean of both ears, for single-curve outputs.
    average: EarEq,
    /// Gain applied before the filters so boosts can't clip, in dB (≤ 0).
    pub preamp_db: f32,
}

impl EqDesign {
    /// Designs an EQ from measured thresholds.
    #[must_use]
    pub fn new(profile: &HearingProfile, settings: &EqSettings) -> Self {
        let left = smoothed_deviations(profile, Ear::Left, settings.smoothing);
        let right = smoothed_deviations(profile, Ear::Right, settings.smoothing);
        let shared_offset = median(left.iter().chain(&right).map(|p| p.db));
        let to_target = |deviations: &[FreqPoint]| -> Vec<FreqPoint> {
            let offset = if settings.balance_ears {
                shared_offset
            } else {
                median(deviations.iter().map(|p| p.db))
            };
            deviations
                .iter()
                .map(|p| {
                    let gain = settings.strength * (p.db - offset);
                    FreqPoint::new(
                        p.hz,
                        gain.clamp(-settings.max_cut_db, settings.max_boost_db),
                    )
                })
                .collect()
        };
        let left = EarEq::from_target(to_target(&left));
        let right = EarEq::from_target(to_target(&right));
        let peak = left.peak_gain_db().max(right.peak_gain_db()).max(0.0);
        Self::from_ears(left, right, -(peak * 10.0).ceil() / 10.0)
    }

    /// Combines two ears' EQs, computing their average.
    #[must_use]
    pub fn from_ears(left: EarEq, right: EarEq, preamp_db: f32) -> Self {
        let average = average_of(&left, &right);
        Self {
            left,
            right,
            average,
            preamp_db,
        }
    }

    /// Returns the EQ for one ear.
    #[must_use]
    pub fn ear(&self, ear: Ear) -> &EarEq {
        match ear {
            Ear::Left => &self.left,
            Ear::Right => &self.right,
        }
    }

    /// Returns a single-channel EQ, for players that can't EQ each ear separately.
    #[must_use]
    pub fn channels(&self, channels: Channels) -> &EarEq {
        match channels {
            Channels::Left => &self.left,
            Channels::Right => &self.right,
            Channels::Average => &self.average,
        }
    }

    /// Describes the EQ in one plain sentence, such as "Boosts treble by about
    /// 6 dB and trims bass slightly."
    #[must_use]
    pub fn summary(&self, ears_together: bool) -> String {
        let average = self.channels(Channels::Average);
        let region = |low: f32, high: f32| {
            let curve = average.response_curve_db(&freq_point::log_spaced(low, high, 24));
            curve.iter().sum::<f32>() / curve.len() as f32
        };
        // Describe bass and treble relative to the mids, not to 0 dB: the
        // preamp makes absolute levels meaningless.
        let mids = region(250.0, 4000.0);
        let regions = [
            ("bass", region(40.0, 250.0)),
            ("treble", region(4000.0, 16_000.0)),
        ];
        let mut parts: Vec<String> = regions
            .iter()
            .filter_map(|&(name, level)| {
                let change = level - mids;
                let amount = change.abs().round();
                match (change > 0.0, amount) {
                    (_, a) if a < 1.5 => None,
                    (true, a) if a < 3.0 => Some(format!("lifts {name} slightly")),
                    (true, a) => Some(format!("boosts {name} by about {a:.0} dB")),
                    (false, a) if a < 3.0 => Some(format!("trims {name} slightly")),
                    (false, a) => Some(format!("cuts {name} by about {a:.0} dB")),
                }
            })
            .collect();
        if !ears_together {
            let balance = self.left.response_db(1000.0) - self.right.response_db(1000.0);
            if balance.abs() >= 1.5 {
                let louder = if balance > 0.0 { "left" } else { "right" };
                parts.push(format!(
                    "makes your {louder} ear about {:.0} dB louder",
                    balance.abs()
                ));
            }
        }
        match parts.as_slice() {
            [] => "Nearly flat: your hearing and headphones are well balanced.".to_owned(),
            [only] => format!("{}.", capitalise(only)),
            [rest @ .., last] => format!("{} and {last}.", capitalise(&rest.join(", "))),
        }
    }

    /// Returns true if neither ear has any measurements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.left.target.is_empty() && self.right.target.is_empty()
    }
}

/// Returns the EQ halfway between two ears, refitted as one set of filters.
fn average_of(left: &EarEq, right: &EarEq) -> EarEq {
    let mut freqs: Vec<f32> = left
        .target
        .iter()
        .chain(&right.target)
        .map(|p| p.hz)
        .collect();
    freqs.sort_by(f32::total_cmp);
    freqs.dedup();
    let target = freqs
        .into_iter()
        .map(|hz| {
            let sum = freq_point::interpolate(&left.target, hz)
                + freq_point::interpolate(&right.target, hz);
            FreqPoint::new(hz, sum / 2.0)
        })
        .collect();
    EarEq::from_target(target)
}

/// Returns, for each frequency heard in `ear`, how far the threshold sits above
/// a normal ear's, in dB, smoothed across neighbouring bands.
fn smoothed_deviations(profile: &HearingProfile, ear: Ear, smoothing: f32) -> Vec<FreqPoint> {
    let raw: Vec<FreqPoint> = profile
        .ear(ear)
        .filter_map(|m| match m.threshold {
            Threshold::Level(level) => {
                Some(FreqPoint::new(m.hz, level - reference::threshold_db(m.hz)))
            }
            Threshold::BeyondMax => None,
        })
        .collect();
    let smoothing = smoothing.clamp(0.0, 1.0);
    (0..raw.len())
        .map(|i| {
            let neighbours: Vec<f32> = [i.checked_sub(1), Some(i + 1)]
                .into_iter()
                .flatten()
                .filter_map(|j| raw.get(j).map(|p| p.db))
                .collect();
            if neighbours.is_empty() {
                return raw[i];
            }
            let neighbour_mean = neighbours.iter().sum::<f32>() / neighbours.len() as f32;
            FreqPoint::new(
                raw[i].hz,
                (1.0 - smoothing) * raw[i].db + smoothing * neighbour_mean,
            )
        })
        .collect()
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut values: Vec<f32> = values.collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn response_db(filters: &[Filter], hz: f32) -> f32 {
    filters
        .iter()
        .map(|filter| filter.response_db(hz, FIT_SAMPLE_RATE))
        .sum()
}

/// Places one filter per target point and adjusts their gains until the summed
/// response passes through every point.
///
/// The lowest and highest points get shelves, cornered halfway to their
/// neighbour, so the gain holds beyond the tested range as it does in `target`.
fn fit_filters(target: &[FreqPoint], max_gain_db: f32) -> Vec<Filter> {
    let n = target.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![Filter {
            kind: FilterKind::Peak,
            hz: target[0].hz,
            gain_db: target[0].db,
            q: 0.5,
        }];
    }
    let mut filters: Vec<Filter> = (0..n)
        .map(|i| {
            let (kind, hz, q) = if i == 0 {
                (
                    FilterKind::LowShelf,
                    (target[0].hz * target[1].hz).sqrt(),
                    SHELF_Q,
                )
            } else if i == n - 1 {
                let hz = (target[n - 2].hz * target[n - 1].hz).sqrt();
                (FilterKind::HighShelf, hz, SHELF_Q)
            } else {
                let octaves = (target[i + 1].hz / target[i - 1].hz).log2() / 2.0;
                (FilterKind::Peak, target[i].hz, q_for_bandwidth(octaves))
            };
            Filter {
                kind,
                hz,
                gain_db: target[i].db,
                q,
            }
        })
        .collect();
    fit_gains(&mut filters, target, max_gain_db);
    for filter in &mut filters {
        filter.gain_db = (filter.gain_db * 10.0).round() / 10.0;
        filter.q = (filter.q * 100.0).round() / 100.0;
        filter.hz = filter.hz.round();
    }
    filters
}

/// Adjusts the gains of `filters`, one per point of `target`, until their summed
/// response passes through every point. Frequencies and Qs stay as they are.
fn fit_gains(filters: &mut [Filter], target: &[FreqPoint], max_gain_db: f32) {
    let freqs: Vec<f32> = target.iter().map(|p| p.hz).collect();
    let grid = FrequencyGrid::new(&freqs, FIT_SAMPLE_RATE);
    for _ in 0..FIT_ITERATIONS {
        let errors: Vec<f32> = target
            .iter()
            .zip(grid.response_db(filters))
            .map(|(point, response)| point.db - response)
            .collect();
        if errors.iter().all(|e| e.abs() < FIT_TOLERANCE_DB) {
            break;
        }
        for (filter, error) in filters.iter_mut().zip(errors) {
            filter.gain_db =
                (filter.gain_db + FIT_RELAXATION * error).clamp(-max_gain_db, max_gain_db);
        }
    }
}

/// Converts a bandwidth in octaves to a peaking filter's Q.
fn q_for_bandwidth(octaves: f32) -> f32 {
    let ratio = 2f32.powf(octaves.max(0.1));
    ratio.sqrt() / (ratio - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hearing_test::{Measurement, TestLength};

    fn profile(threshold: impl Fn(Ear, f32) -> Threshold, length: TestLength) -> HearingProfile {
        let mut measurements = Vec::new();
        for ear in Ear::BOTH {
            for hz in length.frequencies() {
                measurements.push(Measurement {
                    ear,
                    hz,
                    threshold: threshold(ear, hz),
                });
            }
        }
        HearingProfile {
            measurements,
            ..Default::default()
        }
    }

    /// Thresholds a normal ear would give, plus `extra_db(ear, hz)`.
    fn normal_plus(extra_db: impl Fn(Ear, f32) -> f32) -> impl Fn(Ear, f32) -> Threshold {
        move |ear, hz| Threshold::Level(-75.0 + reference::threshold_db(hz) + extra_db(ear, hz))
    }

    #[test]
    fn sanitized_settings_are_in_range() {
        let settings = EqSettings {
            strength: f32::NAN,
            max_boost_db: 99.0,
            max_cut_db: -4.0,
            smoothing: f32::INFINITY,
            balance_ears: false,
        }
        .sanitized();
        assert_eq!(settings.strength, EqSettings::default().strength);
        assert_eq!(settings.max_boost_db, 20.0);
        assert_eq!(settings.max_cut_db, 0.0);
        assert_eq!(settings.smoothing, EqSettings::default().smoothing);
        assert!(!settings.balance_ears);
    }

    #[test]
    fn summary_describes_the_shape() {
        let flat = EqDesign::new(
            &profile(normal_plus(|_, _| 0.0), TestLength::Standard),
            &EqSettings::default(),
        );
        assert_eq!(
            flat.summary(false),
            "Nearly flat: your hearing and headphones are well balanced."
        );

        let loss = |_: Ear, hz: f32| if hz >= 4000.0 { 20.0 } else { 0.0 };
        let treble = EqDesign::new(
            &profile(normal_plus(loss), TestLength::Standard),
            &EqSettings::default(),
        );
        let summary = treble.summary(false);
        assert!(summary.starts_with("Boosts treble by about"), "{summary}");
        assert!(!summary.contains("bass"), "{summary}");

        let weaker_right = |ear: Ear, _: f32| if ear == Ear::Right { 10.0 } else { 0.0 };
        let balanced = EqDesign::new(
            &profile(normal_plus(weaker_right), TestLength::Quick),
            &EqSettings::default(),
        );
        assert_eq!(
            balanced.summary(false),
            "Makes your right ear about 5 dB louder."
        );
    }

    #[test]
    fn normal_hearing_gives_flat_eq() {
        let design = EqDesign::new(
            &profile(normal_plus(|_, _| 0.0), TestLength::Standard),
            &EqSettings::default(),
        );
        assert!(!design.is_empty());
        for ear in Ear::BOTH {
            for point in &design.ear(ear).target {
                assert!(point.db.abs() < 1e-3, "{ear:?} {point:?}");
            }
            assert!(design.ear(ear).response_db(1000.0).abs() < 0.1);
        }
        assert_eq!(design.preamp_db, 0.0);
    }

    #[test]
    fn treble_loss_gets_boosted_and_preamp_prevents_clipping() {
        let settings = EqSettings {
            smoothing: 0.0,
            ..Default::default()
        };
        let loss = |_: Ear, hz: f32| if hz >= 4000.0 { 20.0 } else { 0.0 };
        let design = EqDesign::new(&profile(normal_plus(loss), TestLength::Standard), &settings);
        let left = design.ear(Ear::Left);
        assert!((freq_point::interpolate(&left.target, 8000.0) - 10.0).abs() < 1e-3);
        assert!((left.response_db(8000.0) - 10.0).abs() < 0.5);
        assert!(left.response_db(500.0).abs() < 0.5);
        assert!(
            design.preamp_db <= -left.peak_gain_db(),
            "preamp {}",
            design.preamp_db
        );
        assert!(design.preamp_db <= -9.5, "preamp {}", design.preamp_db);
    }

    #[test]
    fn fitted_filters_follow_an_uneven_target() {
        let freqs = TestLength::Detailed.frequencies();
        let target: Vec<FreqPoint> = freqs
            .iter()
            .enumerate()
            .map(|(i, &hz)| FreqPoint::new(hz, [3.0, -2.0, 5.0, 0.0, -4.0][i % 5]))
            .collect();
        let filters = fit_filters(&target, 20.0);
        for point in &target {
            let error = point.db - response_db(&filters, point.hz);
            assert!(error.abs() < 0.6, "error {error} at {} Hz", point.hz);
        }
    }

    #[test]
    fn balance_boosts_the_weaker_ear() {
        let weaker_right = |ear: Ear, _: f32| if ear == Ear::Right { 10.0 } else { 0.0 };
        let profile = profile(normal_plus(weaker_right), TestLength::Quick);
        let balanced = EqDesign::new(&profile, &EqSettings::default());
        assert!(balanced.right.response_db(1000.0) > balanced.left.response_db(1000.0) + 4.0);

        let unbalanced = EqDesign::new(
            &profile,
            &EqSettings {
                balance_ears: false,
                ..Default::default()
            },
        );
        assert!(unbalanced.right.response_db(1000.0).abs() < 0.1);
    }

    #[test]
    fn unheard_bands_are_skipped_and_average_combines_ears() {
        let profile = profile(
            |ear, hz| {
                if ear == Ear::Right && hz > 10_000.0 {
                    Threshold::BeyondMax
                } else {
                    normal_plus(|_, _| 0.0)(ear, hz)
                }
            },
            TestLength::Standard,
        );
        let design = EqDesign::new(&profile, &EqSettings::default());
        assert_eq!(design.right.target.len(), design.left.target.len() - 1);
        assert_eq!(
            design.channels(Channels::Average).target.len(),
            design.left.target.len()
        );
    }
}
