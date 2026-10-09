#!/bin/bash
# Release build that links against an Ubuntu 22.04 (glibc 2.35, GTK 4.6) sysroot so the binaries
# run on Linux Mint 21/22, LMDE 6 and Debian 12+. Create the sysroot once with:
#   sudo debootstrap --variant=minbase --include=libgtk-4-dev,pkg-config,libc6-dev,libglib2.0-dev \
#        jammy /opt/jammy http://archive.ubuntu.com/ubuntu     (the final "configure" warnings are harmless)
set -euo pipefail
SYS="${RUSTBOX_SYSROOT:-/opt/jammy}"
LIBDIR="$SYS/usr/lib/x86_64-linux-gnu"
export PKG_CONFIG_SYSROOT_DIR="$SYS"
export PKG_CONFIG_LIBDIR="$LIBDIR/pkgconfig:$SYS/usr/share/pkgconfig"
export RUSTFLAGS="-L native=$LIBDIR -C link-arg=--sysroot=$SYS -C link-arg=-Wl,-rpath-link,$LIBDIR"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target-compat}"
cd "$(dirname "$0")/.."
cargo build --release --workspace
