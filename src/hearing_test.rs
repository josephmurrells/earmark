//! Runs a staircase for every frequency and ear, with catch trials.
//!
//! Bands are measured a few at a time in random order, switching randomly
//! between them, so tones are unpredictable but bands finish steadily. Each
//! band starts just above the level predicted from the bands already measured.

use serde::{Deserialize, Serialize};

use crate::freq_point::{self, FreqPoint};
use crate::reference;
use crate::staircase::{Response, Staircase, Threshold};

/// The quietest tone the test plays, in dBFS.
pub const MIN_LEVEL_DB: f32 = -110.0;
/// The loudest tone the test plays, in dBFS.
pub const MAX_LEVEL_DB: f32 = -10.0;
/// The level of the calibration tone, in dBFS.
pub const CALIBRATION_LEVEL_DB: f32 = -10.0;
/// After calibration, a normal ear's 1 kHz threshold is expected near this level.
const EXPECTED_1K_THRESHOLD_DB: f32 = -75.0;
/// How far above the expected threshold a band starts when nothing has been
/// measured yet, in dB.
const START_ABOVE_EXPECTED_DB: f32 = 20.0;
/// Start margin when the other ear has been measured at the same frequency.
const START_ABOVE_OTHER_EAR_DB: f32 = 5.0;
/// Start margin when predicting from neighbouring frequencies.
const START_ABOVE_NEIGHBOURS_DB: f32 = 5.0;
/// How many bands are measured at once.
const ACTIVE_BANDS: usize = 3;
/// One in this many trials (on average) is silent, to catch guessing.
const CATCH_TRIAL_ODDS: u32 = 8;
/// Typical time per trial (pause, tone, and answer), for time estimates.
pub const SECS_PER_TRIAL: f32 = 2.4;
/// Typical trials per band including catch trials, from `print_test_lengths`.
const TYPICAL_TRIALS_PER_BAND: f32 = 8.0;
/// No catch trials until this many tones have been played.
const TRIALS_BEFORE_CATCH: u32 = 4;

/// A side of the head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ear {
    Left,
    Right,
}

impl Ear {
    /// Both ears, left first.
    pub const BOTH: [Ear; 2] = [Ear::Left, Ear::Right];

    /// Returns a human-readable name.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Ear::Left => "Left",
            Ear::Right => "Right",
        }
    }
}

/// Whether each ear is tested on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EarMode {
    /// One ear at a time, giving a separate EQ for each.
    #[default]
    Separate,
    /// Both ears at once: half the time, one EQ for both.
    Together,
}

impl EarMode {
    /// Every option.
    #[cfg(test)]
    pub const ALL: [EarMode; 2] = [EarMode::Separate, EarMode::Together];

    /// Returns the ears tested separately, or `[None]` for both together.
    fn targets(self) -> Vec<Option<Ear>> {
        match self {
            EarMode::Separate => vec![Some(Ear::Left), Some(Ear::Right)],
            EarMode::Together => vec![None],
        }
    }
}

/// How many frequencies to test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TestLength {
    /// Seven bands.
    Quick,
    /// Ten bands.
    #[default]
    Standard,
    /// Seventeen half-octave bands.
    Detailed,
}

impl TestLength {
    /// Every option, shortest first.
    pub const ALL: [TestLength; 3] = [
        TestLength::Quick,
        TestLength::Standard,
        TestLength::Detailed,
    ];

    /// Returns the option's short name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            TestLength::Quick => "Quick",
            TestLength::Standard => "Standard",
            TestLength::Detailed => "Detailed",
        }
    }

    /// Returns the typical duration in whole minutes (at least 1).
    #[must_use]
    pub fn minutes(self, ears: EarMode) -> u32 {
        let total_bands = self.frequencies().len() * ears.targets().len();
        let minutes = total_bands as f32 * TYPICAL_TRIALS_PER_BAND * SECS_PER_TRIAL / 60.0;
        minutes.round().max(1.0) as u32
    }

    /// Returns the test frequencies in Hz, ascending.
    #[must_use]
    pub fn frequencies(self) -> Vec<f32> {
        match self {
            TestLength::Quick => vec![125.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 12000.0],
            TestLength::Standard => vec![
                63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 6000.0, 8000.0, 12000.0,
            ],
            TestLength::Detailed => freq_point::log_spaced(62.5, 16000.0, 17)
                .into_iter()
                .map(|hz| hz.round())
                .collect(),
        }
    }
}

