#!/bin/sh

set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TEMPORARY=$(mktemp -d "${TMPDIR:-/tmp}/rust-cli-release-installer.XXXXXX")
trap 'rm -rf "$TEMPORARY"' EXIT HUP INT TERM

python3 "$ROOT/scripts/render-installers.py" \
    --binary fixture \
    --display-name Fixture \
    --repository owner/fixture \
    --environment-prefix FIXTURE \
    --output "$TEMPORARY/rendered"

RUST_CLI_RELEASE_INSTALLER_SOURCE_ONLY=1
export RUST_CLI_RELEASE_INSTALLER_SOURCE_ONLY
. "$TEMPORARY/rendered/install.sh"

assert_equal() {
    expected=$1
    actual=$2
    message=$3
    [ "$expected" = "$actual" ] || {
        printf '%s\n' "FAIL: $message: expected '$expected', got '$actual'" >&2
        exit 1
    }
}

assert_equal fixture-linux-x86_64 "$(artifact_for Linux x86_64 gnu)" "Linux GNU"
assert_equal fixture-linux-x86_64-musl "$(artifact_for Linux x86_64 musl)" "Linux musl"
assert_equal fixture-linux-aarch64 "$(artifact_for Linux aarch64 gnu)" "Linux ARM64"
assert_equal fixture-linux-aarch64-musl "$(artifact_for Linux aarch64 musl)" "Linux ARM64 musl"
assert_equal fixture-macos-x86_64 "$(artifact_for Darwin x86_64 gnu)" "macOS Intel"
assert_equal fixture-macos-aarch64 "$(artifact_for Darwin aarch64 gnu)" "macOS ARM64"

if (download_file http://example.invalid/file "$TEMPORARY/forbidden") >/dev/null 2>&1; then
    printf '%s\n' "FAIL: insecure URL was accepted" >&2
    exit 1
fi

mkdir -p "$TEMPORARY/release/latest/download" "$TEMPORARY/install"
artifact=$(artifact_for "$(uname -s)" "$(normalize_arch "$(uname -m)")" "$(detect_libc)")
binary="$TEMPORARY/release/latest/download/$artifact"
printf '%s\n' '#!/bin/sh' "printf 'fixture 9.9.9\\n'" > "$binary"
chmod 0755 "$binary"
checksum=$(sha256_file "$binary")
printf '%s  %s\n' "$checksum" "$artifact" > "$binary.sha256"

FIXTURE_RELEASE_BASE_URL="file://$TEMPORARY/release" \
FIXTURE_INSTALL_DIR="$TEMPORARY/install" \
RUST_CLI_RELEASE_INSTALLER_SOURCE_ONLY=0 \
sh "$TEMPORARY/rendered/install.sh" >/dev/null
assert_equal "fixture 9.9.9" "$("$TEMPORARY/install/fixture" --version)" "installed binary"

before=$(sha256_file "$TEMPORARY/install/fixture")
printf '%064d  %s\n' 0 "$artifact" > "$binary.sha256"
if FIXTURE_RELEASE_BASE_URL="file://$TEMPORARY/release" \
    FIXTURE_INSTALL_DIR="$TEMPORARY/install" \
    RUST_CLI_RELEASE_INSTALLER_SOURCE_ONLY=0 \
    sh "$TEMPORARY/rendered/install.sh" >/dev/null 2>&1; then
    printf '%s\n' "FAIL: invalid checksum was accepted" >&2
    exit 1
fi
assert_equal "$before" "$(sha256_file "$TEMPORARY/install/fixture")" "failed update preservation"
sh -n "$TEMPORARY/rendered/install.sh"
printf '%s\n' "installer tests passed"
