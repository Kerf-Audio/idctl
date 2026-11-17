//! CPU and memory use, read from `/proc`, for the Latency page: a busy core
//! or a full memory is the commonest reason a buffer will not hold.

use std::fs;

/// Clock ticks a core has spent, in total and not idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreTicks {
    pub busy: u64,
    pub total: u64,
}

/// One entry per core from `/proc/stat` text; the all-cores line is skipped.
pub fn parse_stat(text: &str) -> Vec<CoreTicks> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            fields.next()?.strip_prefix("cpu")?.parse::<usize>().ok()?;
            let v: Vec<u64> = fields.filter_map(|f| f.parse().ok()).collect();
            // user nice system idle iowait irq softirq steal; the guest
            // columns after them are already counted in user and nice.
            let idle = v.get(3)? + v.get(4).copied().unwrap_or(0);
            let total: u64 = v.iter().take(8).sum();
            Some(CoreTicks {
                busy: total - idle,
                total,
            })
        })
        .collect()
}

/// Each core's busy share (0 to 1) between two readings.
pub fn usage(before: &[CoreTicks], after: &[CoreTicks]) -> Vec<f32> {
    after
        .iter()
        .enumerate()
        .map(|(i, now)| {
            let Some(then) = before.get(i).filter(|t| now.total > t.total) else {
                return 0.0;
            };
            let busy = now.busy.saturating_sub(then.busy) as f32;
            (busy / (now.total - then.total) as f32).clamp(0.0, 1.0)
        })
        .collect()
}

/// Memory in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
}

impl Memory {
    pub fn used_share(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f32 / self.total as f32
        }
    }
}

/// Used memory is what is not available for new work, as `free` reports it.
pub fn parse_meminfo(text: &str) -> Option<Memory> {
    let kib = |key: &str| -> Option<u64> {
        let line = text.lines().find_map(|l| l.strip_prefix(key))?;
        line.split_whitespace().next()?.parse::<u64>().ok()
    };
    let total = kib("MemTotal:")? * 1024;
    let available = kib("MemAvailable:")? * 1024;
    Some(Memory {
        total,
        used: total.saturating_sub(available),
    })
}

pub fn memory() -> Option<Memory> {
    parse_meminfo(&fs::read_to_string("/proc/meminfo").ok()?)
}

/// Shortest span, in clock ticks of 10 ms, that a reading may cover. Over a
/// single tick a core is either idle or busy, which would show as 0 or 100.
const MIN_TICKS: u64 = 50;

/// How far each new reading pulls the figure shown, so a one-second burst
/// does not flash a core to 100.
const SMOOTHING: f32 = 0.5;

/// Turns successive `/proc/stat` readings into per-core use.
#[derive(Default)]
pub struct Sampler {
    previous: Vec<CoreTicks>,
    cores: Vec<f32>,
}

impl Sampler {
    pub fn push(&mut self, stat: &str) {
        let now = parse_stat(stat);
        if self.previous.is_empty() {
            self.previous = now;
            return;
        }
        let span = now
            .iter()
            .zip(&self.previous)
            .map(|(n, p)| n.total.saturating_sub(p.total))
            .max()
            .unwrap_or(0);
        if span < MIN_TICKS && now.len() == self.previous.len() {
            // Keep the older reading as the baseline until enough time has
            // passed for the difference to mean something.
            return;
        }
        let raw = usage(&self.previous, &now);
        self.cores = if self.cores.len() == raw.len() {
            self.cores
                .iter()
                .zip(&raw)
                .map(|(shown, new)| shown + SMOOTHING * (new - shown))
                .collect()
        } else {
            raw
        };
        self.previous = now;
    }

    /// Read `/proc/stat` now.
    pub fn sample(&mut self) {
        if let Ok(stat) = fs::read_to_string("/proc/stat") {
            self.push(&stat);
        }
    }