/// The measured threshold at one frequency in one ear.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub ear: Ear,
    pub hz: f32,
    pub threshold: Threshold,
}

/// The results of a finished test.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HearingProfile {
    /// Both ears were tested together, so each ear holds the same thresholds.
    pub ears_together: bool,
    /// Thresholds sorted by ear, then frequency.
    pub measurements: Vec<Measurement>,
    /// Silent trials played.
    pub catch_trials: u32,
    /// Silent trials the listener said they heard.
    pub false_alarms: u32,
}

/// Most measurements a profile may hold (17 bands × 2 ears is the most a test
/// records); more means the file is corrupt or was made by hand.
const MAX_MEASUREMENTS: usize = 128;

impl HearingProfile {
    /// Returns the profile with implausible measurements removed (non-finite or
    /// out-of-range frequencies or levels, duplicates), sorted, or `None` if
    /// nothing usable is left. Use on anything read from disk.
    #[must_use]
    pub fn sanitized(mut self) -> Option<Self> {
        self.measurements.retain(|m| {
            let hz_ok = m.hz.is_finite() && (20.0..=20_000.0).contains(&m.hz);
            let level_ok = match m.threshold {
                Threshold::Level(level) => level.is_finite() && (-160.0..=20.0).contains(&level),
                Threshold::BeyondMax => true,
            };
            hz_ok && level_ok
        });
        self.measurements.sort_by(|a, b| {
            (a.ear as u8)
                .cmp(&(b.ear as u8))
                .then(a.hz.total_cmp(&b.hz))
        });
        self.measurements
            .dedup_by(|a, b| a.ear == b.ear && a.hz == b.hz);
        self.measurements.truncate(MAX_MEASUREMENTS);
        self.false_alarms = self.false_alarms.min(self.catch_trials);
        (!self.measurements.is_empty()).then_some(self)
    }

    /// Returns the measurements for one ear, sorted by frequency.
    pub fn ear(&self, ear: Ear) -> impl Iterator<Item = &Measurement> {
        self.measurements.iter().filter(move |m| m.ear == ear)
    }

    /// Returns the weaker ear and how much less sensitive it is, in dB (the
    /// median difference across pitches), or `None` if both ears were tested
    /// together or there's too little data.
    #[must_use]
    pub fn ear_difference(&self) -> Option<(Ear, f32)> {
        if self.ears_together {
            return None;
        }
        let levels = |ear| -> Vec<(f32, f32)> {
            self.ear(ear)
                .filter_map(|m| match m.threshold {
                    Threshold::Level(level) => Some((m.hz, level)),
                    Threshold::BeyondMax => None,
                })
                .collect()
        };
        let (left, right) = (levels(Ear::Left), levels(Ear::Right));
        let mut differences: Vec<f32> = left
            .iter()
            .filter_map(|&(hz, l)| right.iter().find(|r| r.0 == hz).map(|&(_, r)| r - l))
            .collect();
        if differences.len() < 3 {
            return None;
        }
        differences.sort_by(f32::total_cmp);
        // Positive: the right ear needed louder tones.
        let median = differences[differences.len() / 2];
        let weaker = if median > 0.0 { Ear::Right } else { Ear::Left };
        Some((weaker, median.abs()))
    }

    /// Returns true if the listener often reported hearing silence.
    #[must_use]
    pub fn is_unreliable(&self) -> bool {
        self.catch_trials > 0 && self.false_alarms * 4 > self.catch_trials
    }
}

/// What to play for one trial.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stimulus {
    /// A pulsed tone in one ear, or in both when `ear` is `None`.
    Tone {
        ear: Option<Ear>,
        hz: f32,
        level_db: f32,
    },
    /// Nothing at all; the listener should report not hearing anything.
    Silence,
}

/// One presentation waiting for a response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trial {
    band: Option<usize>,
    /// What to play.
    pub stimulus: Stimulus,
}

#[derive(Debug, Clone)]
struct Band {
    /// The ear tested, or `None` for both together.
    ear: Option<Ear>,
    hz: f32,
    staircase: Staircase,
    /// Whether the start level has been chosen; bands start when first played.
    started: bool,
}

