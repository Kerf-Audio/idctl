//! Output routing: which sample-bus source feeds each output slot.

use idctl_protocol::params::output;
use idctl_protocol::ParamAddr;

/// Source columns shared by every page, left to right.
pub const HEADERS: [&str; 5] = ["Main", "Alt", "Mix A", "Mix B", "Virtual thru"];

/// Columns under the MONITORING pill.
pub const MONITORING: std::ops::Range<usize> = 0..2;

/// Output slot of the first digital pair.
const DIGITAL_FIRST_SLOT: u8 = 6;

/// Digital output pairs (optical channels 1 to 8).
pub const DIGITAL_PAIRS: usize = 4;

const MAIN: u8 = 55;
const ALT: u8 = 57;
const MIX_A: u8 = 46;
const MIX_B: u8 = 48;
const PHONES_MAIN: u8 = 59;
const LOOPBACK_MAIN: u8 = 55;
const LOOPBACK_THRU: u8 = 10;

/// Sample-bus id of the first virtual output (DAW 1).
const VIRTUAL_FIRST: u8 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Analogue,
    Digital,
    Loopback,
}

impl Page {
    pub const ALL: [Self; 3] = [Self::Analogue, Self::Digital, Self::Loopback];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Analogue => "Analogue outputs",
            Self::Digital => "Digital outputs 1 - 8",
            Self::Loopback => "Loopback",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub page: Page,
    /// Per digital pair: the two slots pick their sources independently.
    pub mono: [bool; DIGITAL_PAIRS],
}

impl Default for State {
    fn default() -> Self {
        Self {
            page: Page::Analogue,
            mono: [false; DIGITAL_PAIRS],
        }
    }
}

