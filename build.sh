#!/usr/bin/env bash
# 一键构建：调用 Rust 构建脚本，产物为仓库根目录的 ./rtsp_pusher。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec bash "$ROOT/scripts/build-rust.sh"
