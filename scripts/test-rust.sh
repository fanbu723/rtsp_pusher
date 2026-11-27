#!/usr/bin/env bash
# Rust 静态检查 + 单测：fmt / clippy / cargo test。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# shellcheck source=scripts/repo-user.sh
source "$ROOT/scripts/repo-user.sh"

# shellcheck source=scripts/rust-env.sh
source "$ROOT/scripts/rust-env.sh"

# shellcheck source=scripts/ffmpeg-env.sh
source "$ROOT/scripts/ffmpeg-env.sh"

cargo fmt --manifest-path "$ROOT/rust/Cargo.toml" -- --check
cargo clippy --manifest-path "$ROOT/rust/Cargo.toml" --all-targets -- -D warnings
cargo test --manifest-path "$ROOT/rust/Cargo.toml"
