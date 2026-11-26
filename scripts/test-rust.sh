#!/usr/bin/env bash
# Rust 静态检查 + 单测：fmt / clippy / cargo test。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if ! command -v cargo >/dev/null 2>&1; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# shellcheck source=scripts/ffmpeg-env.sh
source "$ROOT/scripts/ffmpeg-env.sh"

cargo fmt --manifest-path "$ROOT/rust/Cargo.toml" -- --check
cargo clippy --manifest-path "$ROOT/rust/Cargo.toml" --all-targets -- -D warnings
cargo test --manifest-path "$ROOT/rust/Cargo.toml"
