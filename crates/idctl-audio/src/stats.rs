//! Turns `pw-top` iterations into what the page shows: who had xruns and
//! how much of each period was used, split between the interface and the
//! applications playing through it.

use std::collections::{HashMap, VecDeque};

use crate::top::Row;

/// Seconds of xrun history kept for the strip.
pub const HISTORY_SECS: usize = 300;

/// Who is clocking the graph the interface sits in.
#[derive(Clone, Debug, PartialEq)]
pub enum Driving {
    /// The interface is not streaming.
    Idle,
    /// The interface follows this other driver.
    Other(String),
    Interface,
}

/// The interface, or one application playing through it.
#[derive(Clone, Debug, PartialEq)]
pub struct Party {
    pub name: String,
    /// Xruns since the counts were last reset.
    pub xruns: u64,
    /// Unix seconds of the latest xrun.
    pub last_xrun: Option<u64>,
    /// Share of the period used in the latest iteration.
    pub load: f32,
    pub peak: f32,
    /// Only listens to an output: its lateness is never heard.
    pub listener: bool,
}

impl Party {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            xruns: 0,
            last_xrun: None,
            load: 0.0,
            peak: 0.0,
            listener: false,
        }
    }

    fn add(&mut self, xruns: u64, at: u64) {
        if xruns > 0 {
            self.xruns += xruns;
            self.last_xrun = Some(at);
        }
    }

    fn set_load(&mut self, load: f32) {
        self.load = load;
        self.peak = self.peak.max(load);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub driving: Driving,
    pub quantum: u32,
    pub rate: u32,
    pub interface: Party,
    pub apps: Vec<Party>,
}

pub struct Monitor {
    interface_nodes: Vec<String>,
    listeners: Vec<String>,
    /// Error counter of every node in the previous iteration: the baseline
    /// the next one is measured from.
    seen: HashMap<u32, u64>,
    interface: Party,
    apps: Vec<Party>,
    history: VecDeque<u32>,
}

impl Monitor {
    pub fn new(interface_nodes: Vec<String>) -> Self {
        Self {
            interface_nodes,
            listeners: Vec::new(),
            seen: HashMap::new(),
            interface: Party::new("Interface"),
            apps: Vec::new(),
            history: VecDeque::new(),
        }
    }

    pub fn set_interface(&mut self, names: Vec<String>) {
        self.interface_nodes = names;
    }

    /// Streams that only listen; their xruns are shown but not charted.
    pub fn set_listeners(&mut self, names: Vec<String>) {
        self.listeners = names;
    }

    pub fn reset(&mut self) {
        let listeners = std::mem::take(&mut self.listeners);
        *self = Self::new(std::mem::take(&mut self.interface_nodes));
        self.listeners = listeners;
    }

    /// Xruns per iteration (interface and apps together), oldest first.
    pub fn history(&self) -> impl Iterator<Item = u32> + '_ {
        self.history.iter().copied()
    }

    /// One `pw-top` iteration, stamped with unix seconds.
    pub fn frame(&mut self, rows: &[Row], at: u64) -> Snapshot {
        let mut seen = HashMap::with_capacity(rows.len());
        let mut driving = Driving::Idle;
        let (mut quantum, mut rate) = (0, 0);
        let mut interface_load = 0.0_f32;
        let mut present: Vec<&str> = Vec::new();
        let mut total = 0;
        let mut driver: Option<&Row> = None;

        for row in rows {
            // A node that was absent, or whose counter went backwards, has
            // been recreated: it starts again from where it is now.
            let new = match self.seen.get(&row.id) {
                Some(before) if row.errors >= *before => row.errors - before,
                _ => 0,
            };
            seen.insert(row.id, row.errors);
            if !row.follower {
                driver = Some(row);
            }
            let Some(driver) = driver.filter(|d| d.quantum > 0) else {
                continue;
            };
            let ours = self.is_interface(&driver.name);
            let load = row.load.unwrap_or(0.0);
            if self.is_interface(&row.name) {
                if ours {
                    driving = Driving::Interface;
                    (quantum, rate) = (driver.quantum, driver.rate);
                    interface_load = interface_load.max(load);
                    self.interface.add(new, at);
                    total += new;
                } else if driving == Driving::Idle {
                    driving = Driving::Other(driver.name.clone());
                }
            } else if ours && row.follower {
                let app = match self.apps.iter().position(|a| a.name == row.name) {
                    Some(i) => &mut self.apps[i],
                    None => {
                        self.apps.push(Party::new(&row.name));
                        self.apps.last_mut().expect("just pushed")
                    }
                };
                app.listener = self.listeners.contains(&row.name);
                app.add(new, at);
                app.set_load(load);
                present.push(&row.name);
                if !app.listener {
                    total += new;
                }
            }
        }

        self.seen = seen;
        self.interface.set_load(interface_load);
        self.apps
            .retain(|a| a.xruns > 0 || present.contains(&a.name.as_str()));
        for app in &mut self.apps {
            if !present.contains(&app.name.as_str()) {
                app.load = 0.0;
            }
        }
        if self.history.len() == HISTORY_SECS {
            self.history.pop_front();
        }
        self.history
            .push_back(u32::try_from(total).unwrap_or(u32::MAX));

        Snapshot {
            driving,
            quantum,
            rate,
            interface: self.interface.clone(),
            apps: self.apps.clone(),
        }
    }

    fn is_interface(&self, name: &str) -> bool {
        self.interface_nodes.iter().any(|n| n == name)
    }
}

