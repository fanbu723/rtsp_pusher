#!/usr/bin/env bash
# 构建 Rust 版 rtsp_pusher。
#
#   bash scripts/build-rust.sh                 # 覆盖仓库根目录的 rtsp_pusher
#   RUST_PUSHER_OUT=./rtsp_pusher_rs bash scripts/build-rust.sh  # 可选：输出到对比用文件名
#
# 需要 FFmpeg 6.1 开发头文件；检测逻辑见 scripts/ffmpeg-env.sh。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# shellcheck source=scripts/repo-user.sh
source "$ROOT/scripts/repo-user.sh"

# shellcheck source=scripts/rust-env.sh
source "$ROOT/scripts/rust-env.sh"

# shellcheck source=scripts/ffmpeg-env.sh
source "$ROOT/scripts/ffmpeg-env.sh"

cargo build --release --manifest-path "$ROOT/rust/Cargo.toml"

out="${RUST_PUSHER_OUT:-$ROOT/rtsp_pusher}"
tmp="$out.tmp.$$"
cp "$ROOT/rust/target/release/rtsp_pusher" "$tmp"
chmod +x "$tmp"
mv "$tmp" "$out"
echo "已生成 $out" >&2
