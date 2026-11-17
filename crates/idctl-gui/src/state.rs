//! Small files that outlive a crash, under the user's state directory.

use std::path::{Path, PathBuf};

const DIR: &str = "dplus-control";
/// The folder an earlier version of the app used.
const OLD_DIR: &str = "id-mixer";

/// `$XDG_STATE_HOME/dplus-control/<name>`, falling back to `~/.local/state`.
pub fn path(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    migrate(&base);
    Some(base.join(DIR).join(name))
}

/// Move an earlier version's folder to the new name, so a crash record or
/// saved settings it left are still found.
fn migrate(base: &Path) {
    let (old, new) = (base.join(OLD_DIR), base.join(DIR));
    if old.is_dir() && !new.exists() {
        let _ = std::fs::rename(old, new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dplus-state-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_earlier_folder_is_moved_to_the_new_name() {
        let base = scratch();
        std::fs::create_dir_all(base.join(OLD_DIR)).unwrap();
        std::fs::write(base.join(OLD_DIR).join("latency"), "quantum=64\n").unwrap();
        migrate(&base);
        assert_eq!(
            std::fs::read_to_string(base.join(DIR).join("latency")).unwrap(),
            "quantum=64\n"
        );
        assert!(!base.join(OLD_DIR).exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_new_folder_is_never_overwritten() {
        let base = scratch();
        std::fs::create_dir_all(base.join(OLD_DIR)).unwrap();
        std::fs::create_dir_all(base.join(DIR)).unwrap();
        std::fs::write(base.join(DIR).join("latency"), "new").unwrap();
        migrate(&base);
        assert_eq!(
            std::fs::read_to_string(base.join(DIR).join("latency")).unwrap(),
            "new"
        );
        assert!(base.join(OLD_DIR).exists());
        let _ = std::fs::remove_dir_all(base);
    }
}
