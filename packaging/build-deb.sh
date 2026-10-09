#!/bin/bash
# Builds rustbox_<version>_amd64.deb for Debian / Linux Mint (no cargo-deb needed).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/usr/lib/rust-1.85/bin:$PATH"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
PKG="rustbox_${VERSION}_amd64"
OUT="dist/${PKG}"

# glibc-2.35-compatible binaries (Mint 21/22, LMDE 6, Debian 12+); see build-compat.sh
./packaging/build-compat.sh
BIN=target-compat/release
rm -rf dist && mkdir -p "$OUT"/{DEBIAN,usr/bin,usr/share/applications,usr/share/icons/hicolor/scalable/apps,usr/share/doc/rustbox,usr/share/man/man1}

install -m 755 $BIN/rustbox $BIN/vmctl "$OUT/usr/bin/"
install -m 644 packaging/rustbox.desktop "$OUT/usr/share/applications/"
install -m 644 packaging/rustbox.svg "$OUT/usr/share/icons/hicolor/scalable/apps/"
install -m 644 packaging/copyright README.md "$OUT/usr/share/doc/rustbox/"
printf 'rustbox (%s) stable; urgency=low\n\n  * Initial release.\n\n -- BorgorNinja <BorgorNinja@users.noreply.github.com>  %s\n' \
    "$VERSION" "$(date -R)" | gzip -9n > "$OUT/usr/share/doc/rustbox/changelog.gz"
gzip -9n -c packaging/vmctl.1 > "$OUT/usr/share/man/man1/vmctl.1.gz"
chmod 644 "$OUT/usr/share/man/man1/vmctl.1.gz" "$OUT/usr/share/doc/rustbox/changelog.gz"
install -m 755 packaging/postinst packaging/postrm "$OUT/DEBIAN/"

# Minimum glibc actually required by the binaries (so Depends is honest).
GLIBC_MIN="$(objdump -T $BIN/rustbox $BIN/vmctl | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -Vu | tail -1)"
SIZE="$(du -sk "$OUT/usr" | cut -f1)"

cat > "$OUT/DEBIAN/control" <<CTL
Package: rustbox
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: amd64
Installed-Size: ${SIZE}
Maintainer: BorgorNinja <BorgorNinja@users.noreply.github.com>
Depends: libc6 (>= ${GLIBC_MIN}), libgtk-4-1 (>= 4.6), libglib2.0-0, qemu-system-x86 (>= 1:6.2), qemu-system-gui, qemu-utils, qemu-system-modules-spice, virt-viewer, ovmf, swtpm, curl
Recommends: xorriso, qemu-system-modules-opengl, zram-tools
Description: Fast KVM virtual machines for Windows on modest PCs
 RustBox is a VirtualBox-style manager written in Rust, built on KVM.
 It runs Windows 10/11 guests with Hyper-V enlightenments, NVMe/VirtIO
 storage, TPM 2.0, UEFI, memory ballooning and KSM, with defaults chosen
 for low-RAM, low-core-count Linux machines.
 .
 Includes the "rustbox" graphical manager and the "vmctl" command line tool.
CTL

( cd "$OUT" && find usr -type f -exec md5sum {} + > DEBIAN/md5sums )
dpkg-deb --root-owner-group -Zxz --build "$OUT" "dist/${PKG}.deb"
echo "Built dist/${PKG}.deb (glibc >= ${GLIBC_MIN})"
