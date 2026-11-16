//! State-variable filter coefficients (trapezoidal integration form).

use super::{Band, FilterType};

/// Coefficients `[m0, m1, m2, a1, a2, a3]` plus the prewarp gain `g`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Svf {
    pub m: [f64; 3],
    pub a: [f64; 3],
    g: f64,
    k: f64,
}

impl Svf {
    /// Unity pass-through.
    const PASS: Self = Self {
        m: [1.0, 0.0, 0.0],
        a: [0.0; 3],
        g: 1.0,
        k: 0.0,
    };

    /// Silence.
    const MUTE: Self = Self {
        m: [0.0; 3],
        a: [0.0; 3],
        g: 1.0,
        k: 0.0,
    };

    /// Coefficients for `kind` at `hz` with `gain_db` and `q`, sample rate `fs`.
    pub fn new(kind: FilterType, hz: f64, gain_db: f64, q: f64, fs: f64) -> Self {
        let tan = (std::f64::consts::PI * hz.min(0.499 * fs) / fs).tan();
        let q = q.max(1e-3);
        let amp = 10f64.powf(gain_db / 40.0);
        match kind {
            FilterType::LowPass => Self::build(tan, 1.0 / q, [0.0, 0.0, 1.0]),
            FilterType::HighPass => {
                let k = 1.0 / q;
                Self::build(tan, k, [1.0, -k, -1.0])
            }
            FilterType::Peak => {
                let k = 1.0 / (amp * q);
                Self::build(tan, k, [1.0, k * (amp * amp - 1.0), 0.0])
            }
            FilterType::LowShelf => {
                let k = 1.0 / q;
                Self::build(tan / amp.sqrt(), k, [1.0, k * (amp - 1.0), amp * amp - 1.0])
            }
            FilterType::HighShelf => {
                let k = 1.0 / q;
                Self::build(
                    tan * amp.sqrt(),
                    k,
                    [amp * amp, amp * (1.0 - amp) * k, 1.0 - amp * amp],
                )
            }
            FilterType::AllPass if hz < 20_000.0 => {
                let k = 1.0 / q;
                Self::build(tan, k, [1.0, -2.0 * k, 0.0])
            }
            FilterType::AllPass | FilterType::Bypass => Self::PASS,
            FilterType::Mute => Self::MUTE,
        }
    }

    /// Coefficients for a band.
    pub fn of(band: &Band, fs: f64) -> Self {
        Self::new(
            band.kind,
            f64::from(band.hz),
            f64::from(band.gain_db),
            f64::from(band.q),
            fs,
        )
    }

    fn build(g: f64, k: f64, m: [f64; 3]) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self {
            m,
            a: [a1, a2, a3],
            g,
            k,
        }
    }

    /// Magnitude response in dB at `f` Hz, floored at -120 dB.
    pub fn magnitude_db(&self, f: f64, fs: f64) -> f64 {
        let w = (std::f64::consts::PI * f.min(0.499 * fs) / fs).tan() / self.g;
        // Analogue prototype: 1 / (1 - w^2 + j k w) with s = j w.
        let (dr, di) = (1.0 - w * w, self.k * w);
        let norm = dr * dr + di * di;
        let (lr, li) = (dr / norm, -di / norm);
        // m0 + m1 * (j w) * lp + m2 * lp
        let re = self.m[0] + self.m[1] * (-w * li) + self.m[2] * lr;
        let im = self.m[1] * (w * lr) + self.m[2] * li;
        (10.0 * (re * re + im * im).log10()).max(-120.0)
    }
}
