# Interim package: it repackages a binary built on the host with
# `cargo build --release` (see tools/package_fedora.sh) instead of building
# from source. The proper way, a source RPM that builds offline from vendored
# crates in mock or Copr, is still to do (TODO.md).

Name:           fyle
Version:        0.1.2
Release:        1%{?dist}
Summary:        Two-panel file manager

License:        GPL-3.0-or-later
URL:            https://github.com/janpipek/private-fyle
Source0:        fyle
Source1:        io.github.janpipek.fyle.desktop
Source2:        LICENSE
Source3:        README.md

ExclusiveArch:  x86_64
BuildRequires:  desktop-file-utils
# Optional programs fyle runs for previews; without them it says what to
# install. File paths, so either ffmpeg-free or RPM Fusion's ffmpeg will do.
Recommends:     /usr/bin/pdftoppm
Recommends:     /usr/bin/ffprobe

# The binary is built and stripped outside rpmbuild.
%global debug_package %{nil}

%description
Fyle is a simple two-panel file manager in the spirit of Krusader, built on
GPUI. It previews files, searches with ripgrep's libraries, browses archives
and remote locations, and runs tools on the selection.

%prep
cp -p %{SOURCE2} %{SOURCE3} .

%install
install -Dpm 0755 %{SOURCE0} %{buildroot}%{_bindir}/fyle
desktop-file-install --dir=%{buildroot}%{_datadir}/applications %{SOURCE1}

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.janpipek.fyle.desktop

%files
%license LICENSE
%doc README.md
%{_bindir}/fyle
%{_datadir}/applications/io.github.janpipek.fyle.desktop

%changelog
* Wed Oct 21 2026 Jan Pipek <janpipek@users.noreply.github.com> - 0.1.2-1
- Confirm unknown SFTP host keys and document remote connection setup

* Wed Oct 14 2026 Jan Pipek <janpipek@users.noreply.github.com> - 0.1.1-1
- Preview GPX and FIT tracks, with trip properties
- Preview PowerPoint slide text
- Make remote protocol backends (SFTP, FTP, WebDAV, S3) optional build features

* Fri Oct 09 2026 Jan Pipek <janpipek@users.noreply.github.com> - 0.1.0-1
- First package, repackaging a locally built binary
