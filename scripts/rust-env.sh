#!/usr/bin/env bash
# 定位 Rust 工具链；容器以 root 运行时复用挂载进来的用户级 cargo/rustup。
set -euo pipefail

if [ -z "${CARGO_HOME:-}" ]; then
    candidates=()
    # 仓库内工具链（scripts/install-rust.sh 安装）优先级最高
    if [ -n "${ROOT:-}" ]; then
        candidates+=("$ROOT/.cargo-home")
    fi
    candidates+=("$HOME/.cargo" /home/*/.cargo /usr/local/cargo)
    for candidate in "${candidates[@]}"; do
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
    candidates=()
    if [ -n "${ROOT:-}" ]; then
        candidates+=("$ROOT/.rustup-home")
    fi
    candidates+=("$HOME/.rustup" /home/*/.rustup /usr/local/rustup)
    for candidate in "${candidates[@]}"; do
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

# 提前把「cargo 版本过旧」翻译成可执行的提示，避免 cargo 报 edition2024 之类的晦涩错误。
if [ -n "${ROOT:-}" ] && [ -f "$ROOT/rust/Cargo.toml" ]; then
    min_rust=$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "$ROOT/rust/Cargo.toml")
    have_rust=$(cargo -V 2>/dev/null | awk '{print $2}')
    if [ -n "$min_rust" ] && [ -n "$have_rust" ] &&
        [ "$(printf '%s\n%s\n' "$min_rust" "$have_rust" | sort -V | head -n1)" != "$min_rust" ]; then
        echo "cargo $have_rust 低于项目要求的 $min_rust；先运行: bash $ROOT/scripts/install-rust.sh" >&2
        exit 1
    fi
fi
