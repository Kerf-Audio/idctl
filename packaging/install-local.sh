#!/usr/bin/env sh
# Build and install iD Mixer for the current user (binary, icon, launcher).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
cargo build --release

prefix=${PREFIX:-$HOME/.local}
install -Dm755 target/release/idctl-gui "$prefix/bin/idctl-gui"
install -Dm644 packaging/id-mixer.svg "$prefix/share/icons/hicolor/scalable/apps/id-mixer.svg"
for s in 48 128 256; do
    rsvg-convert -w "$s" -h "$s" packaging/id-mixer.svg \
        -o "$prefix/share/icons/hicolor/${s}x${s}/apps/id-mixer.png" 2>/dev/null || {
        mkdir -p "$prefix/share/icons/hicolor/${s}x${s}/apps"
        rsvg-convert -w "$s" -h "$s" packaging/id-mixer.svg \
            -o "$prefix/share/icons/hicolor/${s}x${s}/apps/id-mixer.png"
    }
done
install -Dm644 packaging/id-mixer.desktop "$prefix/share/applications/id-mixer.desktop"

gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$prefix/share/applications" 2>/dev/null || true
echo "Installed to $prefix"
