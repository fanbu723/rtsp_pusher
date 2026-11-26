#!/usr/bin/env bash
# 用容器内保存的 C++ 参考二进制录制 CLI 与推流基线，产物写入 rust/app-cases/reference/。
#
# 前提：rtsp_pusher 容器存在、mediamtx 在 8554 监听、容器内可运行 C++ 二进制。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
REF_BIN="target/reference/rtsp_pusher_cpp"
OUT_DIR="$ROOT/rust/app-cases/reference"
mkdir -p "$OUT_DIR"

if ! docker inspect rtsp_pusher >/dev/null 2>&1; then
    echo "找不到 rtsp_pusher 容器" >&2
    exit 1
fi

container_run() {
    docker exec rtsp_pusher bash -lc "cd '$ROOT' && $*"
}

echo "录制 C++ CLI 基线..."
container_run "./$REF_BIN -h" >"$OUT_DIR/cpp_help.txt" 2>&1 || true

set +e
container_run "./$REF_BIN -c h266" >"$OUT_DIR/cpp_bad_codec.txt" 2>&1
echo "exit=$?" >>"$OUT_DIR/cpp_bad_codec.txt"
container_run "./$REF_BIN -i /nonexistent/rtsp-pusher.mp4 -u rtsp://127.0.0.1:8554/ref_missing" \
    >"$OUT_DIR/cpp_missing_input.txt" 2>&1
echo "exit=$?" >>"$OUT_DIR/cpp_missing_input.txt"
set -e

record_stream() {
    local name=$1 codec=$2
    local log="/tmp/cpp_${name}.log" probe="/tmp/cpp_${name}.probe" exit_file="/tmp/cpp_${name}.exit"
    container_run "
        rm -f '$log' '$probe' '$exit_file'
        ./$REF_BIN -i target/e2e/testsrc.mp4 -u rtsp://127.0.0.1:8554/cpp_${name} \
            -w 640 --height 360 --no-realtime -c '$codec' >'$log' 2>&1 &
        pid=\$!
        sleep 3
        /app/ffmpeg/bin/ffprobe -v error -rtsp_transport tcp \
            -show_entries stream=codec_name,width,height,avg_frame_rate -of default=nw=1 \
            rtsp://127.0.0.1:8554/cpp_${name} >'$probe' 2>&1
        kill -TERM \$pid
        wait \$pid
        echo \$? >'$exit_file'
    "
    docker exec rtsp_pusher cat "$probe" >"$OUT_DIR/cpp_${name}.probe"
    docker exec rtsp_pusher cat "$log" >"$OUT_DIR/cpp_${name}.log"
    docker exec rtsp_pusher cat "$exit_file" >"$OUT_DIR/cpp_${name}.exit"
    echo "已录制 $name"
}

echo "录制 C++ 推流基线..."
record_stream h264 h264
record_stream h265 h265

echo "参考产物在 $OUT_DIR"
