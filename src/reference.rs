//! Reference hearing thresholds for young adults with normal hearing.
//!
//! Everybody is far less sensitive to deep bass and very high treble than to the
//! midrange. Subtracting this curve from a measured threshold leaves only what is
//! specific to the listener and their headphones.

use crate::freq_point::{self, FreqPoint};

/// Free-field threshold of hearing in dB SPL: ISO 226:2003 up to 12.5 kHz,
/// ISO 389-7 above that.
const THRESHOLDS: [FreqPoint; 31] = [
    FreqPoint::new(20.0, 78.5),
    FreqPoint::new(25.0, 68.7),
    FreqPoint::new(31.5, 59.5),
    FreqPoint::new(40.0, 51.1),
    FreqPoint::new(50.0, 44.0),
    FreqPoint::new(63.0, 37.5),
    FreqPoint::new(80.0, 31.5),
    FreqPoint::new(100.0, 26.5),
    FreqPoint::new(125.0, 22.1),
    FreqPoint::new(160.0, 17.9),
    FreqPoint::new(200.0, 14.4),
    FreqPoint::new(250.0, 11.4),
    FreqPoint::new(315.0, 8.6),
    FreqPoint::new(400.0, 6.2),
    FreqPoint::new(500.0, 4.4),
    FreqPoint::new(630.0, 3.0),
    FreqPoint::new(800.0, 2.2),
    FreqPoint::new(1000.0, 2.4),
    FreqPoint::new(1250.0, 3.5),
    FreqPoint::new(1600.0, 1.7),
    FreqPoint::new(2000.0, -1.3),
    FreqPoint::new(2500.0, -4.2),
    FreqPoint::new(3150.0, -6.0),
    FreqPoint::new(4000.0, -5.4),
    FreqPoint::new(5000.0, -1.5),
    FreqPoint::new(6300.0, 6.0),
    FreqPoint::new(8000.0, 12.6),
    FreqPoint::new(10000.0, 13.9),
    FreqPoint::new(12500.0, 12.3),
    FreqPoint::new(14000.0, 18.4),
    FreqPoint::new(16000.0, 40.2),
];

/// Returns the normal-hearing threshold at `hz`, in dB SPL.
#[must_use]
pub fn threshold_db(hz: f32) -> f32 {
    freq_point::interpolate(&THRESHOLDS, hz)
}
