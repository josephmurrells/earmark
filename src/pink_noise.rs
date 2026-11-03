//! Pink noise: equal energy per octave, a neutral test signal for hearing EQ changes.

/// Brings the filter's output to roughly -18 dBFS RMS.
const OUTPUT_SCALE: f32 = 0.05;

/// A pink noise generator (Paul Kellet's refined filter on white noise).
#[derive(Debug, Clone)]
pub struct PinkNoise {
    rng: fastrand::Rng,
    state: [f32; 7],
}

impl PinkNoise {
    /// Creates a generator with its own random sequence.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: fastrand::Rng::with_seed(seed),
            state: [0.0; 7],
        }
    }

    /// Returns the next sample.
    pub fn next_sample(&mut self) -> f32 {
        let white = self.rng.f32() * 2.0 - 1.0;
        let b = &mut self.state;
        b[0] = 0.99886 * b[0] + white * 0.055_517_9;
        b[1] = 0.99332 * b[1] + white * 0.075_075_9;
        b[2] = 0.96900 * b[2] + white * 0.153_852;
        b[3] = 0.86650 * b[3] + white * 0.310_485_6;
        b[4] = 0.55000 * b[4] + white * 0.532_952_2;
        b[5] = -0.7616 * b[5] - white * 0.016_898_0;
        let pink = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + white * 0.5362;
        b[6] = white * 0.115_926;
        pink * OUTPUT_SCALE
    }
}
