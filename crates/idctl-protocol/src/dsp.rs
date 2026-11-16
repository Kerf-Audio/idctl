//! Per-channel dynamics and equaliser parameters (type 7).
//!
//! The device takes each EQ band as a small control packet plus one
//! fixed-point coefficient blob per sample rate.

mod svf;

use crate::params::kind;
use crate::request::ParamAddr;
use svf::Svf;

/// Filter shapes a band can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterType {
    LowPass,
    HighPass,
    LowShelf,
    HighShelf,
    Peak,
    AllPass,
    Bypass,
    Mute,
}

impl FilterType {
    /// Wire identifier of the filter type.
    pub const fn id(self) -> u8 {
        match self {
            Self::LowPass => 1,
            Self::HighPass => 3,
            Self::LowShelf => 5,
            Self::HighShelf => 7,
            Self::Peak => 8,
            Self::AllPass => 13,
            Self::Bypass => 11,
            Self::Mute => 14,
        }
    }
}

/// One EQ band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub kind: FilterType,
    pub hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

/// The four user bands in order: low shelf, peak, peak, high shelf.
pub const USER_BANDS: [Band; 4] = [
    Band {
        kind: FilterType::LowShelf,
        hz: 25.0,
        gain_db: 0.0,
        q: 0.707,
    },
    Band {
        kind: FilterType::Peak,
        hz: 500.0,
        gain_db: 0.0,
        q: 2.0,
    },
    Band {
        kind: FilterType::Peak,
        hz: 2000.0,
        gain_db: 0.0,
        q: 2.0,
    },
    Band {
        kind: FilterType::HighShelf,
        hz: 12_800.0,
        gain_db: 0.0,
        q: 0.707,
    },
];

/// Magnitude response in dB of one band at `f` Hz, sample rate `fs`.
pub fn magnitude_db(band: &Band, f: f32, fs: f32) -> f32 {
    Svf::of(band, f64::from(fs)).magnitude_db(f64::from(f), f64::from(fs)) as f32
}

/// Compressor parameters.
pub mod comp {
    use super::{kind, ParamAddr};

    const fn at(ch: u8, param: u8) -> ParamAddr {
        ParamAddr::new(kind::DSP, ch, param)
    }

    pub const fn attack(ch: u8) -> ParamAddr {
        at(ch, 0)
    }

    pub const fn release(ch: u8) -> ParamAddr {
        at(ch, 1)
    }

    pub const fn threshold(ch: u8) -> ParamAddr {
        at(ch, 2)
    }

    pub const fn ratio(ch: u8) -> ParamAddr {
        at(ch, 3)
    }

    pub const fn mix(ch: u8) -> ParamAddr {
        at(ch, 4)
    }

    pub const fn makeup(ch: u8) -> ParamAddr {
        at(ch, 0x0e)
    }

    /// Attack or release time: sixths of a millisecond, 3.3333 to 10000 ms.
    pub fn encode_time(ms: f32) -> [u8; 2] {
        ((ms.clamp(10.0 / 3.0, 10_000.0) * 6.0).round() as u16).to_le_bytes()
    }

    pub fn decode_time(raw: &[u8]) -> f32 {
        f32::from(u16::from_le_bytes([raw[0], raw[1]])) / 6.0
    }

    /// Ratio in 1/256 steps, 1 to 16.
    pub fn encode_ratio(r: f32) -> [u8; 2] {
        ((r.clamp(1.0, 16.0) * 256.0).round() as u16).to_le_bytes()
    }

    pub fn decode_ratio(raw: &[u8]) -> f32 {
        f32::from(u16::from_le_bytes([raw[0], raw[1]])) / 256.0
    }

    /// Dry/wet blend; zero switches the compressor off.
    pub fn encode_mix(on: bool, blend: f32) -> [u8; 2] {
        if on {
            ((blend.clamp(0.0, 1.0) * 32767.0).round() as u16).to_le_bytes()
        } else {
            [0, 0]
        }
    }
}

/// Order of EQ and compressor: 0 = EQ then compressor, 1 = compressor then EQ.
pub const fn order(ch: u8) -> ParamAddr {
    ParamAddr::new(kind::DSP, ch, 5)
}

/// One wire write: a normal SET or a MEM (bRequest 7) write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Write {
    Set(ParamAddr, Vec<u8>),
    Mem(ParamAddr, Vec<u8>),
}

const BLOB_PARAM: u8 = 0x1d;
const USER_RATES: [u32; 4] = [44_100, 48_000, 88_200, 96_000];
const FIXED_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const HPF_BAND: u8 = 0;
const ALL_PASS_BAND: u8 = 1;
const FIRST_USER_BAND: u8 = 4;
const AP_ON_ID: u8 = 12;

