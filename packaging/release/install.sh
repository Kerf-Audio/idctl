#!/usr/bin/env sh
# Install iD Mixer for the current user. Set PREFIX to install elsewhere
# (default ~/.local). Run with --udev to also install the device rule, which
# asks for your password through sudo.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
prefix=${PREFIX:-$HOME/.local}

install -Dm755 "$here/idctl-gui" "$prefix/bin/idctl-gui"
install -Dm644 "$here/id-mixer.svg" "$prefix/share/icons/hicolor/scalable/apps/id-mixer.svg"
for size in 48 128 256; do
    install -Dm644 "$here/icons/id-mixer-$size.png" \
        "$prefix/share/icons/hicolor/${size}x${size}/apps/id-mixer.png"
done
install -Dm644 "$here/id-mixer.desktop" "$prefix/share/applications/id-mixer.desktop"
gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$prefix/share/applications" 2>/dev/null || true
echo "Installed iD Mixer to $prefix"

if [ "${1:-}" = "--udev" ]; then
    sudo install -Dm644 "$here/70-id2plus.rules" /etc/udev/rules.d/70-id2plus.rules
    sudo udevadm control --reload
    sudo udevadm trigger
    echo "Installed the device rule. Replug the interface."
else
    echo "To let your user open the interface without root, run: $0 --udev"
fi
case ":$PATH:" in
    *":$prefix/bin:"*) ;;
    *) echo "Add $prefix/bin to your PATH to start it with: idctl-gui" ;;
esac
