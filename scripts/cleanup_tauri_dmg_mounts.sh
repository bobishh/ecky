#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE_DIR="${ECKY_TAURI_DMG_BUNDLE_DIR:-$ROOT/src-tauri/target/release/bundle/macos}"
DRY_RUN=0

if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=1
fi

if [[ ! -d "$BUNDLE_DIR" ]]; then
  exit 0
fi

if ! command -v hdiutil >/dev/null 2>&1; then
  exit 0
fi

mounts_for_image() {
  local image="$1"
  hdiutil info | awk -v image="$image" '
    /^image-path[[:space:]]*:/ {
      current = $0
      sub(/^image-path[[:space:]]*:[[:space:]]*/, "", current)
      in_image = (current == image)
      next
    }
    in_image && $1 ~ /^\/dev\/disk/ && $NF ~ /^\/Volumes\// {
      print $NF
    }
  '
}

find "$BUNDLE_DIR" -maxdepth 1 -type f -name 'rw.*.dmg' -print | while IFS= read -r image; do
  mounts="$(mounts_for_image "$image")"
  if [[ -n "$mounts" ]]; then
    while IFS= read -r mount; do
      [[ -z "$mount" ]] && continue
      if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "Would detach stale Tauri DMG mount: $mount"
      else
        echo "Detaching stale Tauri DMG mount: $mount"
        hdiutil detach "$mount"
      fi
    done <<< "$mounts"
  fi

  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "Would remove stale Tauri DMG image: $image"
  else
    echo "Removing stale Tauri DMG image: $image"
    rm -f "$image"
  fi
done
