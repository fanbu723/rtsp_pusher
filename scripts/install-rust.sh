#!/usr/bin/env bash
# 安装构建所需的 Rust 工具链（>= 项目 rust-version）到仓库内的持久目录。
#
#   bash scripts/install-rust.sh
#   https_proxy=http://127.0.0.1:7897 bash scripts/install-rust.sh   # 需要代理时
#
# 默认装到 <repo>/.cargo-home 与 <repo>/.rustup-home（已加入 .gitignore）：
# 容器重建后仍然可用，scripts/rust-env.sh 会自动优先使用这两个目录。
# 同时安装 rustfmt/clippy（scripts/test-rust.sh 需要）。已满足要求时直接跳过。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
CARGO_HOME="${CARGO_HOME:-$ROOT/.cargo-home}"
RUSTUP_HOME="${RUSTUP_HOME:-$ROOT/.rustup-home}"
export CARGO_HOME RUSTUP_HOME

MIN_RUST=$(awk -F'"' '/^rust-version[[:space:]]*=/{print $2; exit}' "$ROOT/rust/Cargo.toml")
MIN_RUST=${MIN_RUST:-1.85}

# version_ge A B：A <= B 时返回 0
version_ge() {
    [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -n1)" = "$1" ]
}

# 构建只需要 cargo/rustc；fmt 与 clippy 供 scripts/test-rust.sh 使用，缺失不阻断构建。
ensure_components() {
    for component in rustfmt clippy; do
        "$CARGO_HOME/bin/rustup" component add "$component" >/dev/null 2>&1 ||
            echo "警告: 组件 $component 安装失败，可稍后重试: rustup component add $component" >&2
    done
}

# 构建脚本会 setpriv 切到仓库所有者 UID，工具链目录需归该用户所有。
fix_owner() {
    if [ "$(id -u)" -eq 0 ]; then
        owner_uid=$(stat -c '%u' "$ROOT")
        owner_gid=$(stat -c '%g' "$ROOT")
        if [ "$owner_uid" -ne 0 ]; then
            chown -R "$owner_uid:$owner_gid" "$CARGO_HOME" "$RUSTUP_HOME"
        fi
    fi
}

if [ -x "$CARGO_HOME/bin/cargo" ]; then
    have=$("$CARGO_HOME/bin/cargo" -V | awk '{print $2}')
    if version_ge "$MIN_RUST" "$have"; then
        ensure_components
        echo "已安装 Rust $have（$CARGO_HOME），无需重新安装" >&2
        exit 0
    fi
    echo "现有 Rust $have 低于项目要求的 $MIN_RUST，重新安装 ..." >&2
fi

mkdir -p "$CARGO_HOME" "$RUSTUP_HOME"
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

echo "下载 rustup-init ..." >&2
if ! curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$tmp"; then
    cat >&2 <<'EOF'
下载 rustup 失败。需要代理时先导出再重试，例如：
  export https_proxy=http://127.0.0.1:7897 http_proxy=http://127.0.0.1:7897
EOF
    exit 1
fi

echo "安装 Rust $MIN_RUST 到 $CARGO_HOME / $RUSTUP_HOME ..." >&2
RUSTUP_INIT_SKIP_PATH_CHECK=yes sh "$tmp" -y --no-modify-path \
    --profile minimal --default-toolchain "$MIN_RUST"

ensure_components
fix_owner

echo "已安装 $("$CARGO_HOME/bin/cargo" -V)（$CARGO_HOME）" >&2