impl State {
    /// Sets each digital pair's mode from the sources read back from the device.
    pub fn detect(&mut self, source: impl Fn(u8) -> u8) {
        for (pair, mono) in self.mono.iter_mut().enumerate() {
            let slot = DIGITAL_FIRST_SLOT + 2 * pair as u8;
            *mono = !is_stereo(source(slot), source(slot + 1));
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Page(Page),
    /// Digital pair index; `None` makes it mono, `Some(base)` stereo with
    /// both slots taken from the source pair starting at bus `base`.
    Mode(usize, Option<u8>),
    /// Route `source` (the column's left bus id) to `slot`. A stereo pick
    /// also routes the next slot to the right bus.
    Pick {
        slot: u8,
        source: u8,
        stereo: bool,
    },
}

/// A pair is stereo when its right slot follows the left source by one bus.
pub fn is_stereo(left: u8, right: u8) -> bool {
    right == left.wrapping_add(1)
}

/// One routable output on the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    /// What the VIRTUAL THRU column feeds.
    pub destination: String,
    /// Left slot of the pair, or the single slot of a mono row.
    pub slot: u8,
    pub stereo: bool,
    /// Digital pair whose MONO/STEREO toggle this row carries.
    pub toggle: Option<usize>,
    /// Left bus id of each source column; `None` where the output has no such source.
    pub cells: [Option<u8>; HEADERS.len()],
}

impl Row {
    /// Column whose bus pair contains the id read back from the device.
    pub fn selected(&self, source: u8) -> Option<usize> {
        self.cells
            .iter()
            .position(|cell| cell.is_some_and(|base| source == base || source == base + 1))
    }
}

/// Rows of `page`; `mono` selects one row or two for each digital pair.
pub fn rows(page: Page, mono: &[bool; DIGITAL_PAIRS]) -> Vec<Row> {
    match page {
        Page::Analogue => vec![
            line("Line out 1 + 2", 0, VIRTUAL_FIRST),
            line("Alt 3 + 4", 2, VIRTUAL_FIRST + 2),
            Row {
                name: "Headphones".into(),
                destination: virtual_outs(VIRTUAL_FIRST + 4),
                slot: 4,
                stereo: true,
                toggle: None,
                cells: [
                    Some(PHONES_MAIN),
                    None,
                    Some(MIX_A),
                    Some(MIX_B),
                    Some(VIRTUAL_FIRST + 4),
                ],
            },
        ],
        Page::Digital => (0..DIGITAL_PAIRS)
            .flat_map(|pair| digital(pair, mono[pair]))
            .collect(),
        Page::Loopback => vec![Row {
            name: "Loopback".into(),
            destination: "Loopback".into(),
            slot: DIGITAL_FIRST_SLOT + 2 * DIGITAL_PAIRS as u8,
            stereo: true,
            toggle: None,
            cells: [Some(LOOPBACK_MAIN), None, None, None, Some(LOOPBACK_THRU)],
        }],
    }
}

/// Writes for a pick: both buses of a stereo pair, or the one bus a mono
/// slot takes from its side of the pair.
fn pick(slot: u8, source: u8, stereo: bool) -> Vec<(ParamAddr, Vec<u8>)> {
    if stereo {
        vec![
            (output::source(slot), vec![source]),
            (output::source(slot + 1), vec![source + 1]),
        ]
    } else {
        vec![(output::source(slot), vec![source + (slot & 1)])]
    }
}

/// Applies a page message and returns the device writes it needs.
pub fn update(state: &mut State, msg: Msg) -> Vec<(ParamAddr, Vec<u8>)> {
    match msg {
        Msg::Page(page) => {
            state.page = page;
            Vec::new()
        }
        Msg::Mode(pair, stereo) => match (state.mono.get_mut(pair), stereo) {
            (Some(mono), None) => {
                *mono = true;
                Vec::new()
            }
            (Some(mono), Some(base)) if *mono => {
                *mono = false;
                pick(DIGITAL_FIRST_SLOT + 2 * pair as u8, base, true)
            }
            _ => Vec::new(),
        },
        Msg::Pick {
            slot,
            source,
            stereo,
        } => pick(slot, source, stereo),
    }
}

fn virtual_outs(thru: u8) -> String {
    let n = thru - VIRTUAL_FIRST + 1;
    format!("Virtual outs {n} + {}", n + 1)
}

/// A stereo output with every source column.
fn line(name: &str, slot: u8, thru: u8) -> Row {
    Row {
        name: name.into(),
        destination: virtual_outs(thru),
        slot,
        stereo: true,
        toggle: None,
        cells: [Some(MAIN), Some(ALT), Some(MIX_A), Some(MIX_B), Some(thru)],
    }
}

/// The row, or the two rows when mono, of one digital pair.
fn digital(pair: usize, mono: bool) -> Vec<Row> {
    let first = DIGITAL_FIRST_SLOT + 2 * pair as u8;
    let thru = VIRTUAL_FIRST + 6 + 2 * pair as u8;
    let n = 2 * pair as u8 + 1;
    if mono {
        (0..2)
            .map(|side| Row {
                name: format!("Digital {}", n + side),
                destination: format!("Virtual out {}", thru - VIRTUAL_FIRST + 1 + side),
                slot: first + side,
                stereo: false,
                toggle: (side == 0).then_some(pair),
                ..line("", first, thru)
            })
            .collect()
    } else {
        vec![Row {
            name: format!("Digital {n} + {}", n + 1),
            toggle: Some(pair),
            ..line("", first, thru)
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source read-back of all sixteen slots from a connected device.
    const READBACK: [u8; 16] = [
        55, 56, 57, 58, 59, 60, 18, 19, 20, 21, 22, 23, 24, 25, 10, 11,
    ];

    #[test]
    fn stereo_pick_writes_both_slots() {
        let writes = pick(6, 46, true);
        assert_eq!(
            writes,
            vec![(output::source(6), vec![46]), (output::source(7), vec![47])]
        );
    }

    #[test]
    fn mono_pick_writes_one_slot_with_its_side() {
        assert_eq!(pick(6, 46, false), vec![(output::source(6), vec![46])]);
        assert_eq!(pick(7, 46, false), vec![(output::source(7), vec![47])]);
    }

    #[test]
    fn update_pick_and_mode() {
        let mut state = State::default();
        let writes = update(
            &mut state,
            Msg::Pick {
                slot: 0,
                source: 57,
                stereo: true,
            },
        );
        assert_eq!(writes.len(), 2);
        assert!(update(&mut state, Msg::Mode(2, None)).is_empty());
        assert_eq!(state.mono, [false, false, true, false]);
        assert!(update(&mut state, Msg::Mode(9, None)).is_empty());
        // Back to stereo: the right slot follows the left source again.
        let slot = DIGITAL_FIRST_SLOT + 4;
        assert_eq!(
            update(&mut state, Msg::Mode(2, Some(MIX_A))),
            vec![
                (output::source(slot), vec![MIX_A]),
                (output::source(slot + 1), vec![MIX_A + 1]),
            ]
        );
        assert_eq!(state.mono, [false; 4]);
        assert!(update(&mut state, Msg::Mode(2, Some(MIX_A))).is_empty());
        assert!(update(&mut state, Msg::Page(Page::Digital)).is_empty());
        assert_eq!(state.page, Page::Digital);
    }

    #[test]
    fn column_from_readback() {
        let analogue = rows(Page::Analogue, &[false; 4]);
        assert_eq!(analogue[0].selected(READBACK[0]), Some(0));
        assert_eq!(analogue[1].selected(READBACK[2]), Some(1));
        assert_eq!(analogue[2].selected(READBACK[4]), Some(0));
        assert_eq!(analogue[0].selected(47), Some(2));
        assert_eq!(analogue[0].selected(49), Some(3));
        assert_eq!(analogue[0].selected(13), Some(4));
        assert_eq!(analogue[0].selected(100), None);

        let digital = rows(Page::Digital, &[false; 4]);
        assert_eq!(digital[0].selected(READBACK[6]), Some(4));
        assert_eq!(digital[3].selected(READBACK[12]), Some(4));

        let loopback = rows(Page::Loopback, &[false; 4]);
        assert_eq!(loopback[0].selected(READBACK[14]), Some(4));
    }

    #[test]
    fn stereo_detection() {
        assert!(is_stereo(18, 19));
        assert!(!is_stereo(18, 18));
        assert!(!is_stereo(18, 25));
        let mut state = State::default();
        state.detect(|slot| READBACK[usize::from(slot)]);
        assert_eq!(state.mono, [false; 4]);

        let mut mixed = READBACK;
        mixed[9] = 46;
        state.detect(|slot| mixed[usize::from(slot)]);
        assert_eq!(state.mono, [false, true, false, false]);
    }

    #[test]
    fn digital_rows_follow_mode() {
        assert_eq!(rows(Page::Digital, &[false; 4]).len(), 4);
        let mono = rows(Page::Digital, &[true, false, false, false]);
        assert_eq!(mono.len(), 5);
        assert_eq!((mono[0].slot, mono[1].slot, mono[2].slot), (6, 7, 8),);
        assert_eq!(mono[0].toggle, Some(0));
        assert_eq!(mono[1].toggle, None);
        assert!(!mono[1].stereo);
        assert_eq!(mono[1].destination, "Virtual out 8");
    }

    #[test]
    fn destinations() {
        let digital = rows(Page::Digital, &[false; 4]);
        assert_eq!(digital[0].destination, "Virtual outs 7 + 8");
        assert_eq!(digital[3].destination, "Virtual outs 13 + 14");
        let analogue = rows(Page::Analogue, &[false; 4]);
        assert_eq!(analogue[0].destination, "Virtual outs 1 + 2");
        assert_eq!(analogue[2].destination, "Virtual outs 5 + 6");
    }

    #[test]
    fn thru_ids_match_readback() {
        for pair in 0..DIGITAL_PAIRS {
            let row = &rows(Page::Digital, &[false; 4])[pair];
            let slot = usize::from(row.slot);
            assert_eq!(row.selected(READBACK[slot]), Some(4));
            assert_eq!(row.cells[4], Some(READBACK[slot]));
        }
    }
}
