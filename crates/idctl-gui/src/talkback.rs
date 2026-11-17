//! Talkback: while TB is on, the chosen mic is sent at unity, unmuted, to
//! the cue mixes, and put back as it was when TB goes off. The unit itself
//! only dims the monitors.

use std::path::PathBuf;

use crate::mix::{Edit, Mixer, Send, Writes, OFF_DB};

/// Cue mixes that hear the talkback mic: Mix A and Mix B.
const CUE_MIXES: [usize; 2] = [1, 2];

type Saved = (usize, [Send; 2]);

pub struct Talkback {
    /// Mixer node of the talkback mic; `None` leaves talkback silent.
    pub source: Option<usize>,
    /// The mic's cue sends from before talking, restored afterwards.
    held: Option<Saved>,
    /// `held` changed since it was last persisted.
    dirty: bool,
}

/// Where the held sends are kept while talking: the unit keeps the talkback
/// levels if we die before restoring, and would pass them off as the user's.
fn state_path() -> Option<PathBuf> {
    crate::state::path("talkback")
}

fn encode((node, saved): &Saved) -> String {
    let [a, b] = saved;
    format!(
        "{node} {} {} {} {}\n",
        a.level,
        u8::from(a.mute),
        b.level,
        u8::from(b.mute)
    )
}

fn decode(text: &str) -> Option<Saved> {
    let mut it = text.split_whitespace();
    let node = it.next()?.parse().ok()?;
    let mut send = || -> Option<Send> {
        let level: f32 = it.next()?.parse().ok()?;
        let mute = it.next()? == "1";
        Some(Send {
            level: level.max(OFF_DB),
            mute,
            ..Send::default()
        })
    };
    Some((node, [send()?, send()?]))
}

/// Persist the held sends, or forget them.
pub fn store(held: Option<&Saved>) {
    let Some(path) = state_path() else { return };
    match held {
        Some(h) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, encode(h));
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn load() -> Option<Saved> {
    decode(&std::fs::read_to_string(state_path()?).ok()?)
}

impl Default for Talkback {
    fn default() -> Self {
        Self {
            source: Some(0),
            held: None,
            dirty: false,
        }
    }
}

impl Talkback {
    /// The sends held by a talkback left over from an earlier run, as the
    /// state file has them. Call once the device has been read.
    pub fn resume(&mut self) {
        if self.held.is_none() {
            self.held = load();
        }
    }

    /// The held sends if they changed since the last call; `Some(None)` means
    /// they were released.
    pub fn take_change(&mut self) -> Option<Option<Saved>> {
        std::mem::take(&mut self.dirty).then_some(self.held)
    }

    /// A hand edit of the talkback mic's cue send while talking becomes what
    /// is restored afterwards, instead of being overwritten.
    pub fn adopt(&mut self, node: usize, mix: usize, edit: &Edit) {
        let Some((held, saved)) = &mut self.held else {
            return;
        };
        let Some(i) = CUE_MIXES.iter().position(|&m| m == mix) else {
            return;
        };
        if *held != node {
            return;
        }
        match edit {
            Edit::Level(db) => saved[i].level = *db,
            Edit::Mute => saved[i].mute = !saved[i].mute,
            Edit::Pan(_) | Edit::Solo => return,
        }
        self.dirty = true;
    }

    /// Follow the TB state; returns the gains to write when it changes.
    pub fn follow(&mut self, mixer: &mut Mixer, on: bool) -> Writes {
        let mut out = Writes::new();
        match (on, self.held, self.source) {
            (true, None, Some(node)) => {
                self.held = Some((node, CUE_MIXES.map(|m| mixer.send(node, m))));
                self.dirty = true;
                for mix in CUE_MIXES {
                    out.extend(mixer.edit(node, mix, Edit::Level(0.0)));
                    if mixer.send(node, mix).mute {
                        out.extend(mixer.edit(node, mix, Edit::Mute));
                    }
                }
            }
            (false, Some((node, saved)), _) => {
                self.held = None;
                self.dirty = true;
                for (mix, s) in CUE_MIXES.into_iter().zip(saved) {
                    out.extend(mixer.edit(node, mix, Edit::Level(s.level)));
                    if mixer.send(node, mix).mute != s.mute {
                        out.extend(mixer.edit(node, mix, Edit::Mute));
                    }
                }
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talking_opens_the_mic_in_cue_mixes_and_restores_it() {
        let mut mixer = Mixer::default();
        mixer.edit(0, 1, Edit::Level(-20.0));
        mixer.edit(0, 2, Edit::Mute);
        let mut tb = Talkback::default();
        assert!(!tb.follow(&mut mixer, true).is_empty());
        for mix in CUE_MIXES {
            let s = mixer.send(0, mix);
            assert!(s.level == 0.0 && !s.mute);
        }
        assert!(tb.follow(&mut mixer, true).is_empty(), "held, no rewrite");
        tb.follow(&mut mixer, false);
        assert_eq!(mixer.send(0, 1).level, -20.0);
        assert!(mixer.send(0, 2).mute);
    }

    #[test]
    fn edits_made_while_talking_survive_the_release() {
        let mut mixer = Mixer::default();
        mixer.edit(0, 1, Edit::Level(-20.0));
        let mut tb = Talkback::default();
        tb.follow(&mut mixer, true);
        tb.adopt(0, 1, &Edit::Level(-9.0));
        tb.adopt(0, 0, &Edit::Level(-1.0));
        tb.follow(&mut mixer, false);
        assert_eq!(mixer.send(0, 1).level, -9.0);
    }

    #[test]
    fn held_sends_survive_the_state_file_format() {
        let a = Send {
            level: -17.5,
            mute: true,
            ..Send::default()
        };
        let b = Send {
            level: -3.0,
            ..Send::default()
        };
        let (node, [ra, rb]) = decode(&encode(&(1, [a, b]))).unwrap();
        assert_eq!((node, ra.level, ra.mute), (1, -17.5, true));
        assert_eq!((rb.level, rb.mute), (-3.0, false));
        assert!(decode("garbage").is_none());
        assert!(decode("1 -3 0").is_none(), "truncated");
    }
}
