#!/usr/bin/env bash
# 由 build.sh source：让 ffmpeg-sys-next 找到 FFmpeg 6.1 的头文件和可链接库。
#
# 头文件优先级：
#   1. pkg-config 已提供 libavcodec 60.x（正常安装 libav*-dev 的机器）
#   2. FFMPEG_INCLUDE_DIR 指向 FFmpeg 6.1 的 include 目录
#   3. 从 FFmpeg 官方仓库拉取 n6.1.1 源码（.ffmpeg-src/），configure 生成所需头文件
#
# 库来源：FFMPEG_LIB_DIR 指定的目录，否则用 ldconfig 找到的系统 FFmpeg 6.1 运行库。
#
# 不依赖任何其他项目的预置 FFmpeg 目录；找不到头文件时直接失败，不猜测版本、不回退到 4.4。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SYSROOT="$ROOT/.ffmpeg-sysroot"
FFMPEG_SRC="$ROOT/.ffmpeg-src"
FFMPEG_TAG="${FFMPEG_TAG:-n6.1.1}"
FFMPEG_GIT_URL="${FFMPEG_GIT_URL:-https://git.ffmpeg.org/ffmpeg.git}"

pc_major=""
if pkg-config --exists libavcodec 2>/dev/null; then
    pc_major=$(pkg-config --modversion libavcodec | cut -d. -f1)
fi

if [ "$pc_major" = "60" ]; then
    echo "使用 pkg-config 的 FFmpeg $(pkg-config --modversion libavcodec)" >&2
    return 0 2>/dev/null || exit 0
fi

include_dir="${FFMPEG_INCLUDE_DIR:-}"

if [ -z "$include_dir" ]; then
    # 从官方仓库获取固定 tag 的源码；configure 会生成 avconfig.h / config.h 等
    # bindgen 需要的文件，公开头文件本身不受 configure 选项影响。
    if [ ! -d "$FFMPEG_SRC/.git" ]; then
        if ! command -v git >/dev/null 2>&1; then
            echo "未找到 FFmpeg 6.1 头文件且缺少 git，无法从 $FFMPEG_GIT_URL 拉取源码" >&2
            exit 1
        fi
        echo "从官方仓库拉取 FFmpeg $FFMPEG_TAG 源码到 $FFMPEG_SRC ..." >&2
        git clone --depth 1 --branch "$FFMPEG_TAG" "$FFMPEG_GIT_URL" "$FFMPEG_SRC" >&2
    fi

    if [ ! -f "$FFMPEG_SRC/libavutil/avconfig.h" ] || [ ! -f "$FFMPEG_SRC/config.h" ]; then
        configure_args=(--disable-autodetect --disable-doc --disable-programs)
        if ! command -v nasm >/dev/null 2>&1 && ! command -v yasm >/dev/null 2>&1; then
            configure_args+=(--disable-x86asm)
        fi
        echo "配置 FFmpeg 源码以生成头文件 ..." >&2
        ( cd "$FFMPEG_SRC" && ./configure "${configure_args[@]}" >/dev/null )
    fi

    include_dir="$FFMPEG_SRC"
fi

if [ -z "$include_dir" ] || [ ! -f "$include_dir/libavcodec/avcodec.h" ]; then
    cat >&2 <<'EOF'
未找到 FFmpeg 6.1 开发头文件。任选其一：
  1) apt install -y libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev libswresample-dev
  2) export FFMPEG_INCLUDE_DIR=/path/to/ffmpeg-6.1/include
EOF
    exit 1
fi

header_major=$(awk '/^#define LIBAVCODEC_VERSION_MAJOR[[:space:]]/{print $3; exit}' \
    "$include_dir/libavcodec/version_major.h")
header_minor=$(awk '/^#define LIBAVCODEC_VERSION_MINOR[[:space:]]/{print $3; exit}' \
    "$include_dir/libavcodec/version.h")
if [ "$header_major" != "60" ]; then
    echo "需要 FFmpeg 6.1（libavcodec 60），当前头文件为 $header_major.$header_minor" >&2
    exit 1
fi

rm -rf "$SYSROOT"
mkdir -p "$SYSROOT/lib"
ln -s "$include_dir" "$SYSROOT/include"

lib_dir="${FFMPEG_LIB_DIR:-}"
if [ -z "$lib_dir" ] && [ -d /app/ffmpeg/lib ]; then
    lib_dir=/app/ffmpeg/lib
fi

link_lib() {
    local name=$1
    local soname=$2
    local path=""
    if [ -n "$lib_dir" ] && [ -e "$lib_dir/lib$name.so" ]; then
        path="$lib_dir/lib$name.so"
    else
        path=$(ldconfig -p 2>/dev/null | awk -v n="$soname" '$1 == n && !p { p = $NF } END { if (p) print p }')
    fi
    if [ -z "$path" ] || [ ! -e "$path" ]; then
        echo "缺少 FFmpeg 6.1 运行库 $soname（需要与头文件同版本）" >&2
        exit 1
    fi
    ln -s "$path" "$SYSROOT/lib/lib$name.so"
}

link_lib avcodec libavcodec.so.60
link_lib avformat libavformat.so.60
link_lib avfilter libavfilter.so.9
link_lib avutil libavutil.so.58
link_lib swscale libswscale.so.7
link_lib swresample libswresample.so.4

# bindgen 需要 libclang；Ubuntu 常见只装了带版本号的运行库。
if [ -z "${LIBCLANG_PATH:-}" ] && ! ldconfig -p 2>/dev/null | grep 'libclang\.so' >/dev/null; then
    for candidate in /usr/lib/llvm-*/lib /usr/lib/x86_64-linux-gnu; do
        if [ -e "$candidate/libclang.so" ] || ls "$candidate"/libclang-*.so.* >/dev/null 2>&1; then
            export LIBCLANG_PATH="$candidate"
            break
        fi
    done
fi
if [ -z "${LIBCLANG_PATH:-}" ] && ! ldconfig -p 2>/dev/null | grep 'libclang\.so' >/dev/null; then
    echo "缺少 libclang（bindgen 需要）。Ubuntu 可执行: apt install -y libclang-dev" >&2
    exit 1
fi

export FFMPEG_DIR="$SYSROOT"
echo "使用 FFmpeg 6.1 头文件: $include_dir（libavcodec $header_major.$header_minor）" >&2
