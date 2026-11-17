//! Latency page state: follows `pw-top` while the page is open and keeps the
//! split of xruns and load between the interface and the applications.

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::futures::{SinkExt, Stream};
use iced::{Subscription, Task};

use idctl_audio::graph::{self, Interface};
use idctl_audio::pw;
use idctl_audio::stats::{Monitor, Snapshot};
use idctl_audio::top::{Frames, Row};
use idctl_audio::tuning::{self, Found, Mode, Modes, Tuning};
use idctl_usb::{PRODUCT_ID_ID2_PLUS, VENDOR_ID};

/// How long to wait before starting `pw-top` again after it stops.
const RETRY: Duration = Duration::from_secs(2);

/// What the `pw-top` subscription reports.
#[derive(Debug, Clone)]
pub enum Feed {
    Line(String),
    /// `pw-top` could not start or has exited.
    Down,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The page was opened.
    Opened,
    Feed(Feed),
    Dumped(Result<String, String>),
    ResetCounts,
    /// A mode key, or Off: hand the buffer back to the applications.
    Mode(Option<Mode>),
    /// New values for a mode.
    Edit(Mode, Tuning),
    /// What was in force before the first change, and the change waiting on it.
    Found(Result<Found, String>, Tuning),
    Applied(Result<(), String>),
    Restored(Result<(), String>),
}

pub struct State {
    /// The latest iteration; none until one has arrived.
    pub snapshot: Option<Snapshot>,
    /// The iD2+ as PipeWire sees it; none if PipeWire does not list it.
    pub interface: Option<Interface>,
    /// PipeWire's tools are not answering.
    pub down: bool,
    /// Unix seconds of the latest iteration.
    pub now: u64,
    frames: Frames,
    monitor: Monitor,
    /// ALSA nodes in the latest iteration; a change means the interface's
    /// nodes may have been recreated under new ids or names.
    alsa_nodes: Vec<u32>,
    dump_pending: bool,
    pub modes: Modes,
    /// The mode in force; none while applications choose their own buffer.
    pub mode: Option<Mode>,
    /// A change is on its way to PipeWire.
    pub busy: bool,
    /// Why the last change failed.
    pub error: Option<String>,
    /// What to put back when the mode goes off or the app closes.
    found: Option<Found>,
    /// Where `found` is kept in case the app dies while forcing.
    record: Option<PathBuf>,
}

impl Default for State {
    fn default() -> Self {
        Self::with_record(crate::state::path("latency"))
    }
}

/// The record a crashed run left behind, removed as it is read. A record
/// that does not decode is dropped: it cannot be acted on.
pub fn leftover(record: &Path) -> Option<Found> {
    let text = std::fs::read_to_string(record).ok()?;
    let _ = std::fs::remove_file(record);
    tuning::decode(&text)
}

/// Put back what a crashed run left forced. Call once at start-up.
pub fn recover() {
    let found = crate::state::path("latency").and_then(|p| leftover(&p));
    if let Some(found) = found {
        pw::restore_blocking(found, VENDOR_ID, PRODUCT_ID_ID2_PLUS);
    }
}

impl State {
    pub fn with_record(record: Option<PathBuf>) -> Self {
        Self {
            snapshot: None,
            interface: None,
            down: false,
            now: 0,
            frames: Frames::default(),
            monitor: Monitor::new(Vec::new()),
            alsa_nodes: Vec::new(),
            dump_pending: false,
            modes: Modes::default(),
            mode: None,
            busy: false,
            error: None,
            found: None,
            record,
        }
    }

    /// What must be put back before the app closes, if anything.
    pub fn take_found(&mut self) -> Option<Found> {
        let found = self.found.take();
        if found.is_some() {
            self.forget_record();
        }
        found
    }

    fn forget_record(&self) {
        if let Some(path) = &self.record {
            let _ = std::fs::remove_file(path);
        }
    }

    fn apply(&mut self, tuning: Tuning) -> Task<Msg> {
        self.busy = true;
        self.error = None;
        if self.found.is_none() {
            return Task::perform(pw::find(VENDOR_ID, PRODUCT_ID_ID2_PLUS), move |found| {
                Msg::Found(found, tuning)
            });
        }
        Task::perform(
            pw::apply(tuning, VENDOR_ID, PRODUCT_ID_ID2_PLUS),
            Msg::Applied,
        )
    }

