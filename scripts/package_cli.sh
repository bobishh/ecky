#!/bin/sh
set -eu
repository=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
output=${1:?Usage: ECKY_CLI_VERSION=revision ECKY_CLI_BINARY_DIR=path package_cli.sh OUTPUT_DIR}
version=${ECKY_CLI_VERSION:?Set ECKY_CLI_VERSION to the immutable source revision}
binaries=${ECKY_CLI_BINARY_DIR:-$repository/src-tauri/target/release}
runtime=${ECKY_OCCT_RUNTIME_DIR:-$repository/.dist/runtime/occt}
case "$version" in ''|.|..|*[!A-Za-z0-9._-]*) printf 'Invalid CLI version\n' >&2; exit 1 ;; esac
case "$(uname -s)" in Darwin) os=darwin ;; Linux) os=linux ;; *) printf 'Unsupported system\n' >&2; exit 1 ;; esac
case "$(uname -m)" in arm64|aarch64) arch=arm64 ;; x86_64|amd64) arch=x86_64 ;; *) printf 'Unsupported architecture\n' >&2; exit 1 ;; esac
[ -x "$runtime/bin/direct-occt-runner" ] || { printf 'Prepared native runtime is required: %s\n' "$runtime" >&2; exit 1; }
for name in ecky cairn; do [ -x "$binaries/$name" ] || { printf 'Missing CLI binary: %s\n' "$binaries/$name" >&2; exit 1; }; done
stage=$(mktemp -d "${TMPDIR:-/tmp}/ecky-cli-package.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
mkdir -p "$stage/ecky-cli/bin" "$stage/ecky-cli/resources/runtime"
for name in ecky cairn; do
  cp "$binaries/$name" "$stage/ecky-cli/bin/$name"
  if [ "$os" = darwin ]; then
    strip -S -x "$stage/ecky-cli/bin/$name"
    codesign --force --sign - "$stage/ecky-cli/bin/$name"
  else strip --strip-unneeded "$stage/ecky-cli/bin/$name"; fi
done
cp -R "$runtime" "$stage/ecky-cli/resources/runtime/occt"
licenses=$stage/ecky-cli/resources/runtime/occt/licenses
mkdir -p "$licenses/manifold" "$licenses/yyjson"
cp "$repository/.dist/runtime/manifold/licenses/LICENSE" "$licenses/manifold/"
cp "$repository/src-tauri/native/vendor/yyjson/LICENSE" "$licenses/yyjson/"
for dependency in "$runtime"/lib/libtbb*; do
  [ -f "$dependency" ] || continue
  notice=${ECKY_TBB_LICENSE_PATH:-}
  if [ -z "$notice" ] && command -v brew >/dev/null 2>&1; then notice=$(brew --prefix tbb)/LICENSE.txt; fi
  [ -f "$notice" ] || { printf 'ECKY_TBB_LICENSE_PATH is required for the bundled TBB libraries\n' >&2; exit 1; }
  mkdir -p "$licenses/tbb"
  cp "$notice" "$licenses/tbb/LICENSE.txt"
  break
done
# Homebrew SDKs may omit notices; use the checksum-verified OCCT source archive.
if [ ! -f "$licenses/LICENSE_LGPL_21.txt" ]; then
  source_archive=${ECKY_CLI_OCCT_CACHE:-$repository/.dist/cache/cli-occt}/occt-7.9.3.tar.gz
  [ -f "$source_archive" ] || { printf 'Pinned OCCT source archive is required for native notices\n' >&2; exit 1; }
  source_digest=$(shasum -a 256 "$source_archive" | awk '{print $1}')
  [ "$source_digest" = 5ecf094ec6b12d5413dfb851d8c3590c354058aee556e32e408bdfbf8c357d57 ] || { printf 'OCCT source checksum mismatch\n' >&2; exit 1; }
  tar -xOf "$source_archive" OCCT-7_9_3/LICENSE_LGPL_21.txt > "$licenses/LICENSE_LGPL_21.txt"
  tar -xOf "$source_archive" OCCT-7_9_3/OCCT_LGPL_EXCEPTION.txt > "$licenses/LICENSE_EXCEPTION.txt"
fi
mkdir -p "$stage/ecky-cli/resources/server" "$stage/ecky-cli/resources/model-runtime"
cp "$repository/server/freecad_runner.py" "$stage/ecky-cli/resources/server/"
cp "$repository/model-runtime/cad_sdk.py" "$repository/model-runtime/cad_framework_contract.md" "$stage/ecky-cli/resources/model-runtime/"
printf '%s\n' "$version" > "$stage/ecky-cli/VERSION"
printf 'Ecky CLI and Cairn\nSource: https://github.com/bobishh/ecky\nRevision: %s\nNative dependency notices: resources/runtime/occt/licenses\n' "$version" > "$stage/ecky-cli/README.txt"
mkdir -p "$output"
archive=ecky-cli-$os-$arch.tar.gz
COPYFILE_DISABLE=1 tar -czf "$output/$archive" -C "$stage" ecky-cli
if command -v sha256sum >/dev/null 2>&1; then digest=$(sha256sum "$output/$archive" | awk '{print $1}'); else digest=$(shasum -a 256 "$output/$archive" | awk '{print $1}'); fi
printf '%s  %s\n' "$digest" "$archive" > "$output/$archive.sha256"
if [ -f "$output/SHA256SUMS" ]; then awk -v name="$archive" '$2 != name' "$output/SHA256SUMS" > "$stage/sums"; else : > "$stage/sums"; fi
cat "$output/$archive.sha256" >> "$stage/sums"
LC_ALL=C sort "$stage/sums" > "$output/SHA256SUMS"
printf 'Packaged %s/%s: %s/%s\n' "$os" "$arch" "$output" "$archive"
