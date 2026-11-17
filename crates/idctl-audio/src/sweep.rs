//! The test run: step down through the buffer sizes, hold each one for a
//! fixed time and count xruns. Pure: the caller applies each tuning, feeds
//! one snapshot per second and does what the returned action says.

use crate::stats::{Driving, Snapshot};
use crate::tuning::{Presets, Tuning, BUFFERS};

/// Seconds discarded after each change: the stream reopens and PipeWire
/// reports start-up errors that say nothing about the setting.
pub const SETTLE_SECS: u32 = 2;
/// A run stops this many steps after the first glitch.
pub const STOP_AFTER_GLITCH: usize = 2;
/// A clean step is only suggested while its peak load stays under this.
pub const SUGGEST_LOAD: f32 = 0.80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    Glitches,
}

/// What one buffer size did over its hold.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub tuning: Tuning,
    pub interface_xruns: u64,
    /// Xruns of the applications, listening-only streams left out.
    pub app_xruns: u64,
    pub avg_load: f32,
    pub peak_load: f32,
    pub verdict: Verdict,
}

/// The tunings to try, largest buffer first. `buffer_only` leaves the
/// interface's period and headroom as found.
pub fn ladder(presets: &Presets, buffer_only: bool) -> Vec<Tuning> {
    BUFFERS
        .iter()
        .rev()
        .filter_map(|frames| presets.get(*frames))
        .map(|t| {
            if buffer_only {
                Tuning {
                    period: None,
                    headroom: None,
                    ..t
                }
            } else {
                t
            }
        })
        .collect()
}

/// What the caller does next.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Apply this tuning, then call [`Sweep::applied`].
    Apply(Tuning),
    Wait,
    Done,
    Abort(String),
}

enum Phase {
    Idle,
    /// Waiting for the caller to say the change has been made.
    Applying,
    Settling {
        left: u32,
    },
    Holding {
        left: u32,
        base: (u64, u64),
        last: (u64, u64),
        load_sum: f32,
        load_peak: f32,
        taken: u32,
    },
    Finished(Action),
}

pub struct Sweep {
    tunings: Vec<Tuning>,
    hold: u32,
    /// Index of the tuning being run, or next to run.
    at: usize,
    phase: Phase,
    steps: Vec<Step>,
    first_glitch: Option<usize>,
}

impl Sweep {
    pub fn new(tunings: Vec<Tuning>, hold_secs: u32) -> Self {
        Self {
            tunings,
            hold: hold_secs.max(1),
            at: 0,
            phase: Phase::Idle,
            steps: Vec::new(),
            first_glitch: None,
        }
    }

    pub fn start(&mut self) -> Action {
        match self.tunings.first() {
            None => self.finish(Action::Done),
            Some(first) => {
                self.phase = Phase::Applying;
                Action::Apply(*first)
            }
        }
    }

    /// The change asked for by [`Action::Apply`] has been made, or failed.
    pub fn applied(&mut self, result: Result<(), String>) -> Action {
        match (&self.phase, result) {
            (Phase::Finished(done), _) => done.clone(),
            (Phase::Applying, Ok(())) => {
                self.phase = Phase::Settling { left: SETTLE_SECS };
                Action::Wait
            }
            (Phase::Applying, Err(why)) => self.finish(Action::Abort(why)),
            _ => Action::Wait,
        }
    }

