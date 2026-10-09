#!/usr/bin/env bash
# Builds fyle in release mode and packages the binary as a Fedora RPM in
# dist/fedora. An interim setup; see packaging/fedora/fyle.spec.

set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

spec=packaging/fedora/fyle.spec
version=$(sed -nE 's/^version = "([^"]+)"$/\1/p' Cargo.toml | head -n 1)
spec_version=$(sed -nE 's/^Version:[[:space:]]+([^[:space:]]+)$/\1/p' "$spec")
if [[ -z "$version" || "$version" != "$spec_version" ]]; then
    echo "error: Cargo version '$version' and RPM version '$spec_version' disagree" >&2
    exit 1
fi

missing=()
for package in rpm-build desktop-file-utils; do
    rpm -q "$package" >/dev/null 2>&1 || missing+=("$package")
done
if ((${#missing[@]})); then
    echo "error: missing packaging tools; install them with:" >&2
    echo "  sudo dnf install ${missing[*]}" >&2
    exit 1
fi

if ! git diff --quiet HEAD; then
    echo "warning: the working tree differs from HEAD; the package includes those changes" >&2
fi

cargo build --release --locked

work_dir=$(mktemp -d "${TMPDIR:-/tmp}/fyle-rpm.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT
top_dir="$work_dir/rpmbuild"
mkdir -p "$top_dir"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS,TMP}
cp -p target/release/fyle packaging/linux/io.github.janpipek.fyle.desktop \
    LICENSE README.md "$top_dir/SOURCES/"

rpmbuild -bb "$spec" \
    --define "_topdir $top_dir" \
    --define "_tmppath $top_dir/TMP"

output_dir="$repo_root/dist/fedora"
mkdir -p "$output_dir"
rm -f "$output_dir"/*.rpm
mapfile -t built < <(find "$top_dir/RPMS" -type f -name '*.rpm' | sort)
if ((${#built[@]} == 0)); then
    echo "error: rpmbuild produced no packages" >&2
    exit 1
fi
install -m 0644 "${built[@]}" "$output_dir/"

if command -v rpmlint >/dev/null; then
    rpmlint "$output_dir"/*.rpm || true
fi

echo "Fedora package written to $output_dir:"
printf '  %s\n' "${built[@]##*/}"
