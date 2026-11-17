//! Device thread: owns the USB handle, applies writes, mirrors device state.
//!
//! The UI never touches USB. It sends [`Command`]s and drains [`Event`]s.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use idctl_protocol::{params, ParamAddr};
use idctl_usb::{Device2Plus, Error};

use crate::watch;

const POLL: Duration = Duration::from_millis(40);
const RETRY: Duration = Duration::from_secs(1);
/// How long after a write its echo in the change list is ignored.
const ECHO: Duration = Duration::from_millis(500);
/// Passes in a row that may hit a transient USB error before the link counts
/// as lost and the app reconnects.
const MAX_STRIKES: u32 = 5;
/// Extra attempts to land the queue when quitting, and the wait between them.
const QUIT_RETRIES: u32 = 3;
const QUIT_RETRY_WAIT: Duration = Duration::from_millis(50);

pub enum Command {
    Write(ParamAddr, Vec<u8>),
    /// Memory write (coefficient blob); never read back.
    Mem(ParamAddr, Vec<u8>),
    /// Apply everything queued so far, confirm on the sender, stop the thread.
    /// The reply is true when nothing was left unwritten.
    Quit(Sender<bool>),
}

pub enum Event {
    Connected,
    Disconnected(String),
    Param(ParamAddr, Vec<u8>),
    /// The first full read after connecting is done.
    Loaded,
    Meters(u16, Vec<u16>),
}

/// The device operations the worker needs; [`Device2Plus`] in the app, a fake
/// in tests.
pub trait Transport {
    fn get(&self, addr: ParamAddr, len: u16) -> Result<Vec<u8>, Error>;
    fn set(&self, addr: ParamAddr, data: &[u8]) -> Result<(), Error>;
    fn mem_write(&self, addr: ParamAddr, data: &[u8]) -> Result<(), Error>;
    fn pending_changes(&self) -> Result<Vec<ParamAddr>, Error>;
    fn meters(&self, group: u16, count: u16) -> Result<Vec<u16>, Error>;
}

impl Transport for Device2Plus {
    fn get(&self, addr: ParamAddr, len: u16) -> Result<Vec<u8>, Error> {
        Device2Plus::get(self, addr, len)
    }
    fn set(&self, addr: ParamAddr, data: &[u8]) -> Result<(), Error> {
        Device2Plus::set(self, addr, data)
    }
    fn mem_write(&self, addr: ParamAddr, data: &[u8]) -> Result<(), Error> {
        Device2Plus::mem_write(self, addr, data)
    }
    fn pending_changes(&self) -> Result<Vec<ParamAddr>, Error> {
        Device2Plus::pending_changes(self)
    }
    fn meters(&self, group: u16, count: u16) -> Result<Vec<u16>, Error> {
        Device2Plus::meters(self, group, count)
    }
}

pub fn spawn() -> (Sender<Command>, Receiver<Event>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (ev_tx, ev_rx) = mpsc::channel();
    thread::spawn(move || run(&cmd_rx, &ev_tx));
    (cmd_tx, ev_rx)
}

