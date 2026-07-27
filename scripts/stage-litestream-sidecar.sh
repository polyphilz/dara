#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PIN="$ROOT/app/src-tauri/resources/sidecars/litestream-v1.json"
NOTICE_SOURCE="$ROOT/app/src-tauri/resources/sidecars/litestream-NOTICE"
STAGE="$ROOT/app/src-tauri/resources/release"
ARCHIVE_OVERRIDE=${1:-}

for command in curl file jq otool shasum tar; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command not found: $command" >&2
    exit 1
  }
done

test "$(uname -s)" = Darwin || {
  echo "the Litestream release stage requires macOS" >&2
  exit 1
}

EXPECTED_ARCHITECTURE=$(jq -er '.target.architecture' "$PIN")
ACTUAL_ARCHITECTURE=$(uname -m)
test "$ACTUAL_ARCHITECTURE" = "$EXPECTED_ARCHITECTURE" || {
  echo "expected $EXPECTED_ARCHITECTURE, found $ACTUAL_ARCHITECTURE" >&2
  exit 1
}

ASSET_URL=$(jq -er '.upstream.asset.url' "$PIN")
ASSET_SIZE=$(jq -er '.upstream.asset.size' "$PIN")
ASSET_SHA256=$(jq -er '.upstream.asset.sha256' "$PIN")
BINARY_ARCHIVE_PATH=$(jq -er '.binary.archivePath' "$PIN")
BINARY_SHA256=$(jq -er '.binary.sha256' "$PIN")
BINARY_SIZE=$(jq -er '.binary.size' "$PIN")
BINARY_VERSION_ARGUMENT=$(jq -er '.binary.versionArguments[0]' "$PIN")
BINARY_VERSION_OUTPUT=$(jq -er '.binary.versionOutput' "$PIN")
BINARY_MINIMUM_SYSTEM_VERSION=$(jq -er '.target.binaryMinimumSystemVersion' "$PIN")
BINARY_STAGING_PATH=$(jq -er '.stagingPaths.binary' "$PIN")
MANIFEST_STAGING_PATH=$(jq -er '.stagingPaths.releaseManifest' "$PIN")
LICENSE_SOURCE_PATH=$(jq -er '.licenseNotices[0].sourcePath' "$PIN")
LICENSE_SHA256=$(jq -er '.licenseNotices[0].sha256' "$PIN")
LICENSE_STAGING_PATH=$(jq -er '.stagingPaths.license' "$PIN")
NOTICE_STAGING_PATH=$(jq -er '.stagingPaths.notice' "$PIN")

WORK_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/dara-litestream-release.XXXXXX")
EXTRACT_ROOT="$WORK_ROOT/extract"
STAGE_TEMP="$WORK_ROOT/stage"
mkdir -p "$EXTRACT_ROOT"

cleanup() {
  rm -rf "$WORK_ROOT"
}
trap cleanup EXIT INT TERM

if test -n "$ARCHIVE_OVERRIDE"; then
  test -f "$ARCHIVE_OVERRIDE" || {
    echo "Litestream archive not found: $ARCHIVE_OVERRIDE" >&2
    exit 1
  }
  ARCHIVE=$ARCHIVE_OVERRIDE
else
  ARCHIVE="$WORK_ROOT/$(jq -er '.upstream.asset.name' "$PIN")"
  curl -fL --retry 3 --connect-timeout 15 "$ASSET_URL" -o "$ARCHIVE"
fi

ACTUAL_ASSET_SIZE=$(wc -c <"$ARCHIVE" | tr -d ' ')
test "$ACTUAL_ASSET_SIZE" = "$ASSET_SIZE" || {
  echo "Litestream archive size mismatch" >&2
  exit 1
}

ACTUAL_ASSET_SHA256=$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')
test "$ACTUAL_ASSET_SHA256" = "$ASSET_SHA256" || {
  echo "Litestream archive SHA-256 mismatch" >&2
  exit 1
}

tar -xzf "$ARCHIVE" -C "$EXTRACT_ROOT" \
  "$BINARY_ARCHIVE_PATH" \
  "$LICENSE_SOURCE_PATH"

BINARY="$EXTRACT_ROOT/$BINARY_ARCHIVE_PATH"
LICENSE="$EXTRACT_ROOT/$LICENSE_SOURCE_PATH"
test -f "$BINARY" && test ! -L "$BINARY"
test -f "$LICENSE" && test ! -L "$LICENSE"
chmod 755 "$BINARY"

