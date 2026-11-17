//! The only code that runs PipeWire's command-line tools. The kernel audio
//! driver owns the streaming interface, so everything about streaming is
//! done by asking PipeWire.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout};

use crate::graph::{self, Interface};
use crate::tuning::{self, Found, Tuning};

/// Rates the iD2+ runs at. EQ and the compressor stop above 96 kHz.
pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// Prefer the system binary over whatever `PATH` finds first.
fn tool(name: &str) -> PathBuf {
    let system = Path::new("/usr/bin").join(name);
    if system.exists() {
        system
    } else {
        PathBuf::from(name)
    }
}

fn check(tool: &str, out: std::io::Result<Output>) -> Result<Output, String> {
    let out = out.map_err(|e| format!("{tool}: {e}"))?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(format!(
            "{tool}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

async fn run(name: &str, args: &[&str]) -> Result<Output, String> {
    check(
        name,
        tokio::process::Command::new(tool(name))
            .args(args)
            .output()
            .await,
    )
}

/// Force the PipeWire graph rate without blocking the UI. The device follows
/// when the stream reopens.
pub async fn force_rate(hz: u32) -> Result<(), String> {
    let hz = hz.to_string();
    run(
        "pw-metadata",
        &["-n", "settings", "0", "clock.force-rate", &hz],
    )
    .await
    .map(drop)
}

/// How long shutdown waits for `pw-metadata` before giving up on it.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(2);

/// Hand the graph rate back to PipeWire (0 lifts the force). For shutdown
/// only: it blocks, but never longer than [`RELEASE_TIMEOUT`], so a stuck
/// PipeWire cannot keep the window from closing.
pub fn release_rate() {
    let Ok(mut child) = Command::new(tool("pw-metadata"))
        .args(["-n", "settings", "0", "clock.force-rate", "0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let deadline = Instant::now() + RELEASE_TIMEOUT;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Every object PipeWire knows, as JSON.
pub async fn dump() -> Result<String, String> {
    let out = run("pw-dump", &[]).await?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The PipeWire version `pw-cli` is linked with; `unknown` if it will not say.
pub async fn pipewire_version() -> String {
    let out = run("pw-cli", &["--version"]).await;
    out.map_or("unknown".into(), |o| {
        crate::profile::pipewire_version(&String::from_utf8_lossy(&o.stdout))
    })
}

/// The command that runs `pw-top`. Into a pipe it writes in 4 KiB blocks, so
/// a whole second can arrive late and in a burst; `stdbuf -oL` makes it
/// deliver each line as it is printed.
fn top_argv(stdbuf: Option<PathBuf>, top: PathBuf) -> Vec<OsString> {
    let mut argv = Vec::new();
    if let Some(stdbuf) = stdbuf {
        argv.extend([stdbuf.into_os_string(), "-oL".into()]);
    }
    argv.extend([top.into_os_string(), "-b".into()]);
    argv
}

/// A running `pw-top -b`. Dropping it stops the process.
pub struct Top {
    lines: Lines<BufReader<ChildStdout>>,
    _child: Child,
}

impl Top {
    pub fn spawn() -> std::io::Result<Top> {
        let argv = top_argv(
            Path::new("/usr/bin/stdbuf")
                .exists()
                .then(|| PathBuf::from("/usr/bin/stdbuf")),
            tool("pw-top"),
        );
        let mut child = tokio::process::Command::new(&argv[0])
            .args(&argv[1..])
            // Numbers with a decimal point, whatever the desktop's locale.
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child.stdout.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
        Ok(Top {
            lines: BufReader::new(stdout).lines(),
            _child: child,
        })
    }

    /// The next line of output; `None` once `pw-top` has exited.
    pub async fn line(&mut self) -> Option<String> {
        self.lines.next_line().await.ok().flatten()
    }
}

fn run_sync(name: &str, args: &[&str]) -> Result<Output, String> {
    check(name, Command::new(tool(name)).args(args).output())
}

/// The value in `pw-metadata -n settings 0 clock.force-quantum` output.
pub fn parse_forced_quantum(metadata_out: &str) -> Option<u32> {
    let line = metadata_out
        .lines()
        .find(|l| l.contains("key:'clock.force-quantum'"))?;
    let value = line.split("value:'").nth(1)?;
    value.split('\'').next()?.parse().ok()
}

fn forced_quantum() -> Result<u32, String> {
    let out = run_sync(
        "pw-metadata",
        &["-n", "settings", "0", "clock.force-quantum"],
    )?;
    Ok(parse_forced_quantum(&String::from_utf8_lossy(&out.stdout)).unwrap_or(0))
}

fn force_quantum(frames: u32) -> Result<(), String> {
    let frames = frames.to_string();
    run_sync(
        "pw-metadata",
        &["-n", "settings", "0", "clock.force-quantum", &frames],
    )
    .map(drop)
}

fn set_alsa(node: u32, period: u32, headroom: u32) -> Result<(), String> {
    let props = format!(
        "{{ params = [ \"api.alsa.period-size\" {period} \"api.alsa.headroom\" {headroom} ] }}"
    );
    run_sync("pw-cli", &["set-param", &node.to_string(), "Props", &props]).map(drop)
}

fn interface(vendor: u16, product: u16) -> Result<Option<Interface>, String> {
    let out = run_sync("pw-dump", &[])?;
    graph::interface(&String::from_utf8_lossy(&out.stdout), vendor, product)
}

/// A neighbouring buffer size, to make PipeWire reconfigure the device.
fn nudge(quantum: u32) -> u32 {
    if quantum >= tuning::QUANTUM.1 {
        quantum / 2
    } else {
        quantum * 2
    }
}

fn find_sync(vendor: u16, product: u16) -> Result<Found, String> {
    let nodes = interface(vendor, product)?
        .map(|i| i.nodes)
        .unwrap_or_default();
    Ok(Found {
        force_quantum: forced_quantum()?,
        nodes: nodes
            .into_iter()
            .map(|n| (n.name, n.period, n.headroom))
            .collect(),
    })
}

fn apply_sync(t: &Tuning, vendor: u16, product: u16) -> Result<(), String> {
    tuning::validate(t)?;
    let Some(iface) = interface(vendor, product)? else {
        return Err("PipeWire does not list the interface.".into());
    };
    let mut changed = false;
    for node in &iface.nodes {
        let period = t.period.unwrap_or(node.period);
        let headroom = t.headroom.unwrap_or(node.headroom);
        if (period, headroom) != (node.period, node.headroom) {
            set_alsa(node.id, period, headroom)?;
            changed = true;
        }
    }
    // The device takes a new period when the graph is reconfigured, and a
    // change of quantum does that. If the quantum is already right, step
    // away and back.
    if changed && forced_quantum()? == t.quantum {
        force_quantum(nudge(t.quantum))?;
    }
    force_quantum(t.quantum)
}

/// Node id, period and headroom to write for each recorded node that still
/// exists. A node that has gone is skipped, not an error.
pub fn restore_targets(found: &Found, iface: Option<&Interface>) -> Vec<(u32, u32, u32)> {
    let nodes = iface.map(|i| i.nodes.as_slice()).unwrap_or_default();
    found
        .nodes
        .iter()
        .filter_map(|(name, period, headroom)| {
            let node = nodes.iter().find(|n| &n.name == name)?;
            Some((node.id, *period, *headroom))
        })
        .collect()
}

fn restore_sync(found: &Found, vendor: u16, product: u16) -> Result<(), String> {
    let iface = interface(vendor, product)?;
    let targets = restore_targets(found, iface.as_ref());
    // A node that will not take its old values must not keep the graph
    // forced: the buffer size is always handed back, and the first error is
    // reported afterwards.
    let mut failed = None;
    for (node, period, headroom) in &targets {
        if let Err(why) = set_alsa(*node, *period, *headroom) {
            failed.get_or_insert(why);
        }
    }
    // Same reason as in `apply_sync`; with no force to return to, the
    // values land when the stream next opens.
    if !targets.is_empty()
        && found.force_quantum != 0
        && forced_quantum().is_ok_and(|q| q == found.force_quantum)
    {
        let _ = force_quantum(nudge(found.force_quantum));
    }
    let quantum = force_quantum(found.force_quantum);
    failed.map_or(quantum, Err)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

/// What is in force now, to be put back later.
pub async fn find(vendor: u16, product: u16) -> Result<Found, String> {
    blocking(move || find_sync(vendor, product)).await
}

/// Write period and headroom to the interface's nodes, then force the
/// buffer size for the whole graph.
pub async fn apply(t: Tuning, vendor: u16, product: u16) -> Result<(), String> {
    blocking(move || apply_sync(&t, vendor, product)).await
}

pub async fn restore(found: Found, vendor: u16, product: u16) -> Result<(), String> {
    blocking(move || restore_sync(&found, vendor, product)).await
}

/// [`restore`] for start-up and shutdown: blocks, but never longer than
/// three times [`RELEASE_TIMEOUT`]. True when everything was put back.
pub fn restore_blocking(found: Found, vendor: u16, product: u16) -> bool {
    let (done, wait) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = done.send(restore_sync(&found, vendor, product).is_ok());
    });
    wait.recv_timeout(RELEASE_TIMEOUT * 3).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;

    fn node(id: u32, name: &str) -> Node {
        Node {
            id,
            name: name.into(),
            capture: false,
            running: true,
            period: 256,
            headroom: 128,
        }
    }

    fn found() -> Found {
        Found {
            force_quantum: 64,
            nodes: vec![("out".into(), 32, 32), ("in".into(), 64, 16)],
        }
    }

    #[test]
    fn forced_quantum_is_read_from_metadata_output() {
        let out = "Found \"settings\" metadata 31\n\
                   update: id:0 key:'clock.force-quantum' value:'64' type:''\n";
        assert_eq!(parse_forced_quantum(out), Some(64));
        assert_eq!(parse_forced_quantum(&out.replace("'64'", "'0'")), Some(0));
        assert_eq!(
            parse_forced_quantum("Found \"settings\" metadata 31\n"),
            None
        );
        assert_eq!(parse_forced_quantum(&out.replace("'64'", "'x'")), None);
    }

    #[test]
    fn restore_writes_recorded_values_to_the_nodes_present() {
        let iface = Interface {
            device_name: "card".into(),
            profile: "pro-audio".into(),
            nodes: vec![node(46, "in"), node(44, "out")],
        };
        assert_eq!(
            restore_targets(&found(), Some(&iface)),
            [(44, 32, 32), (46, 64, 16)]
        );
    }

    #[test]
    fn restore_skips_nodes_that_are_gone() {
        let iface = Interface {
            device_name: "card".into(),
            profile: "pro-audio".into(),
            nodes: vec![node(44, "out")],
        };
        assert_eq!(restore_targets(&found(), Some(&iface)), [(44, 32, 32)]);
        assert!(restore_targets(&found(), None).is_empty());
    }

    #[test]
    fn nudge_moves_one_step_away_and_stays_in_range() {
        assert_eq!(nudge(64), 128);
        assert_eq!(nudge(8192), 4096);
    }

    #[test]
    fn pw_top_is_line_buffered_when_stdbuf_is_there() {
        let top = PathBuf::from("/usr/bin/pw-top");
        assert_eq!(
            top_argv(Some(PathBuf::from("/usr/bin/stdbuf")), top.clone()),
            ["/usr/bin/stdbuf", "-oL", "/usr/bin/pw-top", "-b"].map(OsString::from)
        );
        assert_eq!(
            top_argv(None, top),
            ["/usr/bin/pw-top", "-b"].map(OsString::from)
        );
    }
}
