#!/usr/bin/env bash
# Builds what the release job publishes next to the installers (ADR 0026):
#   latest.json        the update manifest the app reads
#   notes.md           this version's section of CHANGELOG.md
#   release-files.txt  the installers to attach to the release, one per line
#
# Usage: release-manifest.sh <tag> <dist>
#   <tag>   the version tag, e.g. v0.1.0-beta.2
#   <dist>  the folder the release job downloaded the build artifacts into
#
# Needs jq. If an update signature is missing, latest.json is skipped (the
# installers are still attached), so the app keeps offering nothing instead of
# something it cannot verify.
set -euo pipefail

tag=${1:?usage: release-manifest.sh <tag> <dist>}
dist=${2:?usage: release-manifest.sh <tag> <dist>}
version=${tag#v}
base="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-pablozr/cloudrs}/releases/download/${tag}"

# The first file matching a glob, or nothing.
first() {
  local file
  for file in $1; do
    if [ -e "$file" ]; then
      printf '%s\n' "$file"
      return
    fi
  done
}

# SignPath's signed installer wins over the unsigned one.
windows_dir="$dist/cloudrs-windows-signed"
if [ ! -d "$windows_dir" ]; then
  windows_dir="$dist/cloudrs-windows-latest"
fi
exe=$(first "$windows_dir/*.exe")
deb=$(first "$dist/cloudrs-ubuntu-22.04/*.deb")
appimage=$(first "$dist/cloudrs-ubuntu-22.04/*.AppImage")
dmg=$(first "$dist/cloudrs-macos-latest/*.dmg")
app=$(first "$dist/cloudrs-macos-latest/*.app.tar.gz")

: > release-files.txt
for file in "$exe" "$deb" "$appimage" "$dmg" "$app"; do
  if [ -n "$file" ]; then
    printf '%s\n' "$file" >> release-files.txt
  fi
done

# This version's notes: the "## <version> " section, up to the next "## ".
notes=$(awk -v heading="## ${version} " '
  index($0, heading) == 1 { on = 1; next }
  /^## / { if (on) exit }
  on
' CHANGELOG.md | tr -d '\r' | sed -e '/./,$!d')
if [ -z "$notes" ]; then
  notes="See CHANGELOG.md"
fi
printf '%s\n' "$notes" > notes.md

platforms='{}'
missing=0
add_platform() {
  local key=$1 format=$2 file=$3
  if [ -z "$file" ] || [ ! -f "$file.sig" ]; then
    echo "::warning::no update signature for $key (${file:-no package}); latest.json will not be written"
    missing=1
    return
  fi
  platforms=$(jq -c \
    --arg key "$key" \
    --arg url "$base/$(basename "$file")" \
    --arg signature "$(cat "$file.sig")" \
    --arg format "$format" \
    '. + {($key): {url: $url, signature: $signature, format: $format}}' <<< "$platforms")
}
add_platform windows-x86_64 nsis "$exe"
add_platform linux-x86_64 appimage "$appimage"
add_platform macos-aarch64 app "$app"

rm -f latest.json
if [ "$missing" -ne 0 ]; then
  exit 0
fi
jq -n \
  --arg version "$version" \
  --arg notes "$notes" \
  --arg pub_date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson platforms "$platforms" \
  '{version: $version, notes: $notes, pub_date: $pub_date, platforms: $platforms}' > latest.json