/// The answer-dependent state of a test, saved before each answer so it can be
/// taken back.
#[derive(Debug, Clone)]
struct Snapshot {
    bands: Vec<Band>,
    /// The trial that was being answered.
    trial: Trial,
    last_band: Option<usize>,
    tones_played: u32,
    catch_trials: u32,
    false_alarms: u32,
}

/// A hearing test in progress.
#[derive(Debug)]
pub struct HearingTest {
    bands: Vec<Band>,
    current: Option<Trial>,
    last_band: Option<usize>,
    tones_played: u32,
    catch_trials: u32,
    false_alarms: u32,
    /// The state before each answer, oldest first.
    history: Vec<Snapshot>,
    ears: EarMode,
    predict_starts: bool,
    rng: fastrand::Rng,
}

impl HearingTest {
    /// Creates a test at the frequencies of `length`.
    pub fn new(length: TestLength, ears: EarMode) -> Self {
        Self::with_rng(length, ears, fastrand::Rng::new())
    }

    fn with_rng(length: TestLength, ears: EarMode, mut rng: fastrand::Rng) -> Self {
        let mut bands = Vec::new();
        for ear in ears.targets() {
            for hz in length.frequencies() {
                bands.push(Band {
                    ear,
                    hz,
                    staircase: Staircase::new(default_start_db(hz), MIN_LEVEL_DB, MAX_LEVEL_DB),
                    started: false,
                });
            }
        }
        rng.shuffle(&mut bands);
        Self {
            ears,
            predict_starts: true,
            bands,
            current: None,
            last_band: None,
            tones_played: 0,
            catch_trials: 0,
            false_alarms: 0,
            history: Vec::new(),
            rng,
        }
    }

    /// Returns the trial awaiting a response, choosing a new one if needed.
    ///
    /// Returns `None` once every band has a threshold.
    pub fn trial(&mut self) -> Option<Trial> {
        if self.current.is_none() {
            self.current = self.choose_trial();
        }
        self.current
    }

    /// Records the response to the current trial.
    pub fn respond(&mut self, response: Response) {
        let Some(trial) = self.current.take() else {
            return;
        };
        self.history.push(Snapshot {
            bands: self.bands.clone(),
            trial,
            last_band: self.last_band,
            tones_played: self.tones_played,
            catch_trials: self.catch_trials,
            false_alarms: self.false_alarms,
        });
        match trial.band {
            Some(index) => {
                self.bands[index].staircase.record(response);
                self.last_band = Some(index);
                self.tones_played += 1;
            }
            None => {
                self.catch_trials += 1;
                if response == Response::Heard {
                    self.false_alarms += 1;
                }
            }
        }
    }