    /// Busy share of each core; empty until two readings have been taken.
    pub fn cores(&self) -> &[f32] {
        &self.cores
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  200 0 100 700 0 0 0 0 0 0\n\
                        cpu0 100 0 50 350 0 0 0 0 0 0\n\
                        cpu1 100 0 50 350 0 0 0 0 0 0\n\
                        intr 12345 0 0\n";
    const MEMINFO: &str = "MemTotal:       32000000 kB\n\
                           MemFree:         1000000 kB\n\
                           MemAvailable:   24000000 kB\n\
                           Buffers:          500000 kB\n\
                           SwapTotal:       8000000 kB\n\
                           SwapFree:        6000000 kB\n";

    #[test]
    fn reads_each_core_but_not_the_total() {
        let ticks = parse_stat(STAT);
        assert_eq!(ticks.len(), 2);
        assert_eq!(
            ticks[0],
            CoreTicks {
                busy: 150,
                total: 500
            }
        );
    }

    #[test]
    fn iowait_counts_as_idle() {
        let ticks = parse_stat("cpu0 10 0 10 70 10 0 0 0\n");
        assert_eq!(
            ticks[0],
            CoreTicks {
                busy: 20,
                total: 100
            }
        );
    }

    #[test]
    fn usage_is_the_busy_share_of_the_interval() {
        let before = [
            CoreTicks {
                busy: 100,
                total: 400,
            },
            CoreTicks {
                busy: 0,
                total: 100,
            },
        ];
        let after = [
            CoreTicks {
                busy: 200,
                total: 500,
            },
            CoreTicks {
                busy: 0,
                total: 200,
            },
        ];
        assert_eq!(usage(&before, &after), [1.0, 0.0]);
    }

    #[test]
    fn usage_survives_a_stalled_clock_and_a_changed_core_count() {
        let t = [CoreTicks { busy: 5, total: 10 }];
        assert_eq!(usage(&t, &t), [0.0]);
        assert_eq!(usage(&[], &t), [0.0]);
        assert_eq!(usage(&t, &[]), Vec::<f32>::new());
        let backwards = [CoreTicks { busy: 1, total: 2 }];
        assert_eq!(usage(&t, &backwards), [0.0]);
    }

    #[test]
    fn reads_memory_in_bytes() {
        let m = parse_meminfo(MEMINFO).unwrap();
        assert_eq!(m.total, 32_000_000 * 1024);
        assert_eq!(m.used, 8_000_000 * 1024);
        assert!((m.used_share() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn memory_without_the_fields_is_none() {
        assert_eq!(parse_meminfo(""), None);
        assert_eq!(parse_meminfo("MemTotal: 10 kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: x kB\nMemAvailable: 1 kB\n"), None);
    }

    #[test]
    fn available_above_total_does_not_underflow() {
        let m = parse_meminfo("MemTotal: 10 kB\nMemAvailable: 20 kB\n").unwrap();
        assert_eq!(m.used, 0);
    }

    #[test]
    fn a_sampler_needs_two_readings_before_it_shows_use() {
        let mut s = Sampler::default();
        assert!(s.cores().is_empty());
        s.push(STAT);
        assert!(s.cores().is_empty());
        s.push(&STAT.replace("cpu0 100 0 50 350", "cpu0 200 0 50 350"));
        assert_eq!(s.cores().len(), 2);
        assert!(s.cores()[0] > 0.99);
        assert_eq!(s.cores()[1], 0.0);
    }

    #[test]
    fn a_reading_too_soon_after_the_last_is_ignored() {
        let mut s = Sampler::default();
        s.push("cpu0 0 0 0 0 0 0 0 0\n");
        // Five ticks, every one busy: far too short to mean anything.
        s.push("cpu0 5 0 0 0 0 0 0 0\n");
        assert!(s.cores().is_empty());
        // A hundred ticks since the first reading, five of them busy.
        s.push("cpu0 5 0 0 95 0 0 0 0\n");
        assert!((s.cores()[0] - 0.05).abs() < 1e-6);
    }

    #[test]
    fn a_spike_is_smoothed() {
        let mut s = Sampler::default();
        s.push("cpu0 0 0 0 0 0 0 0 0\n");
        s.push("cpu0 100 0 0 0 0 0 0 0\n");
        assert_eq!(s.cores(), [1.0]);
        s.push("cpu0 100 0 0 100 0 0 0 0\n");
        assert_eq!(s.cores(), [0.5]);
    }
}
