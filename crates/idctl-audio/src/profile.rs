//! A profile: the buffer presets that suit one machine, as a small text a
//! user can share and another user can paste in.

use serde::{Deserialize, Serialize};

use crate::tuning::{Presets, Tuning, BUFFERS};

pub const VERSION: u32 = 1;
/// Larger text is not a profile.
pub const MAX_BYTES: usize = 16 * 1024;
const MAX_LABEL: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    version: u32,
    /// CPU model the presets were tuned on.
    pub machine: String,
    /// PipeWire version they were tuned on.
    pub pipewire: String,
    presets: Vec<Tuning>,
}

impl Profile {
    pub fn new(machine: String, pipewire: String, presets: &Presets) -> Self {
        Self {
            version: VERSION,
            machine,
            pipewire,
            presets: presets.all().to_vec(),
        }
    }

    /// Read a profile, refusing anything this version would not have written.
    pub fn parse(text: &str) -> Result<Profile, String> {
        if text.len() > MAX_BYTES {
            return Err("That text is too large to be a profile.".into());
        }
        let profile: Profile = serde_json::from_str(text)
            .map_err(|_| "That text is not an D+ Control profile.".to_owned())?;
        if profile.version != VERSION {
            return Err(format!(
                "This profile is version {}; this D+ Control reads version {VERSION}.",
                profile.version
            ));
        }
        if profile.machine.len() > MAX_LABEL || profile.pipewire.len() > MAX_LABEL {
            return Err("That profile has an over-long label.".into());
        }
        profile.presets()?;
        Ok(profile)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a profile is plain data")
    }

    /// The presets, checked: one for each buffer size and no other.
    pub fn presets(&self) -> Result<Presets, String> {
        let mut presets = Presets::default();
        let mut seen = Vec::new();
        for tuning in &self.presets {
            if seen.contains(&tuning.quantum) {
                return Err(format!("The {} buffer appears twice.", tuning.quantum));
            }
            seen.push(tuning.quantum);
            presets.set(*tuning)?;
        }
        if let Some(missing) = BUFFERS.iter().find(|b| !seen.contains(b)) {
            return Err(format!("The {missing} buffer is missing."));
        }
        Ok(presets)
    }

    /// True when it was tuned on a different machine, so its numbers are a
    /// starting point and not a promise.
    pub fn untested_on(&self, machine: &str) -> bool {
        self.machine != machine
    }
}

/// CPU model from `/proc/cpuinfo` text; `unknown` if there is none.
pub fn machine(cpuinfo: &str) -> String {
    cpuinfo
        .lines()
        .find_map(|l| l.strip_prefix("model name"))
        .and_then(|l| l.split_once(':'))
        .map_or("unknown".into(), |(_, name)| name.trim().to_owned())
}

/// The library version from `pw-cli --version` output.
pub fn pipewire_version(out: &str) -> String {
    out.lines()
        .find_map(|l| l.strip_prefix("Linked with libpipewire "))
        .map_or("unknown".into(), |v| v.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::{Presets, Tuning};

    fn tuned() -> Presets {
        let mut presets = Presets::default();
        presets
            .set(Tuning {
                quantum: 64,
                period: Some(128),
                headroom: Some(64),
            })
            .unwrap();
        presets
    }

    #[test]
    fn round_trips() {
        let profile = Profile::new("Ryzen 7".into(), "1.6.9".into(), &tuned());
        let back = Profile::parse(&profile.to_json()).unwrap();
        assert_eq!(back, profile);
        assert_eq!(back.presets().unwrap(), tuned());
    }

    #[test]
    fn refuses_other_versions() {
        let text = Profile::new("x".into(), "y".into(), &tuned())
            .to_json()
            .replace("\"version\": 1", "\"version\": 2");
        assert_eq!(
            Profile::parse(&text),
            Err("This profile is version 2; this D+ Control reads version 1.".into())
        );
    }

    #[test]
    fn refuses_values_the_page_would_never_set() {
        let text = Profile::new("x".into(), "y".into(), &tuned()).to_json();
        let bad_period = text.replace("128", "4");
        assert!(Profile::parse(&bad_period).unwrap_err().contains("Period"));
        let bad_size = text.replace("\"quantum\": 64", "\"quantum\": 100");
        assert!(Profile::parse(&bad_size).is_err());
    }

    #[test]
    fn refuses_oversize_and_wrong_shape() {
        assert!(Profile::parse(&" ".repeat(MAX_BYTES + 1))
            .unwrap_err()
            .contains("too large"));
        assert!(Profile::parse("not json").is_err());
        assert!(Profile::parse(r#"{"version":1,"presets":"fast"}"#).is_err());
        let long = Profile::new("m".repeat(500), "y".into(), &tuned()).to_json();
        assert!(Profile::parse(&long).is_err());
    }

    #[test]
    fn refuses_a_profile_with_a_size_missing_or_repeated() {
        let mut profile = Profile::new("x".into(), "y".into(), &tuned());
        profile.presets.pop();
        assert!(Profile::parse(&profile.to_json()).is_err());
        let mut profile = Profile::new("x".into(), "y".into(), &tuned());
        profile.presets[1] = profile.presets[0];
        assert!(Profile::parse(&profile.to_json()).is_err());
    }

    #[test]
    fn untested_when_the_machine_differs() {
        let profile = Profile::new("Ryzen 7".into(), "1.6.9".into(), &tuned());
        assert!(!profile.untested_on("Ryzen 7"));
        assert!(profile.untested_on("Core i5"));
    }

    #[test]
    fn machine_reads_the_model_name() {
        let cpuinfo = "processor\t: 0\nmodel name\t: AMD Ryzen 7 8745HS\nmodel name\t: other\n";
        assert_eq!(machine(cpuinfo), "AMD Ryzen 7 8745HS");
        assert_eq!(machine(""), "unknown");
    }

    #[test]
    fn pipewire_version_comes_from_the_linked_line() {
        let out = "pw-cli\nCompiled with libpipewire 1.4.0\nLinked with libpipewire 1.6.9\n";
        assert_eq!(pipewire_version(out), "1.6.9");
        assert_eq!(pipewire_version(""), "unknown");
    }
}
