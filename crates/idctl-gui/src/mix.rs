//! Host-side mixer logic. The device only knows per-node left/right bus
//! gains; level, pan, mute, solo and stereo link live here (linear pan law,
//! -128 dB means off).

use idctl_protocol::codec::encode_db;
use idctl_protocol::params::{mixer, MIXES};
use idctl_protocol::ParamAddr;

pub const NODES: usize = 26;
pub const OFF_DB: f32 = -128.0;
const MIX_COUNT: usize = MIXES as usize;
/// The last mix feeds the reverb.
pub const REVERB_MIX: usize = MIX_COUNT - 1;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Send {
    pub level: f32,
    /// -1 hard left, +1 hard right.
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Default for Send {
    fn default() -> Self {
        Self {
            level: OFF_DB,
            pan: 0.0,
            mute: false,
            solo: false,
        }
    }
}

pub type Writes = Vec<(ParamAddr, Vec<u8>)>;

#[derive(Default)]
pub struct Mixer {
    sends: [[Send; MIX_COUNT]; NODES],
    /// Stereo link per pair of nodes (`2p`, `2p + 1`).
    linked: [bool; NODES / 2],
    /// Last gains read from the device, per node and mix: (left, right).
    device: [[(f32, f32); MIX_COUNT]; NODES],
    seen: [[u8; MIX_COUNT]; NODES],
}

pub enum Edit {
    Level(f32),
    Pan(f32),
    Mute,
    Solo,
}

impl Mixer {
    pub fn send(&self, node: usize, mix: usize) -> Send {
        self.sends[node][mix]
    }

    pub fn linked(&self, pair: usize) -> bool {
        self.linked[pair]
    }

    /// Apply a UI edit to `node` in `mix`; returns the gains to write.
    pub fn edit(&mut self, node: usize, mix: usize, edit: Edit) -> Writes {
        let partner = self.linked[node / 2].then_some(node ^ 1);
        let solo_changed = matches!(edit, Edit::Solo);
        let mute = !self.sends[node][mix].mute;
        let solo = !self.sends[node][mix].solo;
        for n in std::iter::once(node).chain(partner) {
            let s = &mut self.sends[n][mix];
            match edit {
                Edit::Level(db) => s.level = db,
                Edit::Pan(p) if partner.is_none() => s.pan = p.clamp(-1.0, 1.0),
                Edit::Pan(_) => {}
                Edit::Mute => s.mute = mute,
                Edit::Solo => s.solo = solo,
            }
        }
        if solo_changed {
            return (0..NODES).flat_map(|n| self.gains(n, mix)).collect();
        }
        std::iter::once(node)
            .chain(partner)
            .flat_map(|n| self.gains(n, mix))
            .collect()
    }

    /// Toggle the stereo link of a pair in every mix.
    pub fn toggle_link(&mut self, pair: usize) -> Writes {
        let on = !self.linked[pair];
        self.linked[pair] = on;
        let (l, r) = (2 * pair, 2 * pair + 1);
        let mut out = Writes::new();
        for mix in 0..MIX_COUNT {
            let (pl, pr) = if on { (-1.0, 1.0) } else { (0.0, 0.0) };
            let lead = self.sends[l][mix];
            self.sends[l][mix].pan = pl;
            self.sends[r][mix] = Send { pan: pr, ..lead };
            out.extend(self.gains(l, mix));
            out.extend(self.gains(r, mix));
        }
        out
    }

    /// The node's left/right bus gains as the device must see them.
    fn gains(&self, node: usize, mix: usize) -> Writes {
        let s = self.sends[node][mix];
        let any_solo = (0..NODES).any(|n| self.sends[n][mix].solo);
        let silent = s.mute || (any_solo && !s.solo);
        let (l, r) = if silent {
            (OFF_DB, OFF_DB)
        } else {
            split(s.level, s.pan)
        };
        let n = node as u8;
        let m = mix as u8;
        vec![
            (mixer::gain_left(n, m), encode_db(l).to_vec()),
            (mixer::gain_right(n, m), encode_db(r).to_vec()),
        ]
    }

    /// A gain was read from the device. Once both sides of a node are known,
    /// recover its level and pan unless the host is holding it silent.
    pub fn device_gain(&mut self, addr: ParamAddr, db: f32) {
        let node = usize::from(addr.inst);
        let Some(bus) = addr.param.checked_sub(1) else {
            return;
        };
        let mix = usize::from(bus / 2);
        if node >= NODES || mix >= MIX_COUNT {
            return;
        }
        let g = &mut self.device[node][mix];
        if bus % 2 == 0 {
            g.0 = db
        } else {
            g.1 = db
        }
        self.seen[node][mix] |= 1 << (bus % 2);
        if self.seen[node][mix] != 0b11 {
            return;
        }
        if self.sends[node][mix].mute || (0..NODES).any(|n| self.sends[n][mix].solo) {
            return;
        }
        let (level, pan) = join(self.device[node][mix].0, self.device[node][mix].1);
        let s = &mut self.sends[node][mix];
        s.level = level;
        if let Some(p) = pan {
            s.pan = p;
        }
    }

    /// After the first full read, mark pairs whose nodes sit hard left/right
    /// at equal level in the main mix as linked.
    pub fn detect_links(&mut self) {
        for pair in 0..NODES / 2 {
            let (l, r) = (self.sends[2 * pair][0], self.sends[2 * pair + 1][0]);
            self.linked[pair] = l.pan <= -0.99 && r.pan >= 0.99 && (l.level - r.level).abs() < 0.01;
        }
    }
}

fn to_lin(db: f32) -> f32 {
    if db <= OFF_DB + 0.01 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

fn to_db(lin: f32) -> f32 {
    if lin <= 0.0 {
        OFF_DB
    } else {
        (20.0 * lin.log10()).max(OFF_DB)
    }
}

/// Level and pan to left/right bus gains (linear law: centre is -6 dB a side).
pub fn split(level: f32, pan: f32) -> (f32, f32) {
    if level <= OFF_DB {
        return (OFF_DB, OFF_DB);
    }
    let p = (pan + 1.0) / 2.0;
    let side = |lin: f32| {
        if lin <= 0.0 {
            OFF_DB
        } else {
            (level + 20.0 * lin.log10()).max(OFF_DB)
        }
    };
    (side(1.0 - p), side(p))
}

/// Inverse of [`split`]; pan is `None` when both sides are off.
pub fn join(left: f32, right: f32) -> (f32, Option<f32>) {
    let (a, b) = (to_lin(left), to_lin(right));
    let sum = a + b;
    if sum <= 0.0 {
        return (OFF_DB, None);
    }
    (to_db(sum), Some(2.0 * b / sum - 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use idctl_protocol::codec::decode_db;

    fn db_of(w: &Writes, addr: ParamAddr) -> f32 {
        decode_db(&w.iter().find(|(a, _)| *a == addr).unwrap().1).unwrap()
    }

    #[test]
    fn split_and_join_round_trip() {
        for (level, pan) in [(0.0, 0.0), (-12.0, -0.5), (6.0, 1.0), (-40.0, 0.3)] {
            let (l, r) = split(level, pan);
            let (lv, p) = join(l, r);
            assert!((lv - level).abs() < 0.01, "{level} {pan}");
            assert!((p.unwrap() - pan).abs() < 0.01, "{level} {pan}");
        }
        assert!((split(0.0, 0.0).0 + 6.02).abs() < 0.01);
    }

    #[test]
    fn solo_silences_every_other_node_in_that_mix_only() {
        let mut m = Mixer::default();
        m.edit(0, 0, Edit::Level(0.0));
        m.edit(3, 0, Edit::Level(0.0));
        let w = m.edit(3, 0, Edit::Solo);
        assert_eq!(db_of(&w, mixer::gain_left(0, 0)), OFF_DB);
        assert!(db_of(&w, mixer::gain_left(3, 0)) > -7.0);
        assert!(w.iter().all(|(a, _)| a.param <= 2), "only mix 0 rewritten");
    }

    #[test]
    fn linked_pair_moves_together_and_pans_hard() {
        let mut m = Mixer::default();
        m.toggle_link(5);
        let w = m.edit(10, 1, Edit::Level(-3.0));
        assert_eq!(db_of(&w, mixer::gain_left(10, 1)), -3.0);
        assert_eq!(db_of(&w, mixer::gain_right(10, 1)), OFF_DB);
        assert_eq!(db_of(&w, mixer::gain_right(11, 1)), -3.0);
        let w = m.edit(11, 1, Edit::Mute);
        assert!(w.iter().all(|(_, d)| decode_db(d) == Some(OFF_DB)));
        assert!(m.send(10, 1).mute && m.send(11, 1).mute);
    }

    #[test]
    fn device_reads_recover_level_and_pan() {
        let mut m = Mixer::default();
        let (l, r) = split(-10.0, 0.4);
        m.device_gain(mixer::gain_left(7, 2), l);
        m.device_gain(mixer::gain_right(7, 2), r);
        let s = m.send(7, 2);
        assert!((s.level + 10.0).abs() < 0.01 && (s.pan - 0.4).abs() < 0.01);
    }
}
