//! Latency page state: follows `pw-top` while the page is open and keeps the
//! split of xruns and load between the interface and the applications.

use std::time::Duration;

use iced::futures::{SinkExt, Stream};
use iced::{Subscription, Task};

use idctl_audio::graph::{self, Interface};
use idctl_audio::pw;
use idctl_audio::stats::{Monitor, Snapshot};
use idctl_audio::top::{Frames, Row};
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
}

impl Default for State {
    fn default() -> Self {
        Self {
            snapshot: None,
            interface: None,
            down: false,
            now: 0,
            frames: Frames::default(),
            monitor: Monitor::new(Vec::new()),
            alsa_nodes: Vec::new(),
            dump_pending: false,
        }
    }
}

impl State {
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
}
