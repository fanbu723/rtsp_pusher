#!/usr/bin/env bash
# 由 build.sh source：让 ffmpeg-sys-next 找到 FFmpeg 6.1 的头文件和可链接库。
#
# 优先级：
#   1. pkg-config 已提供 libavcodec 60.x（正常安装 libav*-dev 的机器）
#   2. FFMPEG_INCLUDE_DIR 指向 FFmpeg 6.1 的 include 目录
#   3. 容器内 /app/ffmpeg/include
#   4. 从运行中的 rtsp_pusher 容器查询 /app/ffmpeg 挂载源
#
# 找不到头文件时直接失败，不猜测版本、不回退到 4.4。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SYSROOT="$ROOT/.ffmpeg-sysroot"

pc_major=""
if pkg-config --exists libavcodec 2>/dev/null; then
    pc_major=$(pkg-config --modversion libavcodec | cut -d. -f1)
fi

if [ "$pc_major" = "60" ]; then
    echo "使用 pkg-config 的 FFmpeg $(pkg-config --modversion libavcodec)" >&2
    return 0 2>/dev/null || exit 0
fi

include_dir="${FFMPEG_INCLUDE_DIR:-}"

if [ -z "$include_dir" ] && [ -f /app/ffmpeg/include/libavcodec/avcodec.h ]; then
    include_dir=/app/ffmpeg/include
fi

if [ -z "$include_dir" ] && command -v docker >/dev/null 2>&1; then
    mount_src=$(docker inspect rtsp_pusher --format \
        '{{range .Mounts}}{{if eq .Destination "/app/ffmpeg"}}{{.Source}}{{end}}{{end}}' \
        2>/dev/null || true)
    if [ -n "$mount_src" ] && [ -f "$mount_src/include/libavcodec/avcodec.h" ]; then
        include_dir="$mount_src/include"
    fi
fi

if [ -z "$include_dir" ] || [ ! -f "$include_dir/libavcodec/avcodec.h" ]; then
    cat >&2 <<'EOF'
未找到 FFmpeg 6.1 开发头文件。任选其一：
  1) apt install -y libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev libswresample-dev
  2) export FFMPEG_INCLUDE_DIR=/path/to/ffmpeg-6.1/include
EOF
    exit 1
fi

header_major=$(sed -n 's/^#define LIBAVCODEC_VERSION_MAJOR[[:space:]]*//p' \
    "$include_dir/libavcodec/version_major.h" | head -1)
header_minor=$(sed -n 's/^#define LIBAVCODEC_VERSION_MINOR[[:space:]]*//p' \
    "$include_dir/libavcodec/version.h" | head -1)
if [ "$header_major" != "60" ]; then
    echo "需要 FFmpeg 6.1（libavcodec 60），当前头文件为 $header_major.$header_minor" >&2
    exit 1
fi

rm -rf "$SYSROOT"
mkdir -p "$SYSROOT/lib"
ln -s "$include_dir" "$SYSROOT/include"

lib_dir="${FFMPEG_LIB_DIR:-}"
if [ -z "$lib_dir" ] && [ "$include_dir" = "/app/ffmpeg/include" ] && [ -d /app/ffmpeg/lib ]; then
    lib_dir=/app/ffmpeg/lib
fi

link_lib() {
    local name=$1
    local soname=$2
    local path=""
    if [ -n "$lib_dir" ] && [ -e "$lib_dir/lib$name.so" ]; then
        path="$lib_dir/lib$name.so"
    else
        path=$(ldconfig -p 2>/dev/null | awk -v n="$soname" '$1 == n { print $NF; exit }')
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
if [ -z "${LIBCLANG_PATH:-}" ] && ! ldconfig -p 2>/dev/null | grep -q 'libclang\.so'; then
    for candidate in /usr/lib/llvm-*/lib /usr/lib/x86_64-linux-gnu; do
        if [ -e "$candidate/libclang.so" ] || ls "$candidate"/libclang-*.so.* >/dev/null 2>&1; then
            export LIBCLANG_PATH="$candidate"
            break
        fi
    done
fi

export FFMPEG_DIR="$SYSROOT"
echo "使用 FFmpeg 6.1 头文件: $include_dir（libavcodec $header_major.$header_minor）" >&2
