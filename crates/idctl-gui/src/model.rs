//! UI-side mirror of device state. Pure: no USB, no iced.

use std::collections::HashMap;

use idctl_protocol::codec;
use idctl_protocol::params::{kind, meters, monitor, system};
use idctl_protocol::ParamAddr;

use crate::mix::{Mixer, OFF_DB};

#[derive(Default)]
pub struct Model {
    raw: HashMap<ParamAddr, Vec<u8>>,
    pub mixer: Mixer,
    sources: Vec<u16>,
    buses: Vec<u16>,
    /// Compressor gain, full scale = no reduction.
    reduction: Vec<u16>,
}

/// Deepest gain reduction the meter shows, dB.
const GR_FLOOR: f32 = 60.0;

impl Model {
    /// A value read from the device.
    pub fn apply(&mut self, addr: ParamAddr, data: Vec<u8>) {
        if addr.kind == kind::MIXER {
            if let Some(db) = codec::decode_db(&data) {
                self.mixer.device_gain(addr, db);
            }
        }
        self.raw.insert(addr, data);
    }

    /// A value the UI is about to write: show it at once.
    pub fn assume(&mut self, addr: ParamAddr, data: &[u8]) {
        self.raw.insert(addr, data.to_vec());
    }

    pub fn meters(&mut self, group: u16, levels: Vec<u16>) {
        if group == meters::SOURCES.0 {
            self.sources = levels;
        } else if group == meters::MIX_BUSES.0 {
            self.buses = levels;
        } else if group == meters::GAIN_REDUCTION.0 {
            self.reduction = levels;
        }
    }

    pub fn flag(&self, addr: ParamAddr) -> bool {
        self.byte(addr) != 0
    }

    pub fn byte(&self, addr: ParamAddr) -> u8 {
        self.raw
            .get(&addr)
            .and_then(|d| d.first().copied())
            .unwrap_or(0)
    }

    /// Talkback is on, from the hardware button or ours.
    pub fn talking(&self) -> bool {
        self.flag(monitor::TALKBACK_STATE) || self.flag(monitor::TALKBACK)
    }
    pub fn db(&self, addr: ParamAddr) -> f32 {
        self.raw
            .get(&addr)
            .and_then(|d| codec::decode_db(d))
            .unwrap_or(OFF_DB)
    }

    pub fn unit(&self, addr: ParamAddr) -> f32 {
        self.raw
            .get(&addr)
            .and_then(|d| codec::decode_unit(d))
            .unwrap_or(0.0)
    }

    pub fn u16(&self, addr: ParamAddr) -> u16 {
        self.raw
            .get(&addr)
            .and_then(|d| Some(u16::from_le_bytes(d.get(..2)?.try_into().ok()?)))
            .unwrap_or(0)
    }

    pub fn hz(&self, addr: ParamAddr) -> f32 {
        f32::from(self.u16(addr))
    }

    pub fn sample_rate(&self) -> u32 {
        self.raw
            .get(&system::SAMPLE_RATE)
            .and_then(|d| codec::decode_u32(d))
            .unwrap_or(0)
    }

    /// Mixer source node level in dBFS.
    pub fn source_level(&self, node: usize) -> Option<f32> {
        codec::meter_db(self.sources.get(node).copied().unwrap_or(0))
    }

    /// Mix bus level in dBFS (`2 * mix + side`).
    pub fn bus_level(&self, bus: usize) -> Option<f32> {
        codec::meter_db(self.buses.get(bus).copied().unwrap_or(0))
    }

    /// Compressor gain reduction in dB (positive), 0 when unknown.
    pub fn gain_reduction(&self, ch: u8) -> f32 {
        self.reduction.get(usize::from(ch)).map_or(0.0, |&raw| {
            codec::meter_db(raw).map_or(GR_FLOOR, |db| (-db).min(GR_FLOOR))
        })
    }
}

/// What a mixer node is, for its strip header.
pub struct Channel {
    pub number: usize,
    pub source: String,
    pub name: String,
    pub group: Group,
    /// Analogue input channel, when the node is a preamp.
    pub input: Option<u8>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Group {
    Mic,
    Optical,
    Computer,
    Reverb,
}

pub fn channel(node: usize) -> Channel {
    let (source, name, group, input) = match node {
        0 | 1 => (
            format!("IN {}", node + 1),
            format!("MIC {}", node + 1),
            Group::Mic,
            Some(node as u8),
        ),
        2..=9 => (
            format!("OPT {}", node - 1),
            format!("OPT {}", node - 1),
            Group::Optical,
            None,
        ),
        10..=23 => (
            format!("DAW {}", node - 9),
            format!("DAW {}", node - 9),
            Group::Computer,
            None,
        ),
        _ => (
            format!("FX {}", if node == 24 { "L" } else { "R" }),
            format!("REVERB {}", if node == 24 { "L" } else { "R" }),
            Group::Reverb,
            None,
        ),
    };
    Channel {
        number: node + 1,
        source,
        name,
        group,
        input,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idctl_protocol::params::mixer;

    #[test]
    fn device_gains_feed_the_mixer() {
        let mut m = Model::default();
        m.apply(mixer::gain_left(4, 1), codec::encode_db(-6.0).to_vec());
        m.apply(mixer::gain_right(4, 1), codec::encode_db(OFF_DB).to_vec());
        let s = m.mixer.send(4, 1);
        assert!((s.level + 6.0).abs() < 0.01 && (s.pan + 1.0).abs() < 0.01);
    }
}