    pub fn update(&mut self, msg: Msg, now: u64) -> Task<Msg> {
        match msg {
            Msg::Opened => {
                self.frames = Frames::default();
                self.monitor.reset();
                self.snapshot = None;
                self.down = false;
                return self.dump();
            }
            Msg::Feed(Feed::Line(line)) => {
                self.down = false;
                if let Some(rows) = self.frames.push(&line) {
                    self.now = now;
                    self.snapshot = Some(self.monitor.frame(&rows, now));
                    return self.follow_nodes(&rows);
                }
            }
            Msg::Feed(Feed::Down) => {
                self.down = true;
                self.frames = Frames::default();
                self.snapshot = None;
            }
            Msg::Dumped(dump) => {
                self.dump_pending = false;
                self.interface = dump
                    .and_then(|d| graph::interface(&d, VENDOR_ID, PRODUCT_ID_ID2_PLUS))
                    .ok()
                    .flatten();
                let names = self.interface.iter().flat_map(|i| &i.nodes);
                self.monitor
                    .set_interface(names.map(|n| n.name.clone()).collect());
            }
            Msg::ResetCounts => {
                self.monitor.reset();
                self.snapshot = None;
            }
            Msg::Mode(Some(mode)) => {
                self.mode = Some(mode);
                return self.apply(self.modes.get(mode));
            }
            Msg::Mode(None) => {
                self.mode = None;
                if let Some(found) = self.found.clone() {
                    self.busy = true;
                    self.error = None;
                    return Task::perform(
                        pw::restore(found, VENDOR_ID, PRODUCT_ID_ID2_PLUS),
                        Msg::Restored,
                    );
                }
            }
            Msg::Edit(mode, tuning) => match tuning::validate(&tuning) {
                Err(why) => self.error = Some(why),
                Ok(()) => {
                    self.error = None;
                    self.modes.set(mode, tuning);
                    if self.mode == Some(mode) {
                        return self.apply(tuning);
                    }
                }
            },
            Msg::Found(Ok(found), tuning) => {
                if let Some(path) = &self.record {
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(path, tuning::encode(&found));
                }
                self.found = Some(found);
                return self.apply(tuning);
            }
            Msg::Found(Err(why), _) => {
                self.busy = false;
                self.mode = None;
                self.error = Some(why);
            }
            Msg::Applied(result) => {
                self.busy = false;
                self.error = result.err();
                return self.dump();
            }
            Msg::Restored(Ok(())) => {
                self.busy = false;
                self.found = None;
                self.forget_record();
                return self.dump();
            }
            Msg::Restored(Err(why)) => {
                self.busy = false;
                self.error = Some(why);
            }
        }
        Task::none()
    }

    /// Xruns per second, oldest first.
    pub fn history(&self) -> impl Iterator<Item = u32> + '_ {
        self.monitor.history()
    }

    fn dump(&mut self) -> Task<Msg> {
        if self.dump_pending {
            return Task::none();
        }
        self.dump_pending = true;
        Task::perform(pw::dump(), Msg::Dumped)
    }

    /// Ask PipeWire again when the set of ALSA nodes changes.
    fn follow_nodes(&mut self, rows: &[Row]) -> Task<Msg> {
        let alsa: Vec<u32> = rows
            .iter()
            .filter(|r| r.name.starts_with("alsa_"))
            .map(|r| r.id)
            .collect();
        if alsa == self.alsa_nodes {
            return Task::none();
        }
        self.alsa_nodes = alsa;
        self.dump()
    }
}

/// The next buffer size up or down: powers of two inside the allowed range.
pub fn step_quantum(frames: u32, up: bool) -> u32 {
    let (low, high) = tuning::QUANTUM;
    if up { frames * 2 } else { frames / 2 }.clamp(low, high)
}

/// The next period up or down. Unset starts from 64.
pub fn step_period(frames: Option<u32>, up: bool) -> u32 {
    let (low, high) = tuning::PERIOD;
    match frames {
        None => 64,
        Some(f) => if up { f * 2 } else { f / 2 }.clamp(low, high),
    }
}

/// The next headroom up or down: 0, then 16 doubling to the limit. Unset
/// starts from 32.
pub fn step_headroom(frames: Option<u32>, up: bool) -> u32 {
    match (frames, up) {
        (None, _) => 32,
        (Some(0), true) => 16,
        (Some(f), true) => (f * 2).min(tuning::HEADROOM_MAX),
        (Some(f), false) if f <= 16 => 0,
        (Some(f), false) => f / 2,
    }
}

