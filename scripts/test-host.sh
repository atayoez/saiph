#!/usr/bin/env bash
# Run the unit tests of the host-testable crates (those without a forced
# bare-metal target, such as frames/) on this computer.
#
#   scripts/test-host.sh [cargo test arguments]
#
# Why a script: Cargo reads .cargo/config.toml starting from the *current
# directory*, and the repo's config turns on build-std and the bare-metal
# targets for everything, which ordinary `std` test binaries cannot use.
# Running Cargo from / leaves that config out.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd /
exec cargo test --manifest-path "$root/Cargo.toml" --workspace --exclude kernel "$@"