ACTUAL_BINARY_SIZE=$(wc -c <"$BINARY" | tr -d ' ')
test "$ACTUAL_BINARY_SIZE" = "$BINARY_SIZE" || {
  echo "Litestream binary size mismatch" >&2
  exit 1
}

ACTUAL_BINARY_SHA256=$(shasum -a 256 "$BINARY" | awk '{print $1}')
test "$ACTUAL_BINARY_SHA256" = "$BINARY_SHA256" || {
  echo "Litestream binary SHA-256 mismatch" >&2
  exit 1
}

ACTUAL_VERSION_OUTPUT=$("$BINARY" "$BINARY_VERSION_ARGUMENT" 2>&1)
test "$ACTUAL_VERSION_OUTPUT" = "$BINARY_VERSION_OUTPUT" || {
  echo "Litestream version output mismatch: $ACTUAL_VERSION_OUTPUT" >&2
  exit 1
}

file "$BINARY" | grep -q 'Mach-O 64-bit executable arm64' || {
  file "$BINARY" >&2
  echo "Litestream is not an arm64 Mach-O executable" >&2
  exit 1
}

if otool -L "$BINARY" | tail -n +2 | grep -Ev '^[[:space:]]+(/System/Library/|/usr/lib/)' >/dev/null; then
  otool -L "$BINARY" >&2
  echo "Litestream has a non-system dynamic dependency" >&2
  exit 1
fi

ACTUAL_BINARY_MINIMUM_SYSTEM_VERSION=$(
  otool -l "$BINARY" |
    awk '
      /LC_BUILD_VERSION/ { in_build_version = 1; next }
      in_build_version && $1 == "minos" { print $2; exit }
    '
)
test "$ACTUAL_BINARY_MINIMUM_SYSTEM_VERSION" = "$BINARY_MINIMUM_SYSTEM_VERSION" || {
  echo "Litestream deployment target mismatch: $ACTUAL_BINARY_MINIMUM_SYSTEM_VERSION" >&2
  exit 1
}

ACTUAL_LICENSE_SHA256=$(shasum -a 256 "$LICENSE" | awk '{print $1}')
test "$ACTUAL_LICENSE_SHA256" = "$LICENSE_SHA256" || {
  echo "Litestream license SHA-256 mismatch" >&2
  exit 1
}

mkdir -p \
  "$STAGE_TEMP/$(dirname "$BINARY_STAGING_PATH")" \
  "$STAGE_TEMP/$(dirname "$MANIFEST_STAGING_PATH")" \
  "$STAGE_TEMP/$(dirname "$LICENSE_STAGING_PATH")" \
  "$STAGE_TEMP/$(dirname "$NOTICE_STAGING_PATH")"

install -m 755 "$BINARY" "$STAGE_TEMP/$BINARY_STAGING_PATH"
install -m 644 "$PIN" "$STAGE_TEMP/$MANIFEST_STAGING_PATH"
install -m 644 "$LICENSE" "$STAGE_TEMP/$LICENSE_STAGING_PATH"
install -m 644 "$NOTICE_SOURCE" "$STAGE_TEMP/$NOTICE_STAGING_PATH"

mkdir -p \
  "$STAGE/$(dirname "$BINARY_STAGING_PATH")" \
  "$STAGE/$(dirname "$MANIFEST_STAGING_PATH")" \
  "$STAGE/$(dirname "$LICENSE_STAGING_PATH")" \
  "$STAGE/$(dirname "$NOTICE_STAGING_PATH")"

install -m 755 "$STAGE_TEMP/$BINARY_STAGING_PATH" "$STAGE/$BINARY_STAGING_PATH"
install -m 644 "$STAGE_TEMP/$MANIFEST_STAGING_PATH" "$STAGE/$MANIFEST_STAGING_PATH"
install -m 644 "$STAGE_TEMP/$LICENSE_STAGING_PATH" "$STAGE/$LICENSE_STAGING_PATH"
install -m 644 "$STAGE_TEMP/$NOTICE_STAGING_PATH" "$STAGE/$NOTICE_STAGING_PATH"

echo "staged $(jq -er '.resourceDestinations.binary' "$PIN")"
echo "sha256 $ACTUAL_BINARY_SHA256"
echo "$ACTUAL_VERSION_OUTPUT"