/// Lines from `pw-top` for as long as the subscription is alive. If it
/// cannot start or stops, says so and tries again.
pub fn feed() -> Subscription<Feed> {
    Subscription::run(lines)
}

fn lines() -> impl Stream<Item = Feed> {
    iced::stream::channel(64, async |mut out| loop {
        if let Ok(mut top) = pw::Top::spawn() {
            while let Some(line) = top.line().await {
                if out.send(Feed::Line(line)).await.is_err() {
                    return;
                }
            }
        }
        if out.send(Feed::Down).await.is_err() {
            return;
        }
        tokio::time::sleep(RETRY).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use idctl_audio::stats::Driving;
    use idctl_audio::tuning;
    use std::path::{Path, PathBuf};

    const DRIVING: &str = include_str!("../../idctl-audio/fixtures/top-driving.txt");
    const PRO_AUDIO: &str = include_str!("../../idctl-audio/fixtures/dump-pro-audio.json");

    fn feed(state: &mut State, text: &str, now: u64) {
        for line in text.lines() {
            let _ = state.update(Msg::Feed(Feed::Line(line.into())), now);
        }
    }

    fn opened() -> State {
        let mut state = State::default();
        let _ = state.update(Msg::Opened, 100);
        let _ = state.update(Msg::Dumped(Ok(PRO_AUDIO.into())), 100);
        state
    }

    #[test]
    fn lines_become_a_snapshot() {
        let mut state = opened();
        assert_eq!(state.interface.as_ref().unwrap().nodes.len(), 2);
        feed(&mut state, DRIVING, 101);
        let snap = state.snapshot.as_ref().unwrap();
        assert_eq!(snap.driving, Driving::Interface);
        assert_eq!(snap.quantum, 64);
        assert_eq!(snap.interface.xruns, 0);
        // 3 when first seen is the baseline; 5 in the next iteration is 2 new.
        assert_eq!(snap.apps[0].name, "My DAW");
        assert_eq!(snap.apps[0].xruns, 2);
        assert_eq!(state.now, 101);
    }

    #[test]
    fn down_then_line_recovers() {
        let mut state = opened();
        let half: String = DRIVING.lines().take(9).collect::<Vec<_>>().join("\n");
        feed(&mut state, &half, 101);
        let _ = state.update(Msg::Feed(Feed::Down), 102);
        assert!(state.down);
        assert!(state.snapshot.is_none());
        feed(&mut state, DRIVING, 103);
        assert!(!state.down);
        assert_eq!(state.snapshot.as_ref().unwrap().apps[0].xruns, 2);
    }

    #[test]
    fn reset_clears_counts() {
        let mut state = opened();
        feed(&mut state, DRIVING, 101);
        let _ = state.update(Msg::ResetCounts, 102);
        assert!(state.snapshot.is_none());
        assert_eq!(state.history().count(), 0);
        feed(&mut state, DRIVING, 103);
        assert_eq!(state.snapshot.as_ref().unwrap().apps[0].xruns, 2);
    }

    #[test]
    fn reopening_counts_from_now() {
        let mut state = opened();
        feed(&mut state, DRIVING, 101);
        let _ = state.update(Msg::Opened, 200);
        assert!(state.snapshot.is_none());
        assert!(state.interface.is_some());
    }

    #[test]
    fn a_failed_dump_forgets_the_interface() {
        let mut state = opened();
        let _ = state.update(Msg::Dumped(Err("pw-dump: gone".into())), 101);
        assert!(state.interface.is_none());
        feed(&mut state, DRIVING, 102);
        assert_eq!(state.snapshot.as_ref().unwrap().driving, Driving::Idle);
    }

    fn record() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("id-mixer-latency-test-{}-{n}", std::process::id()))
    }

    fn found() -> Found {
        Found {
            force_quantum: 0,
            nodes: vec![("alsa_output.usb-X.pro-output-0".into(), 32, 32)],
        }
    }

    fn forcing(path: &Path) -> State {
        let mut state = State::with_record(Some(path.to_owned()));
        let _ = state.update(Msg::Mode(Some(Mode::Balanced)), 100);
        let tuning = state.modes.get(Mode::Balanced);
        let _ = state.update(Msg::Found(Ok(found()), tuning), 100);
        let _ = state.update(Msg::Applied(Ok(())), 100);
        state
    }

    #[test]
    fn first_apply_records_what_was_found() {
        let path = record();
        let state = forcing(&path);
        assert_eq!(state.mode, Some(Mode::Balanced));
        assert!(!state.busy);
        assert_eq!(state.error, None);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            tuning::encode(&found())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn second_apply_keeps_the_first_record() {
        let path = record();
        let mut state = forcing(&path);
        let _ = state.update(Msg::Mode(Some(Mode::Aggressive)), 101);
        let _ = state.update(Msg::Applied(Ok(())), 101);
        assert_eq!(state.mode, Some(Mode::Aggressive));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            tuning::encode(&found())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn off_clears_the_record() {
        let path = record();
        let mut state = forcing(&path);
        let _ = state.update(Msg::Mode(None), 101);
        assert_eq!(state.mode, None);
        assert!(path.exists(), "kept until the restore is confirmed");
        let _ = state.update(Msg::Restored(Ok(())), 102);
        assert!(!path.exists());
        assert!(state.take_found().is_none());
    }

    #[test]
    fn a_failed_restore_keeps_the_record() {
        let path = record();
        let mut state = forcing(&path);
        let _ = state.update(Msg::Mode(None), 101);
        let _ = state.update(Msg::Restored(Err("pw-metadata: no".into())), 102);
        assert!(path.exists());
        assert_eq!(state.error.as_deref(), Some("pw-metadata: no"));
        assert_eq!(state.take_found(), Some(found()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_failed_find_changes_nothing() {
        let path = record();
        let mut state = State::with_record(Some(path.clone()));
        let _ = state.update(Msg::Mode(Some(Mode::Safe)), 100);
        let tuning = state.modes.get(Mode::Safe);
        let _ = state.update(Msg::Found(Err("pw-dump: no".into()), tuning), 100);
        assert_eq!(state.mode, None);
        assert!(!state.busy);
        assert!(!path.exists());
        assert_eq!(state.error.as_deref(), Some("pw-dump: no"));
    }

    #[test]
    fn an_invalid_edit_changes_nothing() {
        let mut state = State::with_record(None);
        let before = state.modes.clone();
        let bad = Tuning {
            quantum: 48,
            ..before.balanced
        };
        let _ = state.update(Msg::Edit(Mode::Balanced, bad), 100);
        assert_eq!(state.modes, before);
        assert!(state.error.as_deref().unwrap().starts_with("Buffer"));
        assert!(!state.busy);
    }

    #[test]
    fn editing_the_mode_in_use_applies_it() {
        let path = record();
        let mut state = forcing(&path);
        let wider = Tuning {
            quantum: 256,
            ..state.modes.balanced
        };
        let _ = state.update(Msg::Edit(Mode::Balanced, wider), 101);
        assert_eq!(state.modes.balanced.quantum, 256);
        assert!(state.busy);
        let _ = state.update(Msg::Edit(Mode::Safe, wider), 102);
        assert_eq!(state.modes.safe.quantum, 256);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn steps_stay_inside_the_allowed_ranges() {
        assert_eq!((step_quantum(64, true), step_quantum(64, false)), (128, 32));
        assert_eq!(
            (step_quantum(8192, true), step_quantum(16, false)),
            (8192, 16)
        );
        assert_eq!(
            (step_period(None, true), step_period(Some(1024), true)),
            (64, 1024)
        );
        assert_eq!(step_period(Some(16), false), 16);
        assert_eq!(
            (step_headroom(Some(0), true), step_headroom(Some(16), false)),
            (16, 0)
        );
        assert_eq!(
            (step_headroom(Some(1024), true), step_headroom(None, false)),
            (1024, 32)
        );
        assert_eq!(step_headroom(Some(0), false), 0);
    }

    #[test]
    fn a_leftover_record_is_taken_once() {
        let path = record();
        std::fs::write(&path, tuning::encode(&found())).unwrap();
        assert_eq!(leftover(&path), Some(found()));
        assert!(!path.exists());
        assert_eq!(leftover(&path), None);
    }

    #[test]
    fn undecodable_record_is_deleted() {
        let path = record();
        std::fs::write(&path, "not a record").unwrap();
        assert_eq!(leftover(&path), None);
        assert!(!path.exists());
    }
}
