//! Canvas controls shared by the views.

pub mod fader;
pub mod knob;

use iced::Color;

pub use fader::Fader;
pub use knob::Knob;

/// Maps a control's linear 0..1 range onto its travel.
#[derive(Clone, Copy)]
pub enum Taper {
    Linear,
    /// `travel = linear ^ (1 / k)`: spreads the top of the range out.
    Power(f32),
}

impl Taper {
    pub fn position(self, linear: f32) -> f32 {
        let linear = linear.clamp(0.0, 1.0);
        match self {
            Taper::Linear => linear,
            Taper::Power(k) => linear.powf(k),
        }
    }

    pub fn linear(self, position: f32) -> f32 {
        match self {
            Taper::Linear => position,
            Taper::Power(k) => position.powf(1.0 / k),
        }
    }
}

/// Piecewise-linear scale: `(value, fraction from top)`, value descending.
pub struct Scale(pub &'static [(f32, f32)]);

impl Scale {
    pub fn fraction(&self, value: f32) -> f32 {
        let marks = self.0;
        if value >= marks[0].0 {
            return marks[0].1;
        }
        for w in marks.windows(2) {
            let ((v0, f0), (v1, f1)) = (w[0], w[1]);
            if value >= v1 {
                return f0 + (f1 - f0) * (v0 - value) / (v0 - v1);
            }
        }
        marks[marks.len() - 1].1
    }

    pub fn value(&self, fraction: f32) -> f32 {
        let marks = self.0;
        for w in marks.windows(2) {
            let ((v0, f0), (v1, f1)) = (w[0], w[1]);
            if fraction <= f1 {
                return v0 + (v1 - v0) * (fraction - f0) / (f1 - f0);
            }
        }
        marks[marks.len() - 1].0
    }
}

pub fn mix_colour(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgb(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
    )
}

#[cfg(test)]
mod tests {
    use super::fader::GAIN;
    use super::Taper;

    #[test]
    fn gain_scale_round_trips() {
        for db in [6.0, 0.0, -2.5, -12.0, -45.0, -128.0] {
            assert!((GAIN.value(GAIN.fraction(db)) - db).abs() < 0.01, "{db}");
        }
    }

    #[test]
    fn power_taper_inverts() {
        let t = Taper::Power(3.4);
        for x in [0.0, 0.2, 0.72, 1.0] {
            assert!((t.linear(t.position(x)) - x).abs() < 1e-4);
        }
    }
}
