#!/usr/bin/env bash
# Builds fyle in release mode and packages the binary as a Debian/Ubuntu .deb
# in dist/debian. Interim setup, like tools/package_fedora.sh: it repackages
# the host-built binary with plain dpkg-deb.

set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

version=$(sed -nE 's/^version = "([^"]+)"$/\1/p' Cargo.toml | head -n 1)
if [[ -z "$version" ]]; then
    echo "error: cannot read the version from Cargo.toml" >&2
    exit 1
fi

missing=()
for tool in dpkg-deb dpkg-shlibdeps desktop-file-validate; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
if ((${#missing[@]})); then
    echo "error: missing packaging tools; install them with:" >&2
    echo "  sudo apt install dpkg-dev desktop-file-utils" >&2
    exit 1
fi

if ! git diff --quiet HEAD; then
    echo "warning: the working tree differs from HEAD; the package includes those changes" >&2
fi

cargo build --release --locked

arch=$(dpkg --print-architecture)
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/fyle-deb.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT
root="$work_dir/pkg"
desktop=io.github.janpipek.fyle.desktop

desktop-file-validate "packaging/linux/$desktop"
install -Dm 0755 target/release/fyle "$root/usr/bin/fyle"
strip --strip-unneeded "$root/usr/bin/fyle"
install -Dm 0644 "packaging/linux/$desktop" "$root/usr/share/applications/$desktop"
install -Dm 0644 README.md "$root/usr/share/doc/fyle/README.md"
install -Dm 0644 LICENSE "$root/usr/share/doc/fyle/copyright"

# dpkg-shlibdeps wants a debian/control in the current directory.
mkdir -p "$work_dir/debian"
cat > "$work_dir/debian/control" <<CONTROL
Source: fyle

Package: fyle
Architecture: $arch
CONTROL
depends=$(cd "$work_dir" && dpkg-shlibdeps -O -e"$root/usr/bin/fyle" 2>/dev/null \
    | sed -nE 's/^shlibs:Depends=//p')

mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<CONTROL
Package: fyle
Version: $version
Section: utils
Priority: optional
Architecture: $arch
Maintainer: Jan Pipek <janpipek@users.noreply.github.com>
Depends: $depends
Recommends: poppler-utils, ffmpeg
Homepage: https://github.com/janpipek/private-fyle
Description: Two-panel file manager
 Fyle is a simple two-panel file manager in the spirit of Krusader, built on
 GPUI. It previews files, searches with ripgrep's libraries, browses archives
 and remote locations, and runs tools on the selection.
CONTROL

output_dir="$repo_root/dist/debian"
mkdir -p "$output_dir"
rm -f "$output_dir"/*.deb
deb="$output_dir/fyle_${version}_${arch}.deb"
dpkg-deb --root-owner-group --build "$root" "$deb" >/dev/null

echo "Debian package written to $output_dir:"
echo "  ${deb##*/}"
