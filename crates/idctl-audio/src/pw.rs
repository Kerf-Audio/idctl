//! The only code that runs PipeWire's command-line tools. The kernel audio
//! driver owns the streaming interface, so everything about streaming is
//! done by asking PipeWire.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout};

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

/// A running `pw-top -b`. Dropping it stops the process.
pub struct Top {
    lines: Lines<BufReader<ChildStdout>>,
    _child: Child,
}

impl Top {
    pub fn spawn() -> std::io::Result<Top> {
        let mut child = tokio::process::Command::new(tool("pw-top"))
            .arg("-b")
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