    /// Returns true if there's an answer to take back.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Takes back the last answer, making its trial current again so it can be
    /// answered afresh. Returns false if nothing has been answered yet.
    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.history.pop() else {
            return false;
        };
        self.bands = snapshot.bands;
        self.current = Some(snapshot.trial);
        self.last_band = snapshot.last_band;
        self.tones_played = snapshot.tones_played;
        self.catch_trials = snapshot.catch_trials;
        self.false_alarms = snapshot.false_alarms;
        true
    }

    /// Returns `(finished bands, total bands)`.
    #[must_use]
    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .bands
            .iter()
            .filter(|band| band.staircase.result().is_some())
            .count();
        (done, self.bands.len())
    }

    /// Returns roughly how far through the test this is, from 0 to 1.
    #[must_use]
    pub fn fraction_done(&self) -> f32 {
        let total: f32 = self
            .bands
            .iter()
            .map(|band| band.staircase.progress())
            .sum();
        total / self.bands.len().max(1) as f32
    }

    /// Returns a rough number of trials still to come, including catch trials.
    #[must_use]
    pub fn trials_left_estimate(&self) -> f32 {
        let tones: u32 = self
            .bands
            .iter()
            .map(|band| band.staircase.trials_left_estimate())
            .sum();
        tones as f32 * (1.0 + 1.0 / CATCH_TRIAL_ODDS as f32)
    }

    /// Returns true when every band has a threshold.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        let (done, total) = self.progress();
        done == total
    }

    /// Returns the thresholds found so far.
    #[must_use]
    pub fn profile(&self) -> HearingProfile {
        let mut measurements = Vec::new();
        for band in &self.bands {
            let Some(threshold) = band.staircase.result() else {
                continue;
            };
            let ears = match band.ear {
                Some(ear) => vec![ear],
                None => Ear::BOTH.to_vec(),
            };
            for ear in ears {
                measurements.push(Measurement {
                    ear,
                    hz: band.hz,
                    threshold,
                });
            }
        }
        measurements.sort_by(|a, b| {
            (a.ear as u8)
                .cmp(&(b.ear as u8))
                .then(a.hz.total_cmp(&b.hz))
        });
        HearingProfile {
            ears_together: self.ears == EarMode::Together,
            measurements,
            catch_trials: self.catch_trials,
            false_alarms: self.false_alarms,
        }
    }

    fn choose_trial(&mut self) -> Option<Trial> {
        let open: Vec<usize> = (0..self.bands.len())
            .filter(|&i| self.bands[i].staircase.result().is_none())
            .take(ACTIVE_BANDS)
            .collect();
        if open.is_empty() {
            return None;
        }
        for &index in &open {
            if !self.bands[index].started {
                let start = if self.predict_starts {
                    self.predicted_start_db(index)
                } else {
                    default_start_db(self.bands[index].hz)
                };
                let band = &mut self.bands[index];
                band.staircase = Staircase::new(start, MIN_LEVEL_DB, MAX_LEVEL_DB);
                band.started = true;
            }
        }
        if self.tones_played >= TRIALS_BEFORE_CATCH && self.rng.u32(0..CATCH_TRIAL_ODDS) == 0 {
            return Some(Trial {
                band: None,
                stimulus: Stimulus::Silence,
            });
        }
        // Avoid repeating the same band twice in a row so the listener can't
        // predict the next tone.
        let candidates: Vec<usize> = if open.len() > 1 {
            open.iter()
                .copied()
                .filter(|&i| Some(i) != self.last_band)
                .collect()
        } else {
            open
        };
        let index = candidates[self.rng.usize(0..candidates.len())];
        let band = &self.bands[index];
        Some(Trial {
            band: Some(index),
            stimulus: Stimulus::Tone {
                ear: band.ear,
                hz: band.hz,
                level_db: band.staircase.level_db(),
            },
        })
    }

    /// Predicts a comfortably audible start level for a band from the bands
    /// already measured: the other ear at the same frequency if possible, else
    /// neighbouring frequencies in the same ear, else any measured band.
    ///
    /// Predictions are relative to the normal-hearing curve, so a listener who
    /// hears every band 12 dB better than expected starts 12 dB lower.
    fn predicted_start_db(&self, index: usize) -> f32 {
        let target = &self.bands[index];
        // (ear, frequency, threshold relative to a normal ear)
        let measured: Vec<(Option<Ear>, f32, f32)> = self
            .bands
            .iter()
            .filter_map(|band| match band.staircase.result() {
                Some(Threshold::Level(level)) => {
                    Some((band.ear, band.hz, level - expected_threshold_db(band.hz)))
                }
                _ => None,
            })
            .collect();

        let other_ear = measured
            .iter()
            .find(|(ear, hz, _)| *ear != target.ear && *hz == target.hz);
        let (offset, margin) = if let Some(&(_, _, offset)) = other_ear {
            (offset, START_ABOVE_OTHER_EAR_DB)
        } else {
            let mut same_ear: Vec<FreqPoint> = measured
                .iter()
                .filter(|(ear, _, _)| *ear == target.ear)
                .map(|&(_, hz, offset)| FreqPoint::new(hz, offset))
                .collect();
            same_ear.sort_by(|a, b| a.hz.total_cmp(&b.hz));
            if !same_ear.is_empty() {
                let offset = freq_point::interpolate(&same_ear, target.hz);
                (offset, START_ABOVE_NEIGHBOURS_DB)
            } else if !measured.is_empty() {
                let mean = measured.iter().map(|m| m.2).sum::<f32>() / measured.len() as f32;
                (mean, START_ABOVE_NEIGHBOURS_DB)
            } else {
                (0.0, START_ABOVE_EXPECTED_DB)
            }
        };
        round_to_grid(expected_threshold_db(target.hz) + offset + margin)
    }
}

