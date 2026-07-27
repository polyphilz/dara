#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PIN="$ROOT/app/src-tauri/resources/sidecars/llama-server-v1.json"
EMBEDDING_MANIFEST="$ROOT/app/src-tauri/resources/embedding-indexes/jina-v1.json"
GOLDEN_FIXTURES="$ROOT/app/src-tauri/resources/embedding-indexes/jina-v1-golden.json"
VERIFY="$ROOT/scripts/verify-jina-v1.sh"
STAGE="$ROOT/app/src-tauri/resources/release"
MODEL_FILE=${1:-"$ROOT/models/v5-nano-retrieval-Q8_0.gguf"}

for command in cmake curl file git jq otool shasum; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command not found: $command" >&2
    exit 1
  }
done

test "$(uname -s)" = Darwin || {
  echo "the v1 llama-server release build requires macOS" >&2
  exit 1
}

ARCHITECTURE=$(uname -m)
EXPECTED_ARCHITECTURE=$(jq -er '.target.architecture' "$PIN")
test "$ARCHITECTURE" = "$EXPECTED_ARCHITECTURE" || {
  echo "expected $EXPECTED_ARCHITECTURE, found $ARCHITECTURE" >&2
  exit 1
}

test -f "$MODEL_FILE" || {
  echo "model not found: $MODEL_FILE" >&2
  echo "download the pinned model using the instructions in README.md" >&2
  exit 1
}

UPSTREAM_REPOSITORY=$(jq -er '.upstream.repository' "$PIN")
UPSTREAM_REVISION=$(jq -er '.upstream.revision' "$PIN")
BINARY_DESTINATION=$(jq -er '.resourceDestinations.binary' "$PIN")
BINARY_STAGING_PATH=$(jq -er '.stagingPaths.binary' "$PIN")
MANIFEST_STAGING_PATH=$(jq -er '.stagingPaths.releaseManifest' "$PIN")
LICENSE_SOURCE=$(jq -er '.licenseNotices[0].sourcePath' "$PIN")
LICENSE_DESTINATION=$(jq -er '.licenseNotices[0].bundlePath' "$PIN")
LICENSE_STAGING_PATH=$(jq -er '.stagingPaths.license' "$PIN")

BUILD_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/dara-llama-release.XXXXXX")
SOURCE="$BUILD_ROOT/llama.cpp"
BUILD="$BUILD_ROOT/build"
STAGE_TEMP=

cleanup() {
  rm -rf "$BUILD_ROOT"
  if test -n "$STAGE_TEMP"; then
    rm -rf "$STAGE_TEMP"
  fi
}
trap cleanup EXIT INT TERM

git init -q "$SOURCE"
git -C "$SOURCE" remote add origin "$UPSTREAM_REPOSITORY"
git -C "$SOURCE" fetch -q --depth 1 origin "$UPSTREAM_REVISION"
git -C "$SOURCE" checkout -q --detach FETCH_HEAD

ACTUAL_REVISION=$(git -C "$SOURCE" rev-parse HEAD)
test "$ACTUAL_REVISION" = "$UPSTREAM_REVISION" || {
  echo "checked out $ACTUAL_REVISION instead of $UPSTREAM_REVISION" >&2
  exit 1
}

CMAKE_ARGUMENTS="$BUILD_ROOT/cmake-arguments.txt"
jq -er '.cmake.flags[]' "$PIN" >"$CMAKE_ARGUMENTS"
set -- -S "$SOURCE" -B "$BUILD"
while IFS= read -r argument; do
  set -- "$@" "$argument"
done <"$CMAKE_ARGUMENTS"
cmake "$@"

BUILD_JOBS=$(sysctl -n hw.logicalcpu 2>/dev/null || printf '1')
cmake --build "$BUILD" --config Release --parallel "$BUILD_JOBS" \
  --target llama-server llama-embedding

LLAMA_SERVER="$BUILD/bin/llama-server"
LLAMA_EMBEDDING="$BUILD/bin/llama-embedding"
test -x "$LLAMA_SERVER"
test -x "$LLAMA_EMBEDDING"

