//! Talkback: while TB is on, the chosen mic is sent at unity, unmuted, to
//! the cue mixes, and put back as it was when TB goes off. The unit itself
//! only dims the monitors.

use crate::mix::{Edit, Mixer, Send, Writes};

/// Cue mixes that hear the talkback mic: Mix A and Mix B.
const CUE_MIXES: [usize; 2] = [1, 2];

pub struct Talkback {
    /// Mixer node of the talkback mic; `None` leaves talkback silent.
    pub source: Option<usize>,
    /// The mic's cue sends from before talking, restored afterwards.
    held: Option<(usize, [Send; 2])>,
}

impl Default for Talkback {
    fn default() -> Self {
        Self {
            source: Some(0),
            held: None,
        }
    }
}

impl Talkback {
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
            Edit::Pan(_) | Edit::Solo => {}
        }
    }

    /// Follow the TB state; returns the gains to write when it changes.
    pub fn follow(&mut self, mixer: &mut Mixer, on: bool) -> Writes {
        let mut out = Writes::new();
        match (on, self.held, self.source) {
            (true, None, Some(node)) => {
                self.held = Some((node, CUE_MIXES.map(|m| mixer.send(node, m))));
                for mix in CUE_MIXES {
                    out.extend(mixer.edit(node, mix, Edit::Level(0.0)));
                    if mixer.send(node, mix).mute {
                        out.extend(mixer.edit(node, mix, Edit::Mute));
                    }
                }
            }
            (false, Some((node, saved)), _) => {
                self.held = None;
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
}
