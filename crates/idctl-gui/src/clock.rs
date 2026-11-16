//! Sample rate. The kernel audio driver owns the streaming interface, so the
//! rate is set by asking PipeWire to run its graph at that rate; the device
//! follows when the stream reopens.

use std::process::Command;

/// Rates the iD2+ runs at. EQ and the compressor stop above 96 kHz.
pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

/// Force the PipeWire graph rate. Blocks for the length of one `pw-metadata` call.
pub fn force(hz: u32) -> Result<(), String> {
    let out = Command::new("pw-metadata")
        .args(["-n", "settings", "0", "clock.force-rate", &hz.to_string()])
        .output()
        .map_err(|e| format!("pw-metadata: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "pw-metadata: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}
