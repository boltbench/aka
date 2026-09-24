#!/bin/sh
# Installs aka on macOS or Linux.
#
#   curl -fsSL https://github.com/boltbench/aka/releases/latest/download/install.sh | sh
#
# Settings (all optional):
#   AKA_VERSION       version to install, like 0.1.0 (default: the latest)
#   AKA_INSTALL_DIR   where the binary goes (default: ~/.local/bin)
set -eu

repo="boltbench/aka"
install_dir="${AKA_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
fail() { printf 'aka install: %s\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || fail "needs \`$1\`, which isn't installed"
}

download() {
  case "$1" in
  file://*) cp "${1#file://}" "$2" ;;
  *)
    if command -v curl >/dev/null 2>&1; then
      curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
      wget -qO "$2" "$1"
    else
      fail "needs curl or wget to download"
    fi
    ;;
  esac
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

case "$(uname -s)" in
Darwin) os="apple-darwin" ;;
Linux) os="unknown-linux-musl" ;;
*) fail "this script is for macOS and Linux. On Windows, use install.ps1" ;;
esac

case "$(uname -m)" in
x86_64 | amd64) arch="x86_64" ;;
arm64 | aarch64) arch="aarch64" ;;
*) fail "no prebuilt binary for $(uname -m). Build from source with cargo instead" ;;
esac

need tar
need uname

if [ -z "${AKA_VERSION:-}" ]; then
  # The latest release's page redirects to its tag, e.g. .../tag/v0.1.0
  need curl
  url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/${repo}/releases/latest")"
  AKA_VERSION="${url##*/v}"
  [ -n "$AKA_VERSION" ] && [ "$AKA_VERSION" != "$url" ] || fail "couldn't find the latest version"
fi
version="${AKA_VERSION#v}"

name="aka-v${version}-${arch}-${os}"
base="${AKA_DOWNLOAD_BASE:-https://github.com/${repo}/releases/download/v${version}}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading aka ${version} for ${arch}-${os}..."
download "${base}/${name}.tar.gz" "$tmp/${name}.tar.gz"
download "${base}/${name}.tar.gz.sha256" "$tmp/${name}.tar.gz.sha256"

expected="$(cut -d' ' -f1 < "$tmp/${name}.tar.gz.sha256")"
actual="$(sha256 "$tmp/${name}.tar.gz")"
[ "$expected" = "$actual" ] || fail "checksum mismatch, the download may be corrupted (expected $expected, got $actual)"

tar -xzf "$tmp/${name}.tar.gz" -C "$tmp"
mkdir -p "$install_dir"
cp "$tmp/${name}/aka" "$install_dir/aka"
chmod 755 "$install_dir/aka"

say "Installed aka ${version} to ${install_dir}/aka"
case ":${PATH}:" in
*":${install_dir}:"*) ;;
*) say "Note: ${install_dir} isn't on your PATH yet. Add it in your shell profile." ;;
esac
say "Next, run: aka setup"