VERSION_OUTPUT=$("$LLAMA_SERVER" --version 2>&1)

LLAMA_DEVICE=none \
LLAMA_GPU_LAYERS=0 \
LLAMA_EMBEDDING="$LLAMA_EMBEDDING" \
LLAMA_SERVER="$LLAMA_SERVER" \
  "$VERIFY" "$MODEL_FILE"

METAL_DEVICE=MTL0
LLAMA_DEVICE="$METAL_DEVICE" \
LLAMA_GPU_LAYERS=all \
LLAMA_REQUIRE_METAL=1 \
LLAMA_EMBEDDING="$LLAMA_EMBEDDING" \
LLAMA_SERVER="$LLAMA_SERVER" \
  "$VERIFY" "$MODEL_FILE"

file "$LLAMA_SERVER" | grep -q 'Mach-O 64-bit executable arm64' || {
  file "$LLAMA_SERVER" >&2
  echo "llama-server is not an arm64 Mach-O executable" >&2
  exit 1
}

if otool -L "$LLAMA_SERVER" | tail -n +2 | grep -Ev '^[[:space:]]+(/System/Library/|/usr/lib/)' >/dev/null; then
  otool -L "$LLAMA_SERVER" >&2
  echo "llama-server has a non-system dynamic dependency" >&2
  exit 1
fi

BINARY_SHA256=$(shasum -a 256 "$LLAMA_SERVER" | awk '{print $1}')
BINARY_SIZE=$(wc -c <"$LLAMA_SERVER" | tr -d ' ')
MANIFEST_SHA256=$(shasum -a 256 "$EMBEDDING_MANIFEST" | awk '{print $1}')
GOLDEN_SHA256=$(shasum -a 256 "$GOLDEN_FIXTURES" | awk '{print $1}')
LICENSE_SHA256=$(shasum -a 256 "$SOURCE/$LICENSE_SOURCE" | awk '{print $1}')

STAGE_TEMP=$(mktemp -d "$ROOT/app/src-tauri/resources/.release.XXXXXX")
mkdir -p \
  "$STAGE_TEMP/$(dirname "$BINARY_STAGING_PATH")" \
  "$STAGE_TEMP/$(dirname "$MANIFEST_STAGING_PATH")" \
  "$STAGE_TEMP/$(dirname "$LICENSE_STAGING_PATH")"

install -m 755 "$LLAMA_SERVER" "$STAGE_TEMP/$BINARY_STAGING_PATH"
install -m 644 "$SOURCE/$LICENSE_SOURCE" "$STAGE_TEMP/$LICENSE_STAGING_PATH"

jq \
  --arg binarySha256 "$BINARY_SHA256" \
  --argjson binarySize "$BINARY_SIZE" \
  --arg versionOutput "$VERSION_OUTPUT" \
  --arg embeddingManifestSha256 "$MANIFEST_SHA256" \
  --arg goldenFixturesSha256 "$GOLDEN_SHA256" \
  --arg licenseSha256 "$LICENSE_SHA256" \
  '. + {
    binary: {
      bundlePath: .resourceDestinations.binary,
      sha256: $binarySha256,
      size: $binarySize,
      versionOutput: $versionOutput
    },
    verification: {
      modelBundled: false,
      embeddingManifest: {
        bundlePath: "embedding-indexes/jina-v1.json",
        sha256: $embeddingManifestSha256
      },
      goldenFixtures: {
        bundlePath: "embedding-indexes/jina-v1-golden.json",
        sha256: $goldenFixturesSha256
      },
      cpuPassed: true,
      metalPassed: true
    },
    licenseNotices: (
      .licenseNotices
      | map(. + {sha256: $licenseSha256})
    )
  }' "$PIN" >"$STAGE_TEMP/$MANIFEST_STAGING_PATH"
chmod 644 "$STAGE_TEMP/$MANIFEST_STAGING_PATH"

rm -rf "$STAGE"
mv "$STAGE_TEMP" "$STAGE"
STAGE_TEMP=

echo "staged $BINARY_DESTINATION"
echo "sha256 $BINARY_SHA256"
echo "$VERSION_OUTPUT"