/// Q raw value of the high-pass filter (about 0.707).
const HPF_Q_RAW: u16 = 181;
/// Q raw value of the all-pass filter (1.0).
const ALL_PASS_Q_RAW: u16 = 256;

/// Packet plus one coefficient blob per rate for user band `index` (0..4 -> band 4..7).
pub fn user_band(ch: u8, index: usize, band: &Band, enabled: bool) -> Vec<Write> {
    let number = FIRST_USER_BAND + index as u8;
    let packet = packet(
        band.kind.id(),
        band.hz,
        (band.gain_db * 256.0).round() as i16,
        (band.q * 256.0).round() as u16,
    );
    let shape = if enabled {
        *band
    } else {
        Band {
            kind: FilterType::Bypass,
            ..*band
        }
    };
    let mut writes = vec![Write::Set(
        ParamAddr::new(kind::DSP, ch, 6 + number),
        packet,
    )];
    writes.extend(blobs(ch, number, &USER_RATES, |fs| Svf::of(&shape, fs)));
    writes
}

/// High-pass filter, 20 to 250 Hz.
pub fn hpf(ch: u8, hz: f32, enabled: bool) -> Vec<Write> {
    let hz = hz.clamp(20.0, 250.0);
    let kind_of = if enabled {
        FilterType::HighPass
    } else {
        FilterType::Bypass
    };
    let q = f64::from(HPF_Q_RAW) / 256.0;
    let mut writes = vec![Write::Set(
        ParamAddr::new(kind::DSP, ch, 6 + HPF_BAND),
        packet(FilterType::HighPass.id(), hz, 0, HPF_Q_RAW),
    )];
    writes.extend(blobs(ch, HPF_BAND, &FIXED_RATES, |fs| {
        Svf::new(kind_of, f64::from(hz), 0.0, q, fs)
    }));
    writes
}

/// Variable-phase all-pass, `degrees` 0..180.
///
/// The all-pass corner runs from 101 kHz (0 degrees) down to 1 kHz
/// (180 degrees). The packet's frequency field is 16 bits wide, so values
/// above 65535 Hz saturate there; the coefficients use the exact frequency.
pub fn vari_phase(ch: u8, degrees: f32, enabled: bool) -> Vec<Write> {
    let hz = phase_hz(degrees);
    let id = if enabled { AP_ON_ID } else { 0 };
    let kind_of = if enabled {
        FilterType::AllPass
    } else {
        FilterType::Bypass
    };
    let q = f64::from(ALL_PASS_Q_RAW) / 256.0;
    let mut writes = vec![Write::Set(
        ParamAddr::new(kind::DSP, ch, 6 + ALL_PASS_BAND),
        packet(id, hz, 0, ALL_PASS_Q_RAW),
    )];
    writes.extend(blobs(ch, ALL_PASS_BAND, &FIXED_RATES, |fs| {
        Svf::new(kind_of, f64::from(hz), 0.0, q, fs)
    }));
    writes
}

fn phase_hz(degrees: f32) -> f32 {
    let t = (degrees / 180.0).clamp(0.0, 1.0);
    let u = if t <= 0.5 {
        1.7 * t
    } else {
        0.85 + 0.3 * (t - 0.5)
    };
    (1.0 - u) * 100_000.0 + 1000.0
}

/// 8-byte band control packet.
fn packet(id: u8, hz: f32, gain_raw: i16, q_raw: u16) -> Vec<u8> {
    let mut p = Vec::with_capacity(8);
    p.push(id);
    p.push(0);
    p.extend_from_slice(&(hz.round() as u16).to_le_bytes());
    p.extend_from_slice(&gain_raw.to_le_bytes());
    p.extend_from_slice(&q_raw.to_le_bytes());
    p
}

/// One 32-byte coefficient blob per sample rate.
fn blobs(ch: u8, band: u8, rates: &[u32], coeffs: impl Fn(f64) -> Svf) -> Vec<Write> {
    let addr = ParamAddr::new(kind::DSP, ch, BLOB_PARAM);
    let m_scale = if band == ALL_PASS_BAND { 4.0 } else { 2.0 };
    rates
        .iter()
        .map(|&fs| {
            let c = coeffs(f64::from(fs));
            let words = [
                i32::from(band),
                fs as i32,
                fixed(c.m[0] / m_scale),
                fixed(c.m[1] / m_scale),
                fixed(c.m[2] / m_scale),
                fixed(c.a[0]),
                fixed(c.a[1]),
                fixed(c.a[2]),
            ];
            Write::Mem(addr, words.iter().flat_map(|w| w.to_le_bytes()).collect())
        })
        .collect()
}

/// Q2.30 fixed point, truncated toward zero and saturated.
fn fixed(x: f64) -> i32 {
    (x * 1_073_741_824.0).clamp(-2_147_483_648.0, 2_147_483_647.0) as i32
}

#[cfg(test)]
mod tests;
