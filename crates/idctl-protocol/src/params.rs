//! Parameter addresses of the iD2+ control tree.
//!
//! Value types: `u8` flags/enums, `i16` dB (see [`crate::codec`]), `u32`
//! counts.

use crate::request::ParamAddr;

/// Entity types.
pub mod kind {
    pub const SYSTEM: u8 = 0;
    pub const INPUT: u8 = 1;
    pub const OUTPUT: u8 = 2;
    pub const DIGITAL_IN: u8 = 3;
    pub const MONITOR: u8 = 4;
    pub const MIXER: u8 = 5;
    pub const MIX_MASTER: u8 = 6;
    pub const DSP: u8 = 7;
    pub const REVERB: u8 = 9;
    pub const DIGITAL_OUT: u8 = 10;
    pub const AUTO_GAIN: u8 = 11;
}

/// iD2+ has two analogue inputs (channels 0 and 1).
pub const ANALOGUE_INPUTS: u8 = 2;
/// Mixes: Main, Mix A, Mix B and the reverb return (inferred ordering).
pub const MIXES: u8 = 4;

/// Meter groups for `bRequest 4`: `(group id, level count)`. The per-index
/// order is inferred from the counts matching the mixer and bus layouts.
pub mod meters {
    /// Mixer source nodes 0..26 (mics, ADAT, computer 1..14, reverb).
    pub const SOURCES: (u16, u16) = (0, 26);
    /// Mix buses: Main L/R, Mix A L/R, Mix B L/R, reverb L/R.
    pub const MIX_BUSES: (u16, u16) = (1, 8);
    /// Monitor outputs (main L/R, line 3/4, headphones L/R).
    pub const OUTPUTS: (u16, u16) = (2, 6);
    /// Compressor gain reduction, one per DSP channel.
    pub const GAIN_REDUCTION: (u16, u16) = (3, 10);
}

/// Read-only device state (`inst` 0).
pub mod system {
    use super::*;
    /// `u32` Hz. Changing the rate goes through standard UAC2, not this protocol.
    pub const SAMPLE_RATE: ParamAddr = ParamAddr::new(kind::SYSTEM, 0, 2);
    /// `u32` clock id.
    pub const CLOCK_SOURCE: ParamAddr = ParamAddr::new(kind::SYSTEM, 0, 3);
    /// `u8` front-panel meter source.
    pub const PANEL_METER_SOURCE: ParamAddr = ParamAddr::new(kind::SYSTEM, 0, 8);
    /// `u8` global mute.
    pub const GLOBAL_MUTE: ParamAddr = ParamAddr::new(kind::SYSTEM, 0, 9);
    /// `u8` volume recall on power-up.
    pub const VOLUME_RECALL: ParamAddr = ParamAddr::new(kind::SYSTEM, 0, 0x0c);
    /// `u8` per clock `inst`: 0 unlocked, non-zero locked.
    pub const fn clock_locked(clock: u8) -> ParamAddr {
        ParamAddr::new(kind::DIGITAL_IN, clock, 1)
    }
    /// `u8`: 0 ADAT, 1 S/PDIF.
    pub const OPTICAL_INPUT_MODE: ParamAddr = ParamAddr::new(kind::DIGITAL_IN, 0, 2);
    /// `u8`: 0 ADAT, 1 S/PDIF (inferred from the input enum).
    pub const OPTICAL_OUTPUT_MODE: ParamAddr = ParamAddr::new(kind::DIGITAL_OUT, 0, 0);
}

/// Per-channel preamp controls (`inst` = channel).
pub mod input {
    use super::*;
    const fn at(ch: u8, param: u8) -> ParamAddr {
        ParamAddr::new(kind::INPUT, ch, param)
    }
    /// `i16` dB, 0..=75 (60 when trim is in use).
    pub const fn gain(ch: u8) -> ParamAddr {
        at(ch, 0)
    }
    /// `u8`.
    pub const fn mute(ch: u8) -> ParamAddr {
        at(ch, 1)
    }
    /// `u8` first tone switch (harmonic saturation). The Warm key sets both tone switches.
    pub const fn tone_a(ch: u8) -> ParamAddr {
        at(ch, 3)
    }
    /// `u8` input mode: 0, 1 or 3.
    pub const fn character(ch: u8) -> ParamAddr {
        at(ch, 4)
    }
    /// `u8` +48 V.
    pub const fn phantom(ch: u8) -> ParamAddr {
        at(ch, 5)
    }
    /// `u8` -10 dB pad.
    pub const fn pad(ch: u8) -> ParamAddr {
        at(ch, 6)
    }
    /// `u8` second tone switch (high-frequency sheen).
    pub const fn tone_b(ch: u8) -> ParamAddr {
        at(ch, 0x0a)
    }
    /// `u8` high-pass filter on/off.
    pub const fn hpf(ch: u8) -> ParamAddr {
        at(ch, 0x0b)
    }
    /// `u8` record source.
    pub const fn record_source(ch: u8) -> ParamAddr {
        at(ch, 0x0d)
    }
    /// `i16` dB, 0..=15.
    pub const fn trim(ch: u8) -> ParamAddr {
        ParamAddr::new(kind::DSP, ch, 0x0f)
    }
}

