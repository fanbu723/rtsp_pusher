#!/usr/bin/env bash
# 定位 Rust 工具链；容器以 root 运行时复用挂载进来的用户级 cargo/rustup。
set -euo pipefail

if [ -z "${CARGO_HOME:-}" ]; then
    for candidate in "$HOME/.cargo" /home/*/.cargo /usr/local/cargo; do
        if [ -x "$candidate/bin/cargo" ]; then
            CARGO_HOME="$candidate"
            break
        fi
    done
fi
if [ -n "${CARGO_HOME:-}" ]; then
    export CARGO_HOME
    case ":$PATH:" in
        *":$CARGO_HOME/bin:"*) ;;
        *) export PATH="$CARGO_HOME/bin:$PATH" ;;
    esac
fi

if [ -z "${RUSTUP_HOME:-}" ]; then
    for candidate in "$HOME/.rustup" /home/*/.rustup /usr/local/rustup; do
        if [ -d "$candidate/toolchains" ]; then
            RUSTUP_HOME="$candidate"
            break
        fi
    done
fi
if [ -n "${RUSTUP_HOME:-}" ]; then
    export RUSTUP_HOME
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "找不到 cargo。请安装 Rust，或设置 CARGO_HOME/RUSTUP_HOME/PATH 指向已安装的工具链。" >&2
    exit 1
fi