    /// One snapshot, once a second.
    pub fn sample(&mut self, snap: &Snapshot) -> Action {
        match &self.phase {
            Phase::Finished(done) => return done.clone(),
            Phase::Idle | Phase::Applying => return Action::Wait,
            _ => {}
        }
        let settling = matches!(self.phase, Phase::Settling { .. });
        if snap.driving != Driving::Interface && !settling {
            return self.finish(Action::Abort(
                "The interface stopped driving the graph.".into(),
            ));
        }
        let apps = || snap.apps.iter().filter(|a| !a.listener);
        let counts = (snap.interface.xruns, apps().map(|a| a.xruns).sum());
        let load = apps().map(|a| a.load).fold(snap.interface.load, f32::max);
        match &mut self.phase {
            Phase::Settling { left } if *left > 1 => *left -= 1,
            Phase::Settling { .. } => {
                self.phase = Phase::Holding {
                    left: self.hold,
                    base: counts,
                    last: counts,
                    load_sum: 0.0,
                    load_peak: 0.0,
                    taken: 0,
                }
            }
            Phase::Holding {
                left,
                last,
                load_sum,
                load_peak,
                taken,
                ..
            } => {
                *left -= 1;
                *last = counts;
                *load_sum += load;
                *load_peak = load_peak.max(load);
                *taken += 1;
                if *left == 0 {
                    return self.complete_step();
                }
            }
            _ => {}
        }
        Action::Wait
    }

    fn complete_step(&mut self) -> Action {
        let Phase::Holding {
            base,
            last,
            load_sum,
            load_peak,
            taken,
            ..
        } = self.phase
        else {
            return Action::Wait;
        };
        // Counts restart from zero if the page's counters are reset mid-run.
        let (interface_xruns, app_xruns) =
            (last.0.saturating_sub(base.0), last.1.saturating_sub(base.1));
        let clean = interface_xruns == 0 && app_xruns == 0;
        self.steps.push(Step {
            tuning: self.tunings[self.at],
            interface_xruns,
            app_xruns,
            avg_load: load_sum / taken as f32,
            peak_load: load_peak,
            verdict: if clean {
                Verdict::Clean
            } else {
                Verdict::Glitches
            },
        });
        if !clean {
            self.first_glitch.get_or_insert(self.at);
        }
        let enough = self
            .first_glitch
            .is_some_and(|g| self.at >= g + STOP_AFTER_GLITCH);
        self.at += 1;
        match self.tunings.get(self.at) {
            Some(next) if !enough => {
                self.phase = Phase::Applying;
                Action::Apply(*next)
            }
            _ => self.finish(Action::Done),
        }
    }

    fn finish(&mut self, action: Action) -> Action {
        self.phase = Phase::Finished(action.clone());
        action
    }

    /// The steps finished so far.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The step in progress (from 1), how many there are, and the seconds
    /// left in it.
    pub fn progress(&self) -> (usize, usize, u32) {
        let total = self.tunings.len();
        let step = (self.at + 1).min(total);
        let left = match &self.phase {
            Phase::Idle | Phase::Applying => SETTLE_SECS + self.hold,
            Phase::Settling { left } => left + self.hold,
            Phase::Holding { left, .. } => *left,
            Phase::Finished(_) => 0,
        };
        (step, total, left)
    }
}

