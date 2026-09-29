#!/usr/bin/env bash
# Verify build selection without building or reaching privileged install steps.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo() {
    return 42
}
export -f cargo

assert_build() {
    local expected="$1"
    shift
    local output status

    if output="$("$REPO_ROOT/dist/install.sh" "$@" 2>&1)"; then
        echo "installer unexpectedly continued past the Cargo stub" >&2
        exit 1
    else
        status=$?
    fi

    if [[ $status -ne 42 || "$output" != ">> cargo build $expected" ]]; then
        printf 'args: %s\nexpected: %s\nstatus: %s\noutput: %s\n' \
            "$*" "$expected" "$status" "$output" >&2
        exit 1
    fi
}

assert_build "--release -p shoji_wm" --no-portal
assert_build "--profile release-fast -p shoji_wm" --dev --no-portal
assert_build "--release -p shoji_wm -p xdg-desktop-portal-shojiwm"

echo "installer build scope: ok"