/// Milliseconds added by buffering in one direction.
pub fn buffer_ms(quantum: u32, headroom: u32, rate: u32) -> f32 {
    if rate == 0 {
        return 0.0;
    }
    (quantum + headroom) as f32 * 1000.0 / rate as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "alsa_output.usb-X.pro-output-0";
    const IN: &str = "alsa_input.usb-X.pro-input-0";

    fn row(id: u32, name: &str, follower: bool, errors: u64, load: f32) -> Row {
        Row {
            id,
            quantum: 64,
            rate: 48_000,
            load: Some(load),
            errors,
            follower,
            name: name.into(),
        }
    }

    fn idle(id: u32, name: &str) -> Row {
        Row {
            quantum: 0,
            rate: 0,
            load: None,
            ..row(id, name, false, 0, 0.0)
        }
    }

    /// The interface driving, its input following, and one app.
    fn graph(out_err: u64, in_err: u64, daw_err: u64, daw_load: f32) -> Vec<Row> {
        vec![
            row(44, OUT, false, out_err, 0.18),
            row(46, IN, true, in_err, 0.02),
            row(150, "My DAW", true, daw_err, daw_load),
            idle(134, "hdmi"),
        ]
    }

    fn monitor() -> Monitor {
        Monitor::new(vec![OUT.into(), IN.into()])
    }

    #[test]
    fn interface_and_apps_are_counted_apart() {
        let mut m = monitor();
        m.frame(&graph(0, 0, 0, 0.5), 100);
        let s = m.frame(&graph(1, 2, 4, 0.5), 101);
        assert_eq!(s.driving, Driving::Interface);
        assert_eq!((s.quantum, s.rate), (64, 48_000));
        assert_eq!(s.interface.xruns, 3);
        assert_eq!(s.interface.last_xrun, Some(101));
        assert_eq!(s.apps.len(), 1);
        assert_eq!(s.apps[0].name, "My DAW");
        assert_eq!(s.apps[0].xruns, 4);
        assert_eq!(s.interface.load, 0.18);
    }

    #[test]
    fn baseline_hides_errors_from_before_the_page_opened() {
        let mut m = monitor();
        let s = m.frame(&graph(40, 0, 900, 0.5), 100);
        assert_eq!((s.interface.xruns, s.apps[0].xruns), (0, 0));
        assert_eq!(s.apps[0].last_xrun, None);
        let s = m.frame(&graph(41, 0, 900, 0.5), 101);
        assert_eq!((s.interface.xruns, s.apps[0].xruns), (1, 0));
    }

    #[test]
    fn a_returning_node_is_not_a_glitch() {
        let mut m = monitor();
        m.frame(&graph(0, 0, 0, 0.5), 100);
        m.frame(&graph(0, 0, 5, 0.5), 101);
        m.frame(&graph(0, 0, 0, 0.5)[..2], 102);
        let s = m.frame(&graph(0, 0, 0, 0.5), 103);
        assert_eq!(s.apps[0].xruns, 5);
        let s = m.frame(&graph(0, 0, 2, 0.5), 104);
        assert_eq!(s.apps[0].xruns, 7);
        // The counter going backwards on a live node is a new baseline too.
        let s = m.frame(&graph(0, 0, 1, 0.5), 105);
        assert_eq!(s.apps[0].xruns, 7);
    }

    #[test]
    fn idle_interface_reports_idle() {
        let mut m = monitor();
        let s = m.frame(&[idle(44, OUT), idle(134, "hdmi")], 100);
        assert_eq!(s.driving, Driving::Idle);
        assert!(s.apps.is_empty());
        assert_eq!(m.frame(&[], 101).driving, Driving::Idle);
    }

    #[test]
    fn another_driver_is_named() {
        let mut m = monitor();
        let rows = [
            row(134, "hdmi", false, 0, 0.1),
            row(44, OUT, true, 0, 0.1),
            row(150, "My DAW", true, 0, 0.5),
        ];
        m.frame(&rows, 100);
        let mut later = rows.clone();
        later[2].errors = 9;
        let s = m.frame(&later, 101);
        assert_eq!(s.driving, Driving::Other("hdmi".into()));
        assert!(s.apps.is_empty());
        assert_eq!(s.interface.xruns, 0);
    }

    #[test]
    fn apps_under_another_driver_are_not_ours() {
        let mut m = monitor();
        let rows = |e| {
            let mut v = graph(0, 0, 0, 0.5);
            v.push(row(134, "hdmi", false, 0, 0.1));
            v.push(row(160, "Browser", true, e, 0.1));
            v
        };
        m.frame(&rows(0), 100);
        let s = m.frame(&rows(6), 101);
        assert_eq!(s.apps.len(), 1);
        assert_eq!(s.apps[0].name, "My DAW");
    }

    #[test]
    fn peak_load_is_kept() {
        let mut m = monitor();
        m.frame(&graph(0, 0, 0, 0.4), 100);
        m.frame(&graph(0, 0, 0, 1.04), 101);
        let s = m.frame(&graph(0, 0, 0, 0.3), 102);
        assert_eq!((s.apps[0].load, s.apps[0].peak), (0.3, 1.04));
        assert_eq!(s.interface.peak, 0.18);
    }

    #[test]
    fn history_is_capped_at_300() {
        let mut m = monitor();
        for i in 0..400 {
            m.frame(&graph(0, 0, i, 0.5), i);
        }
        let h: Vec<u32> = m.history().collect();
        assert_eq!(h.len(), HISTORY_SECS);
        assert!(h.iter().all(|x| *x == 1));
    }

    #[test]
    fn buffer_ms_matches_the_arithmetic() {
        assert_eq!(buffer_ms(64, 32, 48_000), 2.0);
        assert_eq!(buffer_ms(64, 32, 0), 0.0);
    }

    #[test]
    fn reset_zeroes_counts_and_peaks() {
        let mut m = monitor();
        m.frame(&graph(0, 0, 0, 0.5), 100);
        m.frame(&graph(3, 0, 4, 1.2), 101);
        m.reset();
        assert_eq!(m.history().count(), 0);
        let s = m.frame(&graph(3, 0, 4, 0.5), 102);
        assert_eq!((s.interface.xruns, s.apps[0].xruns), (0, 0));
        assert_eq!(s.apps[0].peak, 0.5);
    }

    #[test]
    fn quiet_apps_that_left_are_forgotten_and_noisy_ones_kept() {
        let mut m = monitor();
        let with = |e| {
            let mut v = graph(0, 0, 0, 0.5);
            // Followers belong to the driver row above them.
            v.insert(3, row(170, "Ping", true, 0, 0.1));
            v.insert(4, row(171, "Glitchy", true, e, 0.1));
            v
        };
        m.frame(&with(0), 100);
        m.frame(&with(2), 101);
        let s = m.frame(&graph(0, 0, 0, 0.5), 102);
        let names: Vec<&str> = s.apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["My DAW", "Glitchy"]);
    }

    #[test]
    fn a_listener_is_marked_and_kept_out_of_the_history() {
        let mut m = monitor();
        m.set_listeners(vec!["Meter".into()]);
        let with = |e| {
            let mut v = graph(0, 0, 0, 0.5);
            v.insert(3, row(170, "Meter", true, e, 0.01));
            v
        };
        m.frame(&with(0), 100);
        let s = m.frame(&with(40), 101);
        let meter = s.apps.iter().find(|a| a.name == "Meter").unwrap();
        assert!(meter.listener);
        assert_eq!(meter.xruns, 40);
        assert!(!s.apps.iter().find(|a| a.name == "My DAW").unwrap().listener);
        assert_eq!(m.history().last(), Some(0));
    }
}