/// The lowest buffer that was clean and left headroom: its index in `steps`.
pub fn suggest(steps: &[Step]) -> Option<usize> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, s)| s.verdict == Verdict::Clean && s.peak_load < SUGGEST_LOAD)
        .min_by_key(|(_, s)| s.tuning.quantum)
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Party;

    const HOLD: u32 = 3;

    fn party(name: &str, xruns: u64, load: f32, listener: bool) -> Party {
        Party {
            name: name.into(),
            xruns,
            last_xrun: None,
            load,
            peak: load,
            listener,
        }
    }

    fn snap(interface: u64, app: u64, load: f32) -> Snapshot {
        Snapshot {
            driving: Driving::Interface,
            quantum: 64,
            rate: 48_000,
            interface: party("Interface", interface, load, false),
            apps: vec![party("Test", app, load, false)],
        }
    }

    fn idle() -> Snapshot {
        Snapshot {
            driving: Driving::Idle,
            ..snap(0, 0, 0.0)
        }
    }

    fn tuning(quantum: u32) -> Tuning {
        Tuning {
            quantum,
            period: Some(quantum),
            headroom: Some(32),
        }
    }

    fn sweep(sizes: &[u32]) -> Sweep {
        Sweep::new(sizes.iter().map(|q| tuning(*q)).collect(), HOLD)
    }

    /// Settle, then hold, with these cumulative counts at every sample.
    fn run_step(s: &mut Sweep, counts: impl Fn(u32) -> (u64, u64, f32)) -> Action {
        assert_eq!(s.applied(Ok(())), Action::Wait);
        let mut last = Action::Wait;
        for second in 0..SETTLE_SECS + HOLD {
            let (i, a, l) = counts(second);
            last = s.sample(&snap(i, a, l));
        }
        last
    }

    #[test]
    fn ladder_runs_largest_first() {
        let presets = Presets::default();
        let sizes: Vec<u32> = ladder(&presets, false).iter().map(|t| t.quantum).collect();
        assert_eq!(sizes, [256, 128, 64, 32]);
        assert_eq!(ladder(&presets, false)[2], presets.get(64).unwrap());
    }

    #[test]
    fn buffer_only_ladder_leaves_period_alone() {
        for t in ladder(&Presets::default(), true) {
            assert_eq!((t.period, t.headroom), (None, None));
        }
    }

    #[test]
    fn starting_applies_the_first_step() {
        let mut s = sweep(&[256, 128]);
        assert_eq!(s.start(), Action::Apply(tuning(256)));
        assert_eq!(sweep(&[]).start(), Action::Done);
    }

    #[test]
    fn settling_xruns_are_not_counted() {
        let mut s = sweep(&[256]);
        s.start();
        // Start-up errors arrive while settling, then nothing more.
        let end = run_step(&mut s, |sec| (if sec == 0 { 0 } else { 9 }, 0, 0.2));
        assert_eq!(end, Action::Done);
        let step = &s.steps()[0];
        assert_eq!((step.interface_xruns, step.app_xruns), (0, 0));
        assert_eq!(step.verdict, Verdict::Clean);
    }

    #[test]
    fn a_clean_hold_is_clean_and_a_single_xrun_is_not() {
        let mut s = sweep(&[256, 128]);
        s.start();
        assert_eq!(
            run_step(&mut s, |_| (0, 0, 0.3)),
            Action::Apply(tuning(128))
        );
        run_step(&mut s, |sec| (u64::from(sec > SETTLE_SECS), 0, 0.3));
        assert_eq!(s.steps()[0].verdict, Verdict::Clean);
        assert_eq!(s.steps()[1].verdict, Verdict::Glitches);
        assert_eq!(s.steps()[1].interface_xruns, 1);
    }

    #[test]
    fn app_xruns_alone_are_glitches_with_zero_interface_xruns() {
        let mut s = sweep(&[256]);
        s.start();
        run_step(&mut s, |sec| (0, u64::from(sec > SETTLE_SECS) * 4, 0.3));
        let step = &s.steps()[0];
        assert_eq!((step.interface_xruns, step.app_xruns), (0, 4));
        assert_eq!(step.verdict, Verdict::Glitches);
    }

    #[test]
    fn listening_only_streams_do_not_make_a_glitch() {
        let mut s = sweep(&[256]);
        s.start();
        s.applied(Ok(()));
        for sec in 0..SETTLE_SECS + HOLD {
            let mut shot = snap(0, 0, 0.3);
            shot.apps
                .push(party("Meter", u64::from(sec) * 50, 0.01, true));
            s.sample(&shot);
        }
        assert_eq!(s.steps()[0].verdict, Verdict::Clean);
    }

    #[test]
    fn load_is_averaged_and_peaked_over_the_hold() {
        let mut s = sweep(&[256]);
        s.start();
        run_step(&mut s, |sec| {
            (0, 0, [0.9, 0.9, 0.2, 0.4, 0.6][sec as usize % 5])
        });
        let step = &s.steps()[0];
        assert!((step.avg_load - 0.4).abs() < 1e-6);
        assert_eq!(step.peak_load, 0.6);
    }

    #[test]
    fn stops_two_steps_after_the_first_glitch() {
        let mut s = sweep(&[256, 128, 64, 32, 16]);
        s.start();
        assert!(matches!(
            run_step(&mut s, |_| (0, 0, 0.1)),
            Action::Apply(_)
        ));
        let glitchy = |sec| (u64::from(sec >= SETTLE_SECS), 0, 0.1_f32);
        assert!(matches!(run_step(&mut s, glitchy), Action::Apply(_)));
        assert!(matches!(run_step(&mut s, glitchy), Action::Apply(_)));
        assert_eq!(run_step(&mut s, glitchy), Action::Done);
        assert_eq!(s.steps().len(), 4);
    }

    #[test]
    fn aborts_when_the_interface_goes_idle() {
        let mut s = sweep(&[256, 128]);
        s.start();
        s.applied(Ok(()));
        for _ in 0..SETTLE_SECS {
            assert_eq!(s.sample(&snap(0, 0, 0.1)), Action::Wait);
        }
        assert_eq!(
            s.sample(&idle()),
            Action::Abort("The interface stopped driving the graph.".into())
        );
        assert!(s.steps().is_empty());
    }

    #[test]
    fn the_stream_reopening_while_settling_is_not_an_abort() {
        let mut s = sweep(&[256, 128]);
        s.start();
        s.applied(Ok(()));
        // A period or quantum change reopens the stream for a moment.
        assert_eq!(s.sample(&idle()), Action::Wait);
        assert_eq!(s.sample(&snap(0, 0, 0.1)), Action::Wait);
        assert!(matches!(s.sample(&snap(0, 0, 0.1)), Action::Wait));
    }

    #[test]
    fn aborts_when_apply_fails() {
        let mut s = sweep(&[256]);
        s.start();
        assert_eq!(
            s.applied(Err("pw-cli: gone".into())),
            Action::Abort("pw-cli: gone".into())
        );
    }

    #[test]
    fn samples_before_the_change_lands_are_ignored() {
        let mut s = sweep(&[256]);
        s.start();
        assert_eq!(s.sample(&idle()), Action::Wait);
        assert_eq!(s.sample(&snap(5, 5, 0.9)), Action::Wait);
    }

    #[test]
    fn done_and_abort_are_sticky() {
        let mut s = sweep(&[256]);
        s.start();
        assert_eq!(run_step(&mut s, |_| (0, 0, 0.1)), Action::Done);
        assert_eq!(s.sample(&idle()), Action::Done);
        assert_eq!(s.applied(Ok(())), Action::Done);
        let mut s = sweep(&[256]);
        s.start();
        s.applied(Err("no".into()));
        assert_eq!(s.sample(&snap(0, 0, 0.1)), Action::Abort("no".into()));
    }

    #[test]
    fn progress_counts_down() {
        let mut s = sweep(&[256, 128]);
        s.start();
        assert_eq!(s.progress(), (1, 2, SETTLE_SECS + HOLD));
        s.applied(Ok(()));
        s.sample(&snap(0, 0, 0.1));
        assert_eq!(s.progress(), (1, 2, SETTLE_SECS + HOLD - 1));
    }

    fn step(quantum: u32, glitches: bool, peak: f32) -> Step {
        Step {
            tuning: tuning(quantum),
            interface_xruns: u64::from(glitches),
            app_xruns: 0,
            avg_load: peak / 2.0,
            peak_load: peak,
            verdict: if glitches {
                Verdict::Glitches
            } else {
                Verdict::Clean
            },
        }
    }

    #[test]
    fn suggests_the_lowest_clean_step_under_80_percent() {
        let steps = [
            step(256, false, 0.1),
            step(128, false, 0.2),
            step(64, false, 0.9),
            step(32, true, 0.9),
        ];
        assert_eq!(suggest(&steps), Some(1));
        assert_eq!(suggest(&[step(256, true, 0.1)]), None);
        assert_eq!(suggest(&[]), None);
    }

    #[test]
    fn counts_that_go_backwards_do_not_wrap() {
        let mut s = sweep(&[256]);
        s.start();
        // The counts were reset in the middle of the hold.
        run_step(&mut s, |sec| {
            if sec < SETTLE_SECS + 1 {
                (7, 7, 0.1)
            } else {
                (0, 0, 0.1)
            }
        });
        let step = &s.steps()[0];
        assert_eq!((step.interface_xruns, step.app_xruns), (0, 0));
    }
}
