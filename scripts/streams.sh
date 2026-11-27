#!/usr/bin/env bash
# 多路 RTSP 推流：STREAMS 的每个元素就是一组 rtsp_pusher 参数。
# 任意一路退出时停止其余进程；容器是否重启交给 compose 的 restart 策略。
# 每个元素按空白拆分为参数，输入路径不能包含空格。
set -euo pipefail
cd "$(dirname "$0")/.."

STREAMS=(
    "-i videos/1_烟火.mp4 -u rtsp://127.0.0.1:8554/cam1 -w 1280 --height 720 -f 25 -c h264 -b 4000 --low-latency"
    "-i videos/2_加热设备无人值守_无人.mp4 -u rtsp://127.0.0.1:8554/cam2 -w 1280 --height 720 -f 25 -c h264 -b 4000 --low-latency"
)

pids=()
cleanup() {
    kill "${pids[@]}" 2>/dev/null || true
    wait 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 0' INT TERM

for args in "${STREAMS[@]}"; do
    [[ -n "$args" && "$args" != \#* ]] || continue
    read -r -a argv <<<"$args"
    printf '[streams] rtsp_pusher %s\n' "$args"
    ./rtsp_pusher "${argv[@]}" &
    pids+=("$!")
done

[ "${#pids[@]}" -gt 0 ] || exit 0
wait -n
exit 1
