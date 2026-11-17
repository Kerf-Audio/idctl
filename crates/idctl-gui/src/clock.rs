//! Sample rate. The kernel audio driver owns the streaming interface, so the
//! rate is set by asking PipeWire to run its graph at that rate; the device
//! follows when the stream reopens.

use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Rates the iD2+ runs at. EQ and the compressor stop above 96 kHz.
pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
/// Prefer the system binary over whatever `PATH` finds first.
fn pw_metadata() -> &'static str {
    const SYSTEM: &str = "/usr/bin/pw-metadata";
    if Path::new(SYSTEM).exists() {
        SYSTEM
    } else {
        "pw-metadata"
    }
}

fn check(out: std::io::Result<Output>) -> Result<(), String> {
    let out = out.map_err(|e| format!("pw-metadata: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "pw-metadata: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Force the PipeWire graph rate without blocking the UI.
pub async fn force(hz: u32) -> Result<(), String> {
    check(
        tokio::process::Command::new(pw_metadata())
            .args(["-n", "settings", "0", "clock.force-rate", &hz.to_string()])
            .output()
            .await,
    )
}

/// How long shutdown waits for `pw-metadata` before giving up on it.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(2);

/// Hand the graph rate back to PipeWire (0 lifts the force). For shutdown
/// only: it blocks, but never longer than [`RELEASE_TIMEOUT`], so a stuck
/// PipeWire cannot keep the window from closing.
pub fn release() {
    let Ok(mut child) = Command::new(pw_metadata())
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
