//! An adaptive up-down procedure that finds the quietest level a listener hears.
//!
//! This follows the modified Hughson-Westlake method used for audiograms: drop
//! 10 dB after a tone is heard, rise 5 dB after it is missed. The threshold is
//! where two tones were heard on the way up, within 5 dB of each other.

use serde::{Deserialize, Serialize};

/// Level change after a heard tone, in dB.
const STEP_DOWN_DB: f32 = 10.0;
/// Level change after a missed tone, in dB.
const STEP_UP_DB: f32 = 5.0;
/// Misses at the maximum level before giving up on a band.
const MISSES_AT_MAX: u32 = 2;
/// Trials after which the staircase settles on its best estimate.
const MAX_TRIALS: u32 = 18;
/// Trials a band usually takes, for progress estimates.
const TYPICAL_TRIALS: u32 = 8;
/// Tolerance when comparing levels in dB.
const EPSILON_DB: f32 = 0.01;

/// What the listener reported after a tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Heard,
    NotHeard,
}

/// The outcome of a finished staircase.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Threshold {
    /// The quietest level heard, in dBFS.
    Level(f32),
    /// The tone was not heard even at the maximum level.
    BeyondMax,
}

/// The state of one threshold search.
#[derive(Debug, Clone)]
pub struct Staircase {
    level_db: f32,
    min_db: f32,
    max_db: f32,
    last: Option<Response>,
    ascending_hits: Vec<f32>,
    hits_at_min: usize,
    misses_at_max: u32,
    trials: u32,
    result: Option<Threshold>,
}

impl Staircase {
    /// Creates a staircase starting at `start_db`, kept within `min_db..=max_db`.
    ///
    /// The start level should be comfortably audible so the first descent
    /// familiarises the listener with the tone.
    pub fn new(start_db: f32, min_db: f32, max_db: f32) -> Self {
        Self {
            level_db: start_db.clamp(min_db, max_db),
            min_db,
            max_db,
            last: None,
            ascending_hits: Vec::new(),
            hits_at_min: 0,
            misses_at_max: 0,
            trials: 0,
            result: None,
        }
    }

    /// Returns the level of the next tone, in dBFS.
    #[must_use]
    pub fn level_db(&self) -> f32 {
        self.level_db
    }

    /// Returns the threshold once the search has finished.
    #[must_use]
    pub fn result(&self) -> Option<Threshold> {
        self.result
    }

    /// Returns roughly how far through the search this is, from 0 to 1.
    #[must_use]
    pub fn progress(&self) -> f32 {
        if self.result.is_some() {
            return 1.0;
        }
        (self.trials as f32 / TYPICAL_TRIALS as f32).min(0.9)
    }

    /// Returns a rough number of trials still needed.
    #[must_use]
    pub fn trials_left_estimate(&self) -> u32 {
        if self.result.is_some() {
            return 0;
        }
        TYPICAL_TRIALS.saturating_sub(self.trials).max(2)
    }

    /// Returns the number of responses recorded so far.
    #[cfg(test)]
    #[must_use]
    pub fn trials(&self) -> u32 {
        self.trials
    }

    /// Records the response to a tone at the current level and moves the level.
    pub fn record(&mut self, response: Response) {
        if self.result.is_some() {
            return;
        }
        self.trials += 1;
        match response {
            Response::Heard => self.on_heard(),
            Response::NotHeard => self.on_not_heard(),
        }
        self.last = Some(response);
        if self.result.is_none() && self.trials >= MAX_TRIALS {
            self.result = Some(Threshold::Level(self.best_estimate()));
        }
    }

    fn on_heard(&mut self) {
        let level = self.level_db;
        if level <= self.min_db + EPSILON_DB {
            self.hits_at_min += 1;
            if self.hits_at_min >= 2 {
                self.result = Some(Threshold::Level(level));
                return;
            }
        }
        if self.last == Some(Response::NotHeard) {
            // Two ascending hits within one step of each other settle the
            // threshold. Clinical audiometry needs two at the same level, but
            // that takes far longer with real, slightly inconsistent answers.
            if let Some(&previous) = self.ascending_hits.last()
                && (previous - level).abs() <= STEP_UP_DB + EPSILON_DB
            {
                self.result = Some(Threshold::Level((previous + level) / 2.0));
                return;
            }
            self.ascending_hits.push(level);
        }
        self.level_db = (level - STEP_DOWN_DB).max(self.min_db);
    }

    fn on_not_heard(&mut self) {
        if self.level_db >= self.max_db - EPSILON_DB {
            self.misses_at_max += 1;
            if self.misses_at_max >= MISSES_AT_MAX {
                self.result = Some(Threshold::BeyondMax);
            }
            return;
        }
        self.level_db = (self.level_db + STEP_UP_DB).min(self.max_db);
    }

    fn best_estimate(&self) -> f32 {
        if self.ascending_hits.is_empty() {
            return self.level_db;
        }
        self.ascending_hits.iter().sum::<f32>() / self.ascending_hits.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a staircase against a listener who hears everything at or above
    /// `true_threshold_db`.
    fn run(true_threshold_db: f32, start_db: f32) -> (Threshold, u32) {
        let mut staircase = Staircase::new(start_db, -110.0, -10.0);
        while staircase.result().is_none() {
            let response = if staircase.level_db() >= true_threshold_db {
                Response::Heard
            } else {
                Response::NotHeard
            };
            staircase.record(response);
        }
        (
            staircase.result().expect("loop ends with a result"),
            staircase.trials(),
        )
    }

    #[test]
    fn finds_threshold_within_one_step() {
        for true_threshold in [-90.0, -72.5, -61.0, -40.0] {
            let (result, trials) = run(true_threshold, -30.0);
            let Threshold::Level(level) = result else {
                panic!("expected a level for threshold {true_threshold}, got {result:?}");
            };
            assert!(
                level >= true_threshold && level < true_threshold + STEP_UP_DB,
                "threshold {true_threshold} estimated as {level}"
            );
            assert!(
                trials < MAX_TRIALS,
                "took {trials} trials for {true_threshold}"
            );
        }
    }

    #[test]
    fn reports_beyond_max_when_never_heard() {
        let (result, _) = run(0.0, -30.0);
        assert_eq!(result, Threshold::BeyondMax);
    }

    #[test]
    fn stops_at_minimum_level() {
        let (result, _) = run(-200.0, -100.0);
        assert_eq!(result, Threshold::Level(-110.0));
    }

    #[test]
    fn inconsistent_listener_still_finishes() {
        let mut staircase = Staircase::new(-40.0, -110.0, -10.0);
        let mut heard = true;
        while staircase.result().is_none() {
            let response = if heard {
                Response::Heard
            } else {
                Response::NotHeard
            };
            staircase.record(response);
            heard = !heard;
        }
        assert!(staircase.trials() <= MAX_TRIALS);
    }
}
