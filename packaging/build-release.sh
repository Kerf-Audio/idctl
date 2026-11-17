#!/usr/bin/env bash
# Build the release tarball for 64-bit Linux in a Debian 12 container, so the
# binary runs on glibc 2.35 and newer. Needs Docker on an x86_64 machine.
# Writes dist/idctl-<version>-linux-x86_64.tar.gz and dist/SHA256SUMS.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml)
name="idctl-$version-linux-x86_64"
image=idctl-release-build
volume=idctl-release-target
work=$(mktemp -d)
trap 'rm -rf "$work"; docker volume rm -f "$volume" >/dev/null 2>&1 || true' EXIT

docker build -q -t "$image" -f packaging/Dockerfile.release packaging >/dev/null

# Source goes in read-only; the build output lives in a volume that is
# removed afterwards, so nothing root-owned lands in the checkout.
docker run --rm \
    -v "$root":/src:ro \
    -v "$volume":/target \
    -v "$work":/out \
    -e CARGO_TARGET_DIR=/target \
    "$image" bash -c '
        set -euo pipefail
        cargo build --release --locked -p idctl-gui
        strip -o /out/idctl-gui /target/release/idctl-gui
        chown '"$(id -u):$(id -g)"' /out/idctl-gui
    '

# The glibc the binary asks for is the check that this did what it should.
need=$(objdump -T "$work/idctl-gui" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)
echo "binary needs $need"
case $need in GLIBC_2.3[0-6] | GLIBC_2.[0-2]*) ;; *) echo "too new for Debian 12" >&2; exit 1 ;; esac

# Allow-list: only these files go in the package.
stage="$work/$name"
mkdir -p "$stage/icons"
install -m755 "$work/idctl-gui" "$stage/idctl-gui"
install -m644 packaging/dplus-control.desktop packaging/dplus-control.svg packaging/70-id2plus.rules LICENSE "$stage/"
install -m755 packaging/release/install.sh packaging/release/uninstall.sh "$stage/"
install -m644 packaging/release/INSTALL.txt "$stage/"
for size in 48 128 256; do
    rsvg-convert -w "$size" -h "$size" packaging/dplus-control.svg -o "$stage/icons/dplus-control-$size.png"
done

mkdir -p dist
tar -C "$work" --owner=0 --group=0 --sort=name -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > SHA256SUMS)
echo "built dist/$name.tar.gz"
tar -tzf "dist/$name.tar.gz"
