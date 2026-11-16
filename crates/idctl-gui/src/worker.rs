//! Device thread: owns the USB handle, applies writes, mirrors device state.
//!
//! The UI never touches USB. It sends [`Command`]s and drains [`Event`]s.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use idctl_protocol::{params, ParamAddr};
use idctl_usb::Device2Plus;

use crate::watch;

const POLL: Duration = Duration::from_millis(40);
const RETRY: Duration = Duration::from_secs(1);
/// How long after a write its echo in the change list is ignored.
const ECHO: Duration = Duration::from_millis(500);

pub enum Command {
    Write(ParamAddr, Vec<u8>),
    /// Memory write (coefficient blob); never read back.
    Mem(ParamAddr, Vec<u8>),
}

pub enum Event {
    Connected,
    Disconnected(String),
    Param(ParamAddr, Vec<u8>),
    /// The first full read after connecting is done.
    Loaded,
    Meters(u16, Vec<u16>),
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
                let err = serve(&dev, cmds, events).unwrap_err();
                let _ = events.send(Event::Disconnected(err.to_string()));
            }
            Err(e) => {
                let _ = events.send(Event::Disconnected(e.to_string()));
            }
        }
        // Writes queued while offline would be stale on reconnect.
        while cmds.try_recv().is_ok() {}
        thread::sleep(RETRY);
    }
}

fn serve(
    dev: &Device2Plus,
    cmds: &Receiver<Command>,
    events: &Sender<Event>,
) -> Result<(), idctl_usb::Error> {
    let watched = watch::all();
    for &(addr, len) in &watched {
        refresh(dev, addr, len, events)?;
    }
    let _ = events.send(Event::Loaded);
    let mut recent: HashMap<ParamAddr, Instant> = HashMap::new();
    loop {
        let started = Instant::now();

        // Sliders flood us; only the newest write per parameter matters.
        // Coefficient blobs share one address, so they are keyed by their
        // band and sample-rate header as well.
        let mut latest: HashMap<ParamAddr, Vec<u8>> = HashMap::new();
        let mut blobs: HashMap<(ParamAddr, [u8; 8]), Vec<u8>> = HashMap::new();
        while let Ok(cmd) = cmds.try_recv() {
            match cmd {
                Command::Write(addr, data) => {
                    latest.insert(addr, data);
                }
                Command::Mem(addr, data) => {
                    let mut head = [0; 8];
                    let n = data.len().min(8);
                    head[..n].copy_from_slice(&data[..n]);
                    blobs.insert((addr, head), data);
                }
            }
        }
        // The device flags our own writes as changes, sometimes a few passes
        // later. Reading them back mid-drag would pull the control to an
        // older value, so skip anything we wrote within the echo window.
        for (addr, data) in latest {
            dev.set(addr, &data)?;
            recent.insert(addr, started);
        }
        for ((addr, _), data) in blobs {
            dev.mem_write(addr, &data)?;
        }
        recent.retain(|_, at| started.duration_since(*at) < ECHO);

        for addr in dev.pending_changes()? {
            if recent.contains_key(&addr) {
                continue;
            }
            if let Some(&(_, len)) = watched.iter().find(|(a, _)| *a == addr) {
                refresh(dev, addr, len, events)?;
            }
        }
        refresh(dev, params::system::SAMPLE_RATE, 4, events)?;
        for (group, count) in [
            params::meters::SOURCES,
            params::meters::MIX_BUSES,
            params::meters::GAIN_REDUCTION,
        ] {
            let _ = events.send(Event::Meters(group, dev.meters(group, count)?));
        }

        thread::sleep(POLL.saturating_sub(started.elapsed()));
    }
}

fn refresh(
    dev: &Device2Plus,
    addr: ParamAddr,
    len: u16,
    events: &Sender<Event>,
) -> Result<(), idctl_usb::Error> {
    let data = dev.get(addr, len)?;
    let _ = events.send(Event::Param(addr, data));
    Ok(())
}
