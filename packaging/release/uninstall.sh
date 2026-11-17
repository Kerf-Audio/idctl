#!/usr/bin/env sh
# Remove what install.sh put under PREFIX (default ~/.local). Run with --udev
# to remove the device rule too.
set -eu
prefix=${PREFIX:-$HOME/.local}

rm -f "$prefix/bin/idctl-gui" \
    "$prefix/share/applications/id-mixer.desktop" \
    "$prefix/share/icons/hicolor/scalable/apps/id-mixer.svg"
for size in 48 128 256; do
    rm -f "$prefix/share/icons/hicolor/${size}x${size}/apps/id-mixer.png"
done
gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$prefix/share/applications" 2>/dev/null || true
echo "Removed iD Mixer from $prefix"

if [ "${1:-}" = "--udev" ]; then
    sudo rm -f /etc/udev/rules.d/70-id2plus.rules
    sudo udevadm control --reload
    echo "Removed the device rule."
fi
