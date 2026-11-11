#!/bin/sh
# Builds a Debian/Ubuntu package from a release build:
#   target/debian/nabiz_<version>-1_<arch>.deb
#
# Needs only cargo, dpkg-deb and dpkg-shlibdeps (dpkg-dev). The libc6
# dependency is computed from the binary, so the package states the
# oldest glibc it really runs on.
set -eu
umask 022
cd "$(dirname "$0")/.."

name=nabiz
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
revision=1
arch=$(dpkg --print-architecture)
out="target/debian/${name}_${version}-${revision}_${arch}.deb"

cargo build --release --locked

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
root="$work/root"
mkdir -p "$root/DEBIAN"
chmod 755 "$root"

install -Dm755 "target/release/$name" "$root/usr/bin/$name"

# Machine-readable copyright file (DEP-5); Debian calls MIT "Expat".
copyright="$root/usr/share/doc/$name/copyright"
install -d "$(dirname "$copyright")"
{
    echo "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
    echo "Upstream-Name: $name"
    echo "Source: https://github.com/caganerg/$name"
    echo
    echo "Files: *"
    sed -n 's/^Copyright (c) /Copyright: /p' LICENSE
    echo "License: Expat"
    sed '1,/^Copyright/d' LICENSE | sed '/./,$!d' | sed 's/^$/./; s/^/ /'
} >"$copyright"
chmod 644 "$copyright"

# dpkg-shlibdeps wants a debian/control to read; a stub is enough.
mkdir -p "$work/src/debian"
printf 'Source: %s\n\nPackage: %s\nArchitecture: any\n' "$name" "$name" \
    >"$work/src/debian/control"
depends=$(cd "$work/src" && dpkg-shlibdeps -O -e"$root/usr/bin/$name" |
    sed -n 's/^shlibs:Depends=//p')

cat >"$root/DEBIAN/control" <<EOF
Package: $name
Version: $version-$revision
Architecture: $arch
Maintainer: Çağan Ergün <caganergun@tutanota.com>
Installed-Size: $(du -sk "$root/usr" | cut -f1)
Depends: $depends
Section: utils
Priority: optional
Homepage: https://github.com/caganerg/$name
Description: Intel işlemciler için sade, sakin bir terminal monitörü
 Çekirdek başına CPU kullanımını, her çekirdeğin sıcaklığını (coretemp),
 RAM, swap ve disk kullanımını tek ekranda gösterir. Süreç listesi yoktur.
EOF

mkdir -p "$(dirname "$out")"
# xz instead of the newer zstd default, so older dpkg versions can read it.
dpkg-deb --build --root-owner-group -Zxz "$root" "$out"