/// Monitor controller (`inst` 0).
pub mod monitor {
    use super::*;
    const fn at(param: u8) -> ParamAddr {
        ParamAddr::new(kind::MONITOR, 0, param)
    }
    /// `u8` mid/side monitoring.
    pub const MID_SIDE: ParamAddr = at(1);
    /// `u8`.
    pub const MONO: ParamAddr = at(0);
    /// `u8`.
    pub const MUTE: ParamAddr = at(2);
    /// `u8`.
    pub const DIM: ParamAddr = at(3);
    /// `i16` dB, dim attenuation.
    pub const DIM_LEVEL: ParamAddr = at(4);
    /// `u8`, read-only, pushed by hardware.
    pub const TALKBACK_STATE: ParamAddr = at(5);
    /// `u8`, write.
    pub const TALKBACK: ParamAddr = at(6);
    /// `i16` dB, ALT trim.
    pub const ALT_TRIM: ParamAddr = at(8);
    /// `i16` dB, master monitor volume (`-128` is minimum).
    pub const VOLUME: ParamAddr = at(0x0a);
    /// `u8` sample-bus source feeding the monitors.
    pub const SOURCE: ParamAddr = at(0x0c);
    /// `i32` function-button mode.
    pub const FUNCTION_MODE: ParamAddr = at(0x0d);
    /// `u8` function button active.
    pub const FUNCTION_ACTIVE: ParamAddr = at(0x0e);
    /// `u8`: 0 centre, 1 left, 2 right.
    pub const MONO_CHANNEL: ParamAddr = at(0x12);
    /// `i16` dB reference level.
    pub const REFERENCE_LEVEL: ParamAddr = at(0x13);
    /// `u8` trigger: jump to the reference level.
    pub const SET_TO_REF: ParamAddr = at(0x14);
}

/// Output stage and routing (`inst` = output slot).
pub mod output {
    use super::*;
    /// `u8` sample-bus source routed to the slot.
    pub const fn source(slot: u8) -> ParamAddr {
        ParamAddr::new(kind::OUTPUT, slot, 0)
    }
    /// `i16` dB.
    pub const fn level(slot: u8) -> ParamAddr {
        ParamAddr::new(kind::OUTPUT, slot, 1)
    }
}

/// Software mixer. Mute, solo and link are folded into the gains by the host.
pub mod mixer {
    use super::*;
    /// `i16` dB for `source` (`2 * pair + side`, 0..26) into `mix`'s left bus.
    pub const fn gain_left(source: u8, mix: u8) -> ParamAddr {
        ParamAddr::new(kind::MIXER, source, 2 * mix + 1)
    }
    /// `i16` dB for `source` into `mix`'s right bus.
    pub const fn gain_right(source: u8, mix: u8) -> ParamAddr {
        ParamAddr::new(kind::MIXER, source, 2 * mix + 2)
    }
    /// `i16` dB master level; write it to both `2 * mix` and `2 * mix + 1`.
    pub const fn master(mix_side: u8) -> ParamAddr {
        ParamAddr::new(kind::MIX_MASTER, mix_side, 0)
    }
}

/// Reverb (`inst` 0).
pub mod reverb {
    use super::*;
    /// `u16` normalised 0..=1, short to long.
    pub const DECAY: ParamAddr = ParamAddr::new(kind::REVERB, 0, 2);
    /// `u16` Hz, 20..=20000.
    pub const DAMPING: ParamAddr = ParamAddr::new(kind::REVERB, 0, 3);
    /// `u16` normalised 0..=1, early to late reflections.
    pub const BLEND: ParamAddr = ParamAddr::new(kind::REVERB, 0, 4);
}

/// Auto gain (global, `inst` 0).
pub mod auto_gain {
    use super::*;
    const fn at(param: u8) -> ParamAddr {
        ParamAddr::new(kind::AUTO_GAIN, 0, param)
    }
    /// `u8`: 0 stopped, 1 running, 2 success, 3 failed, 4 open, 5 closed.
    pub const STATUS: ParamAddr = at(0);
    /// `u32`.
    pub const DURATION: ParamAddr = at(1);
    /// `u32`.
    pub const COUNTDOWN: ParamAddr = at(2);
    /// `i16` dB, -65..=-40.
    pub const THRESHOLD: ParamAddr = at(3);
    /// `u8`: 0 idle, 1 run, 2 restore, 3 arming, 4 close.
    pub const TRIGGER: ParamAddr = at(4);
    /// Per-channel `u8` result: 0 none, 1 pass, 2 timeout, 3 hot, 4 no signal.
    pub const fn channel_status(ch: u8) -> ParamAddr {
        ParamAddr::new(kind::INPUT, ch, 7)
    }
    /// Per-channel `i16` dB target, -18..=-6.
    pub const fn channel_target(ch: u8) -> ParamAddr {
        ParamAddr::new(kind::INPUT, ch, 9)
    }
}