fn run(cmds: &Receiver<Command>, events: &Sender<Event>) {
    loop {
        match Device2Plus::open() {
            Ok(dev) => {
                let _ = events.send(Event::Connected);
                match serve(&dev, cmds, events) {
                    Ok(()) => return,
                    Err(e) => {
                        let _ = events.send(Event::Disconnected(e.to_string()));
                    }
                }
            }
            Err(e) => {
                let _ = events.send(Event::Disconnected(e.to_string()));
            }
        }
        // Writes queued while offline would be stale on reconnect, so drop
        // them; but a quit request is still honoured.
        let deadline = Instant::now() + RETRY;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match cmds.recv_timeout(left) {
                Ok(Command::Quit(ack)) => {
                    let _ = ack.send(false);
                    return;
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// One queued write.
enum Item {
    Set(ParamAddr, Vec<u8>),
    /// Coefficient blob. Blobs share one address, so they are told apart by
    /// their band and sample-rate header.
    Mem(ParamAddr, Vec<u8>),
}

impl Item {
    fn key(&self) -> (ParamAddr, bool, [u8; 8]) {
        match self {
            Self::Set(addr, _) => (*addr, false, [0; 8]),
            Self::Mem(addr, data) => {
                let mut head = [0; 8];
                let n = data.len().min(8);
                head[..n].copy_from_slice(&data[..n]);
                (*addr, true, head)
            }
        }
    }
}

struct Session {
    watched: Vec<(ParamAddr, u16)>,
    lens: HashMap<ParamAddr, u16>,
    /// Writes not yet applied, oldest first.
    queue: Vec<Item>,
    /// Parameters whose read failed and must be read again.
    retry: Vec<ParamAddr>,
    recent: HashMap<ParamAddr, Instant>,
    /// Passes in a row with a transient error.
    strikes: u32,
    hit: bool,
}

impl Session {
    fn new() -> Self {
        let watched = watch::all();
        Self {
            lens: watched.iter().copied().collect(),
            watched,
            queue: Vec::new(),
            retry: Vec::new(),
            recent: HashMap::new(),
            strikes: 0,
            hit: false,
        }
    }

    /// Queue a write. Sliders flood us; only the newest write per parameter
    /// matters, and it takes the newest position so that writes sent as a
    /// sequence stay in sequence.
    fn push(&mut self, item: Item) {
        let key = item.key();
        self.queue.retain(|i| i.key() != key);
        self.queue.push(item);
    }

    /// Absorb a transient USB error (the device is still there) for a few
    /// passes; anything else, or too many in a row, ends the session.
    fn soft<T>(&mut self, r: Result<T, Error>) -> Result<Option<T>, Error> {
        match r {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.is_transient() && self.strikes < MAX_STRIKES => {
                self.hit = true;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

/// Runs until the link is lost (`Err`) or the UI asks to quit (`Ok`).
fn serve<D: Transport>(
    dev: &D,
    cmds: &Receiver<Command>,
    events: &Sender<Event>,
) -> Result<(), Error> {
    let mut session = Session::new();
    for &(addr, len) in &session.watched {
        let data = dev.get(addr, len)?;
        let _ = events.send(Event::Param(addr, data));
    }
    let _ = events.send(Event::Loaded);
    loop {
        let started = Instant::now();
        if let Some(ack) = pass(dev, &mut session, cmds, events)? {
            let _ = ack.send(session.queue.is_empty());
            return Ok(());
        }
        thread::sleep(POLL.saturating_sub(started.elapsed()));
    }
}

/// One poll cycle: apply queued writes, read what changed, read the meters.
/// Returns the confirmation channel when a quit was requested.
fn pass<D: Transport>(
    dev: &D,
    s: &mut Session,
    cmds: &Receiver<Command>,
    events: &Sender<Event>,
) -> Result<Option<Sender<bool>>, Error> {
    let started = Instant::now();
    s.hit = false;

    let mut quit = None;
    while let Ok(cmd) = cmds.try_recv() {
        match cmd {
            Command::Write(addr, data) => s.push(Item::Set(addr, data)),
            Command::Mem(addr, data) => s.push(Item::Mem(addr, data)),
            Command::Quit(ack) => quit = Some(ack),
        }
    }

    flush(dev, s, started)?;
    s.recent.retain(|_, at| started.duration_since(*at) < ECHO);
    if quit.is_some() {
        // The last chance to land the queue: a transient hiccup now would
        // otherwise lose writes such as the talkback restore.
        for _ in 0..QUIT_RETRIES {
            if s.queue.is_empty() {
                break;
            }
            thread::sleep(QUIT_RETRY_WAIT);
            flush(dev, s, Instant::now())?;
        }
        return Ok(quit);
    }

    let mut changed = std::mem::take(&mut s.retry);
    if let Some(list) = s.soft(dev.pending_changes())? {
        changed.extend(list);
    }
    for addr in changed {
        if s.recent.contains_key(&addr) {
            continue;
        }
        let Some(&len) = s.lens.get(&addr) else {
            continue;
        };
        match s.soft(dev.get(addr, len))? {
            Some(data) => {
                let _ = events.send(Event::Param(addr, data));
            }
            None => s.retry.push(addr),
        }
    }

    if let Some(data) = s.soft(dev.get(params::system::SAMPLE_RATE, 4))? {
        let _ = events.send(Event::Param(params::system::SAMPLE_RATE, data));
    }
    for (group, count) in [
        params::meters::SOURCES,
        params::meters::MIX_BUSES,
        params::meters::GAIN_REDUCTION,
    ] {
        if let Some(levels) = s.soft(dev.meters(group, count))? {
            let _ = events.send(Event::Meters(group, levels));
        }
    }

    s.strikes = if s.hit { s.strikes + 1 } else { 0 };
    Ok(None)
}

/// Apply the queue in order. The device flags our own writes as changes,
/// sometimes a few passes later, and reading them back mid-drag would pull the
/// control to an older value, so each write is stamped in `recent`. A transient
/// failure keeps that write and everything after it, in order.
fn flush<D: Transport>(dev: &D, s: &mut Session, at: Instant) -> Result<(), Error> {
    let mut items = std::mem::take(&mut s.queue).into_iter();
    while let Some(item) = items.next() {
        let sent = match &item {
            Item::Set(addr, data) => dev.set(*addr, data),
            Item::Mem(addr, data) => dev.mem_write(*addr, data),
        };
        if s.soft(sent)?.is_none() {
            s.queue.push(item);
            s.queue.extend(items);
            break;
        }
        if let Item::Set(addr, _) = item {
            s.recent.insert(addr, at);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use idctl_protocol::params::monitor;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Fake {
        /// Every set/mem_write/get that reached the device, in order.
        log: RefCell<Vec<String>>,
        /// Errors the next `set` calls fail with.
        set_errors: RefCell<VecDeque<Error>>,
        changes: RefCell<Vec<ParamAddr>>,
    }

    fn transient() -> Error {
        Error::Short { wanted: 1, got: 0 }
    }

    fn tag(addr: ParamAddr) -> String {
        format!("{}.{}.{}", addr.kind, addr.inst, addr.param)
    }

    impl Transport for Fake {
        fn get(&self, addr: ParamAddr, len: u16) -> Result<Vec<u8>, Error> {
            self.log.borrow_mut().push(format!("get {}", tag(addr)));
            Ok(vec![0; len.into()])
        }
        fn set(&self, addr: ParamAddr, data: &[u8]) -> Result<(), Error> {
            if let Some(e) = self.set_errors.borrow_mut().pop_front() {
                return Err(e);
            }
            self.log
                .borrow_mut()
                .push(format!("set {}={:?}", tag(addr), data));
            Ok(())
        }
        fn mem_write(&self, addr: ParamAddr, _: &[u8]) -> Result<(), Error> {
            self.log.borrow_mut().push(format!("mem {}", tag(addr)));
            Ok(())
        }
        fn pending_changes(&self) -> Result<Vec<ParamAddr>, Error> {
            Ok(std::mem::take(&mut *self.changes.borrow_mut()))
        }
        fn meters(&self, _: u16, count: u16) -> Result<Vec<u16>, Error> {
            Ok(vec![0; count.into()])
        }
    }

    fn rig() -> (
        Fake,
        Session,
        Sender<Command>,
        Receiver<Command>,
        Sender<Event>,
    ) {
        let (tx, rx) = mpsc::channel();
        let (ev, _keep) = mpsc::channel();
        (Fake::default(), Session::new(), tx, rx, ev)
    }

    fn sets(dev: &Fake) -> Vec<String> {
        dev.log
            .borrow()
            .iter()
            .filter(|l| l.starts_with("set") || l.starts_with("mem"))
            .cloned()
            .collect()
    }

    const A: ParamAddr = monitor::MUTE;
    const B: ParamAddr = monitor::DIM;

    #[test]
    fn newest_write_per_parameter_wins_and_sequence_is_kept() {
        let (dev, mut s, tx, rx, ev) = rig();
        for (addr, v) in [(A, 1), (B, 1), (A, 2)] {
            tx.send(Command::Write(addr, vec![v])).unwrap();
        }
        pass(&dev, &mut s, &rx, &ev).unwrap();
        assert_eq!(
            sets(&dev),
            [format!("set {}=[1]", tag(B)), format!("set {}=[2]", tag(A))]
        );
    }

    #[test]
    fn a_transient_error_keeps_the_write_for_the_next_pass() {
        let (dev, mut s, tx, rx, ev) = rig();
        dev.set_errors.borrow_mut().push_back(transient());
        tx.send(Command::Write(A, vec![1])).unwrap();
        tx.send(Command::Write(B, vec![1])).unwrap();
        pass(&dev, &mut s, &rx, &ev).unwrap();
        assert!(sets(&dev).is_empty());
        pass(&dev, &mut s, &rx, &ev).unwrap();
        assert_eq!(
            sets(&dev),
            [format!("set {}=[1]", tag(A)), format!("set {}=[1]", tag(B))],
            "retried in the original order"
        );
    }

    #[test]
    fn a_device_that_keeps_failing_ends_the_session() {
        let (dev, mut s, tx, rx, ev) = rig();
        for _ in 0..=MAX_STRIKES {
            dev.set_errors.borrow_mut().push_back(transient());
        }
        tx.send(Command::Write(A, vec![1])).unwrap();
        for _ in 0..MAX_STRIKES {
            assert!(pass(&dev, &mut s, &rx, &ev).is_ok());
        }
        assert!(pass(&dev, &mut s, &rx, &ev).is_err());
    }

    #[test]
    fn a_fatal_error_ends_the_session_at_once() {
        let (dev, mut s, tx, rx, ev) = rig();
        dev.set_errors.borrow_mut().push_back(Error::NotFound);
        tx.send(Command::Write(A, vec![1])).unwrap();
        assert!(pass(&dev, &mut s, &rx, &ev).is_err());
    }

    #[test]
    fn our_own_writes_are_not_read_back() {
        let (dev, mut s, tx, rx, ev) = rig();
        tx.send(Command::Write(A, vec![1])).unwrap();
        *dev.changes.borrow_mut() = vec![A];
        pass(&dev, &mut s, &rx, &ev).unwrap();
        assert!(!dev.log.borrow().contains(&format!("get {}", tag(A))));

        *dev.changes.borrow_mut() = vec![B];
        pass(&dev, &mut s, &rx, &ev).unwrap();
        assert!(dev.log.borrow().contains(&format!("get {}", tag(B))));
    }

    #[test]
    fn quit_applies_queued_writes_first() {
        let (dev, mut s, tx, rx, ev) = rig();
        let (ack_tx, ack_rx) = mpsc::channel();
        tx.send(Command::Write(A, vec![1])).unwrap();
        tx.send(Command::Quit(ack_tx)).unwrap();
        let ack = pass(&dev, &mut s, &rx, &ev).unwrap().expect("quit");
        assert_eq!(sets(&dev), [format!("set {}=[1]", tag(A))]);
        ack.send(s.queue.is_empty()).unwrap();
        assert_eq!(ack_rx.try_recv(), Ok(true));
    }

    #[test]
    fn quit_retries_a_write_that_hit_a_hiccup() {
        let (dev, mut s, tx, rx, ev) = rig();
        let (ack_tx, _ack_rx) = mpsc::channel();
        dev.set_errors.borrow_mut().push_back(transient());
        tx.send(Command::Write(A, vec![1])).unwrap();
        tx.send(Command::Quit(ack_tx)).unwrap();
        pass(&dev, &mut s, &rx, &ev).unwrap().expect("quit");
        assert_eq!(sets(&dev), [format!("set {}=[1]", tag(A))]);
        assert!(s.queue.is_empty());
    }
}
