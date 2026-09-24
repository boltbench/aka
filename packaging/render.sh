#!/bin/sh
# Prints the Homebrew formula or Scoop manifest for a published release.
#
#   packaging/render.sh homebrew 0.1.0 > aka.rb
#   packaging/render.sh scoop 0.1.0 > aka.json
#
# Checksums come from the release's checksums.txt. Set AKA_CHECKSUMS to a
# local file to render without downloading (used by the tests).
set -eu

kind="${1:?usage: render.sh homebrew|scoop <version>}"
version="${2:?usage: render.sh homebrew|scoop <version>}"
repo="zeuslcf/aka"
base="https://github.com/${repo}/releases/download/v${version}"

case "$kind" in
homebrew | scoop) ;;
*)
  echo "unknown kind: $kind (use homebrew or scoop)" >&2
  exit 1
  ;;
esac

if [ -n "${AKA_CHECKSUMS:-}" ]; then
  sums="$(cat "$AKA_CHECKSUMS")"
else
  sums="$(curl -fsSL "${base}/checksums.txt")"
fi

sha() {
  printf '%s\n' "$sums" | awk -v f="aka-v${version}-$1" '$2 == f { print $1 }'
}

need() {
  value="$(sha "$1")"
  if [ -z "$value" ]; then
    echo "no checksum for aka-v${version}-$1 in checksums.txt" >&2
    exit 1
  fi
  printf '%s' "$value"
}

# Look every checksum up first. An `exit` inside the $(...) further down would
# only leave that subshell, so a missing one has to fail here instead.
case "$kind" in
homebrew)
  mac_arm="$(need aarch64-apple-darwin.tar.gz)"
  mac_intel="$(need x86_64-apple-darwin.tar.gz)"
  linux_arm="$(need aarch64-unknown-linux-musl.tar.gz)"
  linux_intel="$(need x86_64-unknown-linux-musl.tar.gz)"
  ;;
scoop)
  win_x64="$(need x86_64-pc-windows-msvc.zip)"
  win_arm="$(need aarch64-pc-windows-msvc.zip)"
  ;;
esac

case "$kind" in
homebrew)
  cat <<EOF
class Aka < Formula
  desc "Manage your shell aliases from one place, in bash, zsh, fish and PowerShell"
  homepage "https://github.com/${repo}"
  version "${version}"
  license "MIT"

  on_macos do
    on_arm do
      url "${base}/aka-v${version}-aarch64-apple-darwin.tar.gz"
      sha256 "${mac_arm}"
    end
    on_intel do
      url "${base}/aka-v${version}-x86_64-apple-darwin.tar.gz"
      sha256 "${mac_intel}"
    end
  end

  on_linux do
    on_arm do
      url "${base}/aka-v${version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "${linux_arm}"
    end
    on_intel do
      url "${base}/aka-v${version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "${linux_intel}"
    end
  end

  def install
    bin.install "aka"
  end

  def caveats
    <<~EOS
      Run this once to hook aka into your shells:
        aka setup
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/aka --version")
  end
end
EOF
  ;;
scoop)
  cat <<EOF
{
    "version": "${version}",
    "description": "Manage your shell aliases from one place, in bash, zsh, fish and PowerShell",
    "homepage": "https://github.com/${repo}",
    "license": "MIT",
    "architecture": {
        "64bit": {
            "url": "${base}/aka-v${version}-x86_64-pc-windows-msvc.zip",
            "hash": "${win_x64}",
            "extract_dir": "aka-v${version}-x86_64-pc-windows-msvc"
        },
        "arm64": {
            "url": "${base}/aka-v${version}-aarch64-pc-windows-msvc.zip",
            "hash": "${win_arm}",
            "extract_dir": "aka-v${version}-aarch64-pc-windows-msvc"
        }
    },
    "bin": "aka.exe",
    "notes": "Run 'aka setup' once to hook aka into PowerShell.",
    "checkver": "github",
    "autoupdate": {
        "architecture": {
            "64bit": {
                "url": "https://github.com/${repo}/releases/download/v\$version/aka-v\$version-x86_64-pc-windows-msvc.zip",
                "extract_dir": "aka-v\$version-x86_64-pc-windows-msvc"
            },
            "arm64": {
                "url": "https://github.com/${repo}/releases/download/v\$version/aka-v\$version-aarch64-pc-windows-msvc.zip",
                "extract_dir": "aka-v\$version-aarch64-pc-windows-msvc"
            }
        },
        "hash": {
            "url": "\$url.sha256"
        }
    }
}
EOF
  ;;
esac
