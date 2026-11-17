#!/usr/bin/env sh
# Build and install D+ Control for the current user (binary, icon, launcher).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
cargo build --release

prefix=${PREFIX:-$HOME/.local}
# Earlier versions installed under the name id-mixer.
rm -f "$prefix/share/applications/id-mixer.desktop" "$prefix/share/icons/hicolor/scalable/apps/id-mixer.svg" \
    "$prefix"/share/icons/hicolor/*/apps/id-mixer.png
install -Dm755 target/release/idctl-gui "$prefix/bin/idctl-gui"
install -Dm644 packaging/dplus-control.svg "$prefix/share/icons/hicolor/scalable/apps/dplus-control.svg"
for s in 48 128 256; do
    rsvg-convert -w "$s" -h "$s" packaging/dplus-control.svg \
        -o "$prefix/share/icons/hicolor/${s}x${s}/apps/dplus-control.png" 2>/dev/null || {
        mkdir -p "$prefix/share/icons/hicolor/${s}x${s}/apps"
        rsvg-convert -w "$s" -h "$s" packaging/dplus-control.svg \
            -o "$prefix/share/icons/hicolor/${s}x${s}/apps/dplus-control.png"
    }
done
install -Dm644 packaging/dplus-control.desktop "$prefix/share/applications/dplus-control.desktop"

gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$prefix/share/applications" 2>/dev/null || true
echo "Installed to $prefix"
