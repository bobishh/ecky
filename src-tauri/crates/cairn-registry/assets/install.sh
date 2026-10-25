#!/bin/sh
set -eu

fail() { printf 'ecky installer: %s\n' "$*" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || fail 'curl is required'
command -v tar >/dev/null 2>&1 || fail 'tar is required'

hub=${CAIRN_HUB:-https://hub.ecky-cad.com}
hub=${hub%/}
case "$hub" in
  https://*|http://127.0.0.1:*|http://localhost:*|http://\[::1\]:*) ;;
  *) fail 'CAIRN_HUB must use HTTPS; local HTTP is supported for testing' ;;
esac
case "$hub" in *[!A-Za-z0-9:/._\[\]-]*) fail 'Invalid registry URL' ;; esac
case "$(uname -s)" in Darwin) os=darwin ;; Linux) os=linux ;; *) fail 'Supported systems: macOS and Linux' ;; esac
case "$(uname -m)" in arm64|aarch64) arch=arm64 ;; x86_64|amd64) arch=x86_64 ;; *) fail 'Supported architectures: arm64 and x86_64' ;; esac
prefix=${CAIRN_PREFIX:-${HOME:?HOME or CAIRN_PREFIX is required}/.local}
case "$prefix" in /*) ;; *) fail 'CAIRN_PREFIX must be an absolute path' ;; esac
root=$prefix/lib/ecky-cli
bindir=$prefix/bin

# Never replace commands owned by another installation.
for name in ecky cairn; do
  destination=$bindir/$name
  if [ -e "$destination" ] || [ -L "$destination" ]; then
    [ -L "$destination" ] && [ "$(readlink "$destination")" = "$root/current/bin/$name" ] || fail "$destination is not managed by this installer; choose another CAIRN_PREFIX"
  fi
done
if [ -e "$root/current" ] && [ ! -L "$root/current" ]; then fail "$root/current is not a managed version link"; fi

temporary=$(mktemp -d "${TMPDIR:-/tmp}/ecky-cli.XXXXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
fetch() { curl --fail --silent --show-error --location --proto '=https,http' --proto-redir '=https' --retry 2 "$1" -o "$2"; }
fetch "$hub/cli/latest.txt" "$temporary/latest.txt"
version=$(cat "$temporary/latest.txt")
case "$version" in ''|.|..|*[!A-Za-z0-9._-]*) fail 'Invalid release version' ;; esac
[ "${#version}" -le 128 ] || fail 'Release version exceeds limit'
archive=ecky-cli-$os-$arch.tar.gz
fetch "$hub/cli/$version/SHA256SUMS" "$temporary/SHA256SUMS"
expected=$(awk -v name="$archive" '$2 == name {print $1}' "$temporary/SHA256SUMS")
[ "${#expected}" -eq 64 ] || fail "No checksum for $os/$arch in release $version"
case "$expected" in *[!a-f0-9]*) fail 'Invalid release checksum' ;; esac
fetch "$hub/cli/$version/$archive" "$temporary/$archive"
if command -v sha256sum >/dev/null 2>&1; then
  observed=$(sha256sum "$temporary/$archive" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  observed=$(shasum -a 256 "$temporary/$archive" | awk '{print $1}')
else fail 'sha256sum or shasum is required'; fi
[ "$observed" = "$expected" ] || fail 'Archive checksum mismatch; previous installation preserved'
tar -tzf "$temporary/$archive" > "$temporary/contents"
awk 'BEGIN {bad=0} /^\// || /(^|\/)\.\.($|\/)/ || !/^ecky-cli(\/|$)/ {bad=1} END {exit bad}' "$temporary/contents" || fail 'Unsafe archive paths; previous installation preserved'
tar -tvzf "$temporary/$archive" > "$temporary/types"
awk 'substr($1,1,1) != "-" && substr($1,1,1) != "d" {bad=1} END {exit bad}' "$temporary/types" || fail 'Archive links or special files are not supported'
mkdir "$temporary/unpack"
tar -xzf "$temporary/$archive" -C "$temporary/unpack" --no-same-owner
for name in ecky cairn; do
  [ -f "$temporary/unpack/ecky-cli/bin/$name" ] && [ -x "$temporary/unpack/ecky-cli/bin/$name" ] || fail "Missing executable $name"
  ECKY_DATA_DIR="$temporary/data" ECKY_CONFIG_DIR="$temporary/config" "$temporary/unpack/ecky-cli/bin/$name" --help >/dev/null || fail "$name cannot run on this system"
done

mkdir -p "$root/versions" "$bindir"
installed=$root/versions/$version-$os-$arch
if [ -e "$installed" ]; then
  [ -f "$installed/.archive-sha256" ] && [ "$(cat "$installed/.archive-sha256")" = "$expected" ] || fail 'Existing version differs; refusing immutable release replacement'
else
  staging=$root/versions/.stage-$$
  cp -R "$temporary/unpack/ecky-cli" "$staging"
  printf '%s\n' "$expected" > "$staging/.archive-sha256"
  mv "$staging" "$installed"
fi
move_link() {
  if [ "$os" = linux ]; then mv -fT "$1" "$2"; else mv -fh "$1" "$2"; fi
}
# Both command links follow one atomic version switch.
for name in ecky cairn; do
  if [ ! -L "$bindir/$name" ]; then
    ln -s "$root/current/bin/$name" "$bindir/.$name-$$"
    move_link "$bindir/.$name-$$" "$bindir/$name"
  fi
done
ln -s "$installed" "$root/.current-$$"
move_link "$root/.current-$$" "$root/current"
printf 'Installed Ecky CLI + Cairn %s (%s/%s)\nCommands: %s/ecky, %s/cairn\n' "$version" "$os" "$arch" "$bindir" "$bindir"
case ":${PATH:-}:" in *":$bindir:"*) ;; *) printf 'Add %s to PATH to use both commands directly.\n' "$bindir" ;; esac
