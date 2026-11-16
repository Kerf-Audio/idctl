//! Every parameter the GUI mirrors, with its wire length.

use idctl_protocol::dsp::{self, comp};
use idctl_protocol::params::{
    input, mixer, monitor, output, reverb, system, ANALOGUE_INPUTS, MIXES,
};
use idctl_protocol::ParamAddr;

use crate::channel_dsp::DSP_CHANNELS;
use crate::mix::NODES;

/// Output slots of the headphone pair.
pub const PHONES: [u8; 2] = [4, 5];
/// Routable output slots: line, ALT, phones, ADAT, loopback.
pub const OUTPUT_SLOTS: u8 = 16;

pub fn all() -> Vec<(ParamAddr, u16)> {
    let mut v = vec![
        (system::OPTICAL_INPUT_MODE, 1),
        (system::OPTICAL_OUTPUT_MODE, 1),
        (system::clock_locked(0), 1),
        (monitor::MONO, 1),
        (monitor::MID_SIDE, 1),
        (monitor::MUTE, 1),
        (monitor::DIM, 1),
        (monitor::DIM_LEVEL, 2),
        (monitor::TALKBACK, 1),
        (monitor::VOLUME, 2),
        (monitor::SOURCE, 1),
        (reverb::DECAY, 2),
        (reverb::DAMPING, 2),
        (reverb::BLEND, 2),
    ];
    for ch in 0..ANALOGUE_INPUTS {
        v.extend([
            (input::gain(ch), 2),
            (input::trim(ch), 2),
            (input::mute(ch), 1),
            (input::character(ch), 1),
            (input::phantom(ch), 1),
            (input::pad(ch), 1),
            (input::hpf(ch), 1),
            (input::hmx(ch), 1),
            (input::lift(ch), 1),
        ]);
    }
    for node in 0..NODES as u8 {
        for mix in 0..MIXES {
            v.push((mixer::gain_left(node, mix), 2));
            v.push((mixer::gain_right(node, mix), 2));
        }
    }
    for side in 0..2 * MIXES {
        v.push((mixer::master(side), 2));
    }
    for slot in PHONES {
        v.push((output::level(slot), 2));
    }
    for slot in 0..OUTPUT_SLOTS {
        v.push((output::source(slot), 1));
    }
    for ch in 0..DSP_CHANNELS as u8 {
        v.extend([
            (comp::attack(ch), 2),
            (comp::release(ch), 2),
            (comp::threshold(ch), 2),
            (comp::ratio(ch), 2),
            (comp::mix(ch), 2),
            (comp::makeup(ch), 2),
            (dsp::order(ch), 1),
        ]);
    }
    v
}
