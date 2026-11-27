#!/usr/bin/env bash
# Rust 版端到端验证：H.264/H.265、循环、水印、信号退出、失败路径。
#
# 依赖：mediamtx 监听 127.0.0.1:8554；ffmpeg/ffprobe；已构建的 Rust 二进制。
#   bash scripts/e2e.sh
#   RUST_PUSHER_BIN=./rtsp_pusher bash scripts/e2e.sh
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BIN=${RUST_PUSHER_BIN:-$ROOT/rtsp_pusher}
BASE_URL=${E2E_BASE_URL:-rtsp://127.0.0.1:8554}
OUT_DIR="$ROOT/target/e2e"
mkdir -p "$OUT_DIR"

if [ ! -x "$BIN" ]; then
    echo "找不到可执行文件 $BIN；先运行 scripts/build-rust.sh" >&2
    exit 1
fi
for tool in ffmpeg ffprobe python3; do
    command -v "$tool" >/dev/null || {
        echo "缺少 $tool" >&2
        exit 1
    }
done
if ! (exec 3<>/dev/tcp/127.0.0.1/8554) 2>/dev/null; then
    echo "127.0.0.1:8554 不可达；请先启动 mediamtx（docker compose up -d）" >&2
    exit 1
fi

PASS=0
FAIL=0
CURRENT_PID=""
cleanup() {
    if [ -n "$CURRENT_PID" ] && kill -0 "$CURRENT_PID" 2>/dev/null; then
        kill -KILL "$CURRENT_PID" 2>/dev/null || true
        wait "$CURRENT_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

check() {
    local name=$1
    shift
    if "$@"; then
        echo "PASS  $name"
        PASS=$((PASS + 1))
    else
        echo "FAIL  $name"
        FAIL=$((FAIL + 1))
    fi
}

make_inputs() {
    if [ ! -f "$OUT_DIR/testsrc.mp4" ]; then
        ffmpeg -y -loglevel error -f lavfi -i testsrc2=size=320x240:rate=25 \
            -t 2 -pix_fmt yuv420p -c:v libx264 -preset ultrafast "$OUT_DIR/testsrc.mp4"
    fi
    if [ ! -f "$OUT_DIR/gray.mp4" ]; then
        ffmpeg -y -loglevel error -f lavfi -i color=c=gray:s=320x240:r=25 \
            -t 2 -pix_fmt yuv420p -c:v libx264 -preset ultrafast "$OUT_DIR/gray.mp4"
    fi
    if [ ! -f "$OUT_DIR/audio.m4a" ]; then
        ffmpeg -y -loglevel error -f lavfi -i sine=frequency=440:duration=2 \
            -c:a aac "$OUT_DIR/audio.m4a"
    fi
}

probe_codec() {
    ffprobe -v error -rtsp_transport tcp \
        -show_entries stream=codec_name,width,height,avg_frame_rate -of default=nw=1 "$1"
}

check_packet_timing() {
    local url=$1 expected=$2 out=$3
    ffprobe -v error -rtsp_transport tcp -select_streams v:0 \
        -show_entries packet=pts_time -of csv=p=0 \
        -read_intervals "%+#8" "$url" >"$out" 2>/dev/null || return 1
    python3 - "$out" "$expected" <<'PY'
import sys

path, expected = sys.argv[1], float(sys.argv[2])
times = []
for line in open(path):
    value = line.strip()
    if value and value != "N/A":
        times.append(float(value))

deltas = [round(b - a, 6) for a, b in zip(times, times[1:])]
if len(deltas) < 3:
    raise SystemExit(f"not enough PTS samples: {times}")
bad = [d for d in deltas if abs(d - expected) > 0.005]
if bad:
    raise SystemExit(f"bad PTS deltas {bad}, expected {expected}s; samples={times}")
print(f"PTS deltas OK: {deltas[:4]}")
PY
}

start_pusher() {
    local log=$1
    shift
    "$BIN" "$@" >"$log" 2>&1 &
    CURRENT_PID=$!
    sleep 3
    if ! kill -0 "$CURRENT_PID" 2>/dev/null; then
        echo "推流进程提前退出，日志：" >&2
        cat "$log" >&2
        CURRENT_PID=""
        return 1
    fi
}

stop_pusher() {
    local pid=$1
    kill -TERM "$pid" 2>/dev/null || true
    wait "$pid"
}

test_stream() {
    local name=$1 codec=$2 input=$3 expected=$4
    local url="$BASE_URL/rust_$name"
    local log="$OUT_DIR/${name}.log"
    if ! start_pusher "$log" -i "$input" -u "$url" -w 640 --height 360 \
        --no-realtime -c "$codec"; then
        return 1
    fi
    local probe
    probe=$(probe_codec "$url") || {
        stop_pusher "$CURRENT_PID" || true
        CURRENT_PID=""
        return 1
    }
    local timing=0
    check_packet_timing "$url" 0.04 "$OUT_DIR/${name}.pts" || timing=1
    local rc=0
    stop_pusher "$CURRENT_PID" || rc=$?
    CURRENT_PID=""
    echo "$probe" >"$OUT_DIR/${name}.probe"
    [ "$rc" -eq 0 ] || {
        echo "  $codec 退出码=$rc" >&2
        return 1
    }
    [ "$timing" -eq 0 ] || return 1
    grep -q "codec_name=$expected" "$OUT_DIR/${name}.probe" &&
        grep -q "width=640" "$OUT_DIR/${name}.probe" &&
        grep -q "height=360" "$OUT_DIR/${name}.probe" &&
        grep -q "avg_frame_rate=25/1" "$OUT_DIR/${name}.probe" &&
        grep -qE "播放 ([2-9]|[1-9][0-9]+) 轮" "$log"
}

capture_y_plane() {
    local url=$1 out=$2
    ffmpeg -y -v error -rtsp_transport tcp -i "$url" -frames:v 1 \
        -f rawvideo -pix_fmt yuv420p "$out"
}

check_watermark() {
    local mode=$1 url=$2 raw=$3
    capture_y_plane "$url" "$raw" || return 1
    python3 - "$raw" "$mode" <<'PY'
import sys

path, mode = sys.argv[1], sys.argv[2]
width, height = 640, 360
data = open(path, "rb").read()
assert len(data) >= width * height, f"frame size {len(data)} < {width*height}"
data = data[: width * height]

# 时间戳长度 12、scale=2：box_w=150, box_h=22, x0=W-box_w-8, y0=8
x0, y0, box_w, box_h = width - 150 - 8, 8, 150, 22
region = []
for y in range(y0, y0 + box_h):
    row = y * width
    region.extend(data[row + x0 : row + x0 + box_w])

if mode == "on":
    assert region[0] == 16, f"背景角像素 {region[0]} != 16"
    assert 255 in region, "未找到白色字形像素"
    assert sum(1 for p in region if p == 255) > 10, "白色像素过少"
else:
    assert 16 not in region, "关闭水印后仍出现黑色水印背景"
    assert 255 not in region, "关闭水印后仍出现白色水印像素"
PY
}

test_udp() {
    local url="$BASE_URL/rust_udp"
    local log="$OUT_DIR/udp.log"
    if ! start_pusher "$log" -i "$OUT_DIR/testsrc.mp4" -u "$url" -w 640 \
        --height 360 --no-realtime -t udp; then
        return 1
    fi
    local probe
    probe=$(ffprobe -v error -rtsp_transport udp \
        -show_entries stream=codec_name,width,height,avg_frame_rate \
        -of default=nw=1 "$url") || {
        stop_pusher "$CURRENT_PID" || true
        CURRENT_PID=""
        return 1
    }
    local rc=0
    stop_pusher "$CURRENT_PID" || rc=$?
    CURRENT_PID=""
    echo "$probe" >"$OUT_DIR/udp.probe"
    [ "$rc" -eq 0 ] &&
        grep -q "codec_name=h264" "$OUT_DIR/udp.probe" &&
        grep -q "width=640" "$OUT_DIR/udp.probe" &&
        grep -q "height=360" "$OUT_DIR/udp.probe" &&
        grep -q "avg_frame_rate=25/1" "$OUT_DIR/udp.probe"
}

test_watermark() {
    local enabled=$1 name=$2
    local url="$BASE_URL/rust_$name"
    local log="$OUT_DIR/${name}.log"
    local raw="$OUT_DIR/${name}.yuv"
    local extra=()
    [ "$enabled" = "on" ] || extra+=(--no-watermark)
    if ! start_pusher "$log" -i "$OUT_DIR/gray.mp4" -u "$url" -w 640 \
        --height 360 --no-realtime "${extra[@]}"; then
        return 1
    fi
    local rc=0
    check_watermark "$enabled" "$url" "$raw" || rc=1
    stop_pusher "$CURRENT_PID" || rc=1
    CURRENT_PID=""
    return "$rc"
}

test_sigint() {
    local log="$OUT_DIR/sigint.log"
    local rc=0
    timeout --preserve-status -s INT 3 "$BIN" -i "$OUT_DIR/testsrc.mp4" \
        -u "$BASE_URL/rust_sigint" -w 320 --height 180 --no-realtime \
        >"$log" 2>&1 || rc=$?
    [ "$rc" -eq 0 ] && grep -q "\[结束\]" "$log" && grep -q "推流结束。" "$log"
}

test_unreachable() {
    local log="$OUT_DIR/unreachable.log"
    local rc=0
    timeout 8 "$BIN" -i "$OUT_DIR/testsrc.mp4" -u rtsp://127.0.0.1:9/none \
        -w 320 --height 180 --no-realtime >"$log" 2>&1 || rc=$?
    [ "$rc" -eq 1 ] && grep -q "推流失败。" "$log"
}

test_no_video() {
    local log="$OUT_DIR/no_video.log"
    local rc=0
    timeout 8 "$BIN" -i "$OUT_DIR/audio.m4a" -u "$BASE_URL/rust_no_video" \
        -w 320 --height 180 --no-realtime >"$log" 2>&1 || rc=$?
    [ "$rc" -eq 1 ] && grep -q "未找到视频流" "$log" && grep -q "推流失败。" "$log"
}

make_inputs

check "H.264 推流/循环/SIGTERM 退出" test_stream h264 h264 "$OUT_DIR/testsrc.mp4" h264
check "H.265 推流" test_stream h265 h265 "$OUT_DIR/testsrc.mp4" hevc
check "UDP 传输" test_udp
check "水印开启" test_watermark on wm_on
check "水印关闭" test_watermark off wm_off
check "SIGINT 优雅退出" test_sigint
check "不可达 RTSP 退出 1" test_unreachable
check "无视频流输入退出 1" test_no_video

echo
echo "结果: $PASS 通过, $FAIL 失败（产物在 $OUT_DIR）"
[ "$FAIL" -eq 0 ]
