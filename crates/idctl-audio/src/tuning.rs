//! Buffer settings as values: what a mode asks for, what is allowed, and
//! what was in force before the app changed anything.

use serde::{Deserialize, Serialize};

/// Buffer (graph quantum) limits, in frames.
pub const QUANTUM: (u32, u32) = (16, 8192);
pub const PERIOD: (u32, u32) = (16, 1024);
pub const HEADROOM_MAX: u32 = 1024;

/// One set of buffer settings, in frames. `None` leaves the interface's
/// value as it was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuning {
    pub quantum: u32,
    pub period: Option<u32>,
    pub headroom: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Safe,
    Balanced,
    Aggressive,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Safe, Mode::Balanced, Mode::Aggressive];
}

/// What each mode asks for on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modes {
    pub safe: Tuning,
    pub balanced: Tuning,
    pub aggressive: Tuning,
}

impl Modes {
    pub fn get(&self, mode: Mode) -> Tuning {
        match mode {
            Mode::Safe => self.safe,
            Mode::Balanced => self.balanced,
            Mode::Aggressive => self.aggressive,
        }
    }

    pub fn set(&mut self, mode: Mode, tuning: Tuning) {
        match mode {
            Mode::Safe => self.safe = tuning,
            Mode::Balanced => self.balanced = tuning,
            Mode::Aggressive => self.aggressive = tuning,
        }
    }
}

/// Starting values, before a test run has measured the machine.
impl Default for Modes {
    fn default() -> Self {
        let tuning = |quantum, period, headroom| Tuning {
            quantum,
            period,
            headroom,
        };
        Self {
            safe: tuning(256, None, None),
            balanced: tuning(128, Some(128), Some(64)),
            aggressive: tuning(32, Some(32), Some(32)),
        }
    }
}

pub fn validate(t: &Tuning) -> Result<(), String> {
    if !t.quantum.is_power_of_two() || !(QUANTUM.0..=QUANTUM.1).contains(&t.quantum) {
        return Err(format!(
            "Buffer must be a power of two from {} to {} frames.",
            QUANTUM.0, QUANTUM.1
        ));
    }
    if t.period
        .is_some_and(|p| !(PERIOD.0..=PERIOD.1).contains(&p))
    {
        return Err(format!(
            "Period must be from {} to {} frames.",
            PERIOD.0, PERIOD.1
        ));
    }
    if t.headroom.is_some_and(|h| h > HEADROOM_MAX) {
        return Err(format!("Headroom must be at most {HEADROOM_MAX} frames."));
    }
    Ok(())
}

/// What was in force before the app changed anything, so it can be put back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// PipeWire's `clock.force-quantum`; 0 means not forced.
    pub force_quantum: u32,
    /// Node name, period size, headroom.
    pub nodes: Vec<(String, u32, u32)>,
}

/// State-file text: `quantum=N`, then one `node=period,headroom,name` per
/// node. The name comes last because it may hold any character but a newline.
pub fn encode(found: &Found) -> String {
    let mut text = format!("quantum={}\n", found.force_quantum);
    for (name, period, headroom) in &found.nodes {
        text.push_str(&format!("node={period},{headroom},{name}\n"));
    }
    text
}

pub fn decode(text: &str) -> Option<Found> {
    let mut lines = text.lines();
    let force_quantum = lines.next()?.strip_prefix("quantum=")?.parse().ok()?;
    let nodes = lines
        .map(|line| {
            let mut parts = line.strip_prefix("node=")?.splitn(3, ',');
            let period = parts.next()?.parse().ok()?;
            let headroom = parts.next()?.parse().ok()?;
            Some((parts.next()?.to_owned(), period, headroom))
        })
        .collect::<Option<_>>()?;
    Some(Found {
        force_quantum,
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning(quantum: u32, period: Option<u32>, headroom: Option<u32>) -> Tuning {
        Tuning {
            quantum,
            period,
            headroom,
        }
    }

    #[test]
    fn defaults_match_the_design_table() {
        let m = Modes::default();
        assert_eq!(m.get(Mode::Safe), tuning(256, None, None));
        assert_eq!(m.get(Mode::Balanced), tuning(128, Some(128), Some(64)));
        assert_eq!(m.get(Mode::Aggressive), tuning(32, Some(32), Some(32)));
    }

    #[test]
    fn validate_accepts_the_edges() {
        for t in [
            tuning(16, Some(16), Some(0)),
            tuning(8192, Some(1024), Some(1024)),
            tuning(64, None, None),
        ] {
            assert_eq!(validate(&t), Ok(()));
        }
    }

    #[test]
    fn validate_refuses_and_names_the_field() {
        for (t, field) in [
            (tuning(0, None, None), "Buffer"),
            (tuning(48, None, None), "Buffer"),
            (tuning(16_384, None, None), "Buffer"),
            (tuning(64, Some(8), None), "Period"),
            (tuning(64, Some(2048), None), "Period"),
            (tuning(64, None, Some(2048)), "Headroom"),
        ] {
            let why = validate(&t).unwrap_err();
            assert!(why.starts_with(field), "{why}");
        }
    }

    #[test]
    fn found_survives_the_state_file_format() {
        let found = Found {
            force_quantum: 64,
            nodes: vec![
                ("alsa_output.usb-X.pro-output-0".into(), 32, 32),
                ("odd name = with, commas".into(), 256, 0),
            ],
        };
        assert_eq!(decode(&encode(&found)), Some(found));
        let none = Found {
            force_quantum: 0,
            nodes: Vec::new(),
        };
        assert_eq!(decode(&encode(&none)), Some(none));
    }

    #[test]
    fn decode_refuses_junk() {
        for junk in [
            "",
            "quantum=",
            "quantum=x",
            "node=1,2,name",
            "quantum=64\nnode=1,name",
            "quantum=64\nnode=a,b,name",
            "quantum=64\nwhat=this",
            "\u{fffd}\u{fffd}\u{0}",
        ] {
            assert_eq!(decode(junk), None, "{junk:?}");
        }
    }
}
