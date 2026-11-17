//! Small files that outlive a crash, under the user's state directory.

use std::path::PathBuf;

/// `$XDG_STATE_HOME/id-mixer/<name>`, falling back to `~/.local/state`.
pub fn path(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("id-mixer").join(name))
}