/// Returns where a normal ear is expected to hear `hz` after calibration, in dBFS.
fn expected_threshold_db(hz: f32) -> f32 {
    EXPECTED_1K_THRESHOLD_DB + reference::threshold_db(hz) - reference::threshold_db(1000.0)
}

/// Returns a start level comfortably above where a normal ear would hear `hz`.
fn default_start_db(hz: f32) -> f32 {
    round_to_grid(expected_threshold_db(hz) + START_ABOVE_EXPECTED_DB)
}

/// Rounds to a multiple of 5 dB, keeping every level the staircase visits on
/// a 5 dB grid.
fn round_to_grid(level_db: f32) -> f32 {
    (level_db / 5.0).round() * 5.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A listener whose threshold is the same in dBFS at every frequency, except
    /// that the right ear can't hear above 10 kHz.
    fn simulate(stimulus: Stimulus) -> Response {
        match stimulus {
            Stimulus::Tone { ear, hz, level_db } => {
                if ear == Some(Ear::Right) && hz > 10_000.0 {
                    return Response::NotHeard;
                }
                if level_db >= -60.0 {
                    Response::Heard
                } else {
                    Response::NotHeard
                }
            }
            Stimulus::Silence => Response::NotHeard,
        }
    }

    #[test]
    fn completes_every_band() {
        let mut test = HearingTest::with_rng(
            TestLength::Standard,
            EarMode::Separate,
            fastrand::Rng::with_seed(7),
        );
        let mut trials = 0;
        while let Some(trial) = test.trial() {
            test.respond(simulate(trial.stimulus));
            trials += 1;
            assert!(trials < 2000, "test never finished");
        }
        assert!(test.is_finished());
        let profile = test.profile();
        assert_eq!(profile.measurements.len(), 20);
        assert!(
            profile.catch_trials > 0,
            "expected some catch trials in {trials} trials"
        );
        assert_eq!(profile.false_alarms, 0);
        for measurement in profile.ear(Ear::Right).filter(|m| m.hz > 10_000.0) {
            assert_eq!(measurement.threshold, Threshold::BeyondMax);
        }
        for measurement in profile.ear(Ear::Left) {
            assert_eq!(
                measurement.threshold,
                Threshold::Level(-60.0),
                "{measurement:?}"
            );
        }
    }

    #[test]
    fn sanitizing_drops_implausible_measurements() {
        let good = Measurement {
            ear: Ear::Right,
            hz: 1000.0,
            threshold: Threshold::Level(-60.0),
        };
        let profile = HearingProfile {
            measurements: vec![
                good,
                Measurement {
                    hz: f32::NAN,
                    ..good
                },
                Measurement { hz: 0.0, ..good },
                Measurement {
                    threshold: Threshold::Level(f32::INFINITY),
                    ..good
                },
                Measurement {
                    threshold: Threshold::Level(500.0),
                    ..good
                },
                good,
                Measurement {
                    ear: Ear::Left,
                    hz: 500.0,
                    ..good
                },
            ],
            catch_trials: 2,
            false_alarms: 9,
            ears_together: false,
        };
        let clean = profile.sanitized().expect("two valid measurements");
        assert_eq!(clean.measurements.len(), 2, "{:?}", clean.measurements);
        assert_eq!(clean.measurements[0].ear, Ear::Left, "sorted by ear");
        assert_eq!(clean.false_alarms, 2);

        let empty = HearingProfile {
            measurements: vec![Measurement { hz: -5.0, ..good }],
            ..Default::default()
        };
        assert_eq!(empty.sanitized(), None);
    }

    #[test]
    fn ear_difference_finds_the_weaker_ear() {
        let measure = |ear, hz, level| Measurement {
            ear,
            hz,
            threshold: Threshold::Level(level),
        };
        let mut measurements = Vec::new();
        for hz in [250.0, 1000.0, 4000.0, 8000.0] {
            measurements.push(measure(Ear::Left, hz, -70.0));
            measurements.push(measure(Ear::Right, hz, -58.0));
        }
        let profile = HearingProfile {
            measurements,
            ..Default::default()
        };
        assert_eq!(profile.ear_difference(), Some((Ear::Right, 12.0)));

        let together = HearingProfile {
            ears_together: true,
            ..profile
        };
        assert_eq!(together.ear_difference(), None);
    }

    #[test]
    fn detailed_frequencies_are_ascending_and_in_range() {
        let freqs = TestLength::Detailed.frequencies();
        assert_eq!(freqs.len(), 17);
        assert!(freqs.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(freqs[0], 63.0);
        assert_eq!(freqs[16], 16000.0);
    }

    /// A simulated listener. Thresholds differ from a normal ear by an overall
    /// offset (calibration and hearing), a per-frequency shape shared by both
    /// ears, a per-ear shift, and a little per-band noise. Answers follow a
    /// psychometric curve, so tones near threshold are sometimes missed and
    /// sometimes heard.
    struct Listener {
        rng: fastrand::Rng,
        overall: f32,
        ear_shift: [f32; 2],
        shape: Vec<(f32, f32)>,
        noise: Vec<f32>,
    }

    impl Listener {
        fn new(seed: u64, length: TestLength) -> Self {
            let mut rng = fastrand::Rng::with_seed(seed);
            let mut spread = |range: f32| rng.f32() * 2.0 * range - range;
            let overall = spread(10.0);
            let ear_shift = [spread(3.0), spread(3.0)];
            let shape = length
                .frequencies()
                .into_iter()
                .map(|hz| (hz, spread(5.0)))
                .collect();
            let noise = (0..64).map(|_| spread(2.0)).collect();
            Self {
                rng,
                overall,
                ear_shift,
                shape,
                noise,
            }
        }

        fn threshold(&self, ear: Ear, hz: f32) -> f32 {
            let index = self
                .shape
                .iter()
                .position(|s| s.0 == hz)
                .expect("tested frequency");
            expected_threshold_db(hz)
                + self.overall
                + self.shape[index].1
                + self.ear_shift[ear as usize]
                + self.noise[index * 2 + ear as usize]
        }

        /// The threshold for a tone in one ear, or in both (the better ear decides).
        fn threshold_for(&self, ear: Option<Ear>, hz: f32) -> f32 {
            match ear {
                Some(ear) => self.threshold(ear, hz),
                None => self
                    .threshold(Ear::Left, hz)
                    .min(self.threshold(Ear::Right, hz)),
            }
        }

        fn respond(&mut self, stimulus: Stimulus) -> Response {
            let Stimulus::Tone { ear, hz, level_db } = stimulus else {
                return Response::NotHeard;
            };
            let threshold = self.threshold_for(ear, hz);
            let p_heard = 1.0 / (1.0 + (-(level_db - threshold) / 1.5).exp());
            if self.rng.f32() < p_heard {
                Response::Heard
            } else {
                Response::NotHeard
            }
        }
    }

    /// Returns the average trials per test and the average absolute error of
    /// the measured thresholds, in dB.
    fn simulate_tests(length: TestLength, ears: EarMode, predict: bool) -> (f32, f32) {
        const RUNS: u64 = 60;
        let (mut trials, mut error, mut measured) = (0, 0.0, 0);
        for seed in 0..RUNS {
            let mut listener = Listener::new(seed + 1000, length);
            let mut test = HearingTest::with_rng(length, ears, fastrand::Rng::with_seed(seed));
            test.predict_starts = predict;
            while let Some(trial) = test.trial() {
                test.respond(listener.respond(trial.stimulus));
                trials += 1;
            }
            for m in test.profile().measurements {
                if let Threshold::Level(level) = m.threshold {
                    let ear = if ears == EarMode::Together {
                        None
                    } else {
                        Some(m.ear)
                    };
                    error += (level - listener.threshold_for(ear, m.hz)).abs();
                    measured += 1;
                }
            }
        }
        (trials as f32 / RUNS as f32, error / measured.max(1) as f32)
    }

    #[test]
    #[ignore = "prints timing estimates for the test-length labels"]
    fn print_test_lengths() {
        for ears in EarMode::ALL {
            for length in TestLength::ALL {
                let bands = (length.frequencies().len() * ears.targets().len()) as f32;
                for predict in [false, true] {
                    let (trials, error) = simulate_tests(length, ears, predict);
                    let minutes = trials * SECS_PER_TRIAL / 60.0;
                    let per_band = trials / bands;
                    println!(
                        "{ears:?} {length:?} predict={predict}: {trials:.0} trials \
                         ({per_band:.1}/band), {minutes:.1} min, error {error:.1} dB"
                    );
                }
            }
        }
    }

    #[test]
    fn predicted_starts_save_trials_without_losing_accuracy() {
        let (old_trials, old_error) =
            simulate_tests(TestLength::Standard, EarMode::Separate, false);
        let (new_trials, new_error) = simulate_tests(TestLength::Standard, EarMode::Separate, true);
        assert!(
            new_trials < old_trials * 0.9,
            "{old_trials} trials without prediction, {new_trials} with"
        );
        assert!(
            new_error < old_error + 0.5,
            "error {old_error} dB without prediction, {new_error} dB with"
        );
    }

    #[test]
    fn together_mode_tests_each_band_once_for_both_ears() {
        let mut test = HearingTest::with_rng(
            TestLength::Quick,
            EarMode::Together,
            fastrand::Rng::with_seed(5),
        );
        let mut tones = 0;
        while let Some(trial) = test.trial() {
            if let Stimulus::Tone { ear, .. } = trial.stimulus {
                assert_eq!(ear, None, "together mode plays in both ears");
                tones += 1;
            }
            test.respond(simulate(trial.stimulus));
        }
        let profile = test.profile();
        assert!(profile.ears_together);
        assert_eq!(profile.measurements.len(), 14);
        let left: Vec<_> = profile
            .ear(Ear::Left)
            .map(|m| (m.hz, m.threshold))
            .collect();
        let right: Vec<_> = profile
            .ear(Ear::Right)
            .map(|m| (m.hz, m.threshold))
            .collect();
        assert_eq!(left, right);
        assert!(tones < 100, "{tones} tones for 7 bands");
    }
    #[test]
    fn undo_restores_the_answered_trial_and_state() {
        let mut test = HearingTest::with_rng(
            TestLength::Quick,
            EarMode::Separate,
            fastrand::Rng::with_seed(11),
        );
        assert!(!test.can_undo());
        assert!(!test.undo(), "nothing to undo before any answer");

        // Answer a few trials, then remember the state before the next one.
        for _ in 0..6 {
            let trial = test.trial().expect("test not finished");
            test.respond(simulate(trial.stimulus));
        }
        let trial = test.trial().expect("test not finished");
        let fraction = test.fraction_done();
        let profile = test.profile();

        // A wrong answer, then two more, all taken back.
        test.respond(Response::Heard);
        for _ in 0..2 {
            let next = test.trial().expect("test not finished");
            test.respond(simulate(next.stimulus));
        }
        for _ in 0..3 {
            assert!(test.undo());
        }
        assert_eq!(
            test.trial(),
            Some(trial),
            "the undone trial is current again"
        );
        assert_eq!(test.fraction_done(), fraction);
        assert_eq!(test.profile(), profile);

        // Undoing all the way back reaches the first trial.
        while test.undo() {}
        assert_eq!(test.fraction_done(), 0.0);
        assert!(!test.can_undo());
    }

    #[test]
    fn undo_reopens_a_finished_band() {
        let mut test = HearingTest::with_rng(
            TestLength::Quick,
            EarMode::Together,
            fastrand::Rng::with_seed(2),
        );
        while let Some(trial) = test.trial() {
            test.respond(simulate(trial.stimulus));
        }
        assert!(test.is_finished());
        assert!(test.undo());
        assert!(!test.is_finished());
        let trial = test.trial().expect("the last trial is back");
        test.respond(simulate(trial.stimulus));
        assert!(test.is_finished());
    }

    #[test]
    fn progress_moves_steadily() {
        let mut test = HearingTest::with_rng(
            TestLength::Standard,
            EarMode::Separate,
            fastrand::Rng::with_seed(3),
        );
        let mut last = 0.0;
        let mut finished_halfway = 0;
        let mut trials = 0;
        while let Some(trial) = test.trial() {
            test.respond(simulate(trial.stimulus));
            trials += 1;
            let fraction = test.fraction_done();
            assert!(
                fraction >= last,
                "progress went backwards: {last} -> {fraction}"
            );
            last = fraction;
            if trials == 60 {
                finished_halfway = test.progress().0;
            }
        }
        assert_eq!(last, 1.0);
        assert!(
            finished_halfway >= 4,
            "only {finished_halfway} bands done after 60 trials"
        );
    }
}
