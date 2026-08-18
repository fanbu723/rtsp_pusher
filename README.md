# rtsp_pusher — RTSP H.264 视频推流工具

基于 FFmpeg 的 RTSP 推流程序：读取本地视频文件，解码后经滤镜链（缩放 + 时间戳水印 + 格式转换）重新编码为 H.264，实时推送到 RTSP 服务器，并持续输出推流状态日志。

## 功能特性

- 读取 FFmpeg 支持的任意视频文件，重新编码为 H.264 推送 RTSP
- 自动叠加实时时间戳水印（drawtext）
- 自动匹配输入帧率（可手动指定），避免长时间推流画面拖尾
- 输入文件播放完毕后自动循环播放
- 实时推流状态日志（已解码/已推流帧数、轮次、实时帧率、运行时长），每秒刷新
- RTSP 握手/读写超时保护，避免服务器无响应时程序无限卡死
- 命令行参数化（基于 cxxopts 单头文件库）

## 依赖

- FFmpeg 4.x 开发库：`libavcodec` / `libavformat` / `libavfilter` / `libavutil` / `libswscale` / `libswresample`
- CMake >= 3.10
- GCC（支持 C++11）
- cxxopts（单头文件，已内置在 `include/` 中）

Ubuntu 安装 FFmpeg 开发库：

```bash
apt install -y libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev libswresample-dev
```

## 构建

```bash
bash build.sh
```

生成的可执行文件为 `./rtsp_pusher`。

## 使用

```bash
# 默认参数推流（输入 o2.mp4，输出 rtsp://127.0.0.1:8554/live3，2560x1440）
./rtsp_pusher

# 查看帮助
./rtsp_pusher -h

# 常用示例：指定输入/输出/分辨率/帧率
./rtsp_pusher -i video.mp4 -u rtsp://127.0.0.1:8554/test -w 1920 --height 1080 -f 30

# 指定码率 4Mbps、UDP 传输、全速推流
./rtsp_pusher -b 4000 -t udp --no-realtime
```

### 命令行参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `-i, --input` | 输入视频文件 | `o2.mp4` |
| `-u, --url` | RTSP 输出地址 | `rtsp://127.0.0.1:8554/live3` |
| `-w, --width` | 输出宽度 | `2560` |
| `--height` | 输出高度 | `1440` |
| `-f, --fps` | 目标帧率（`0` = 自动使用输入帧率） | `0` |
| `-t, --transport` | RTSP 传输协议（`tcp` / `udp`） | `tcp` |
| `-b, --bitrate` | 编码码率 kbps（`0` = CRF 自动） | `0` |
| `--no-realtime` | 关闭实时速度控制（默认按实时速度推流） | 关闭 |
| `-v, --verbose` | 显示详细 FFmpeg 日志（默认仅显示错误） | 关闭 |
| `-h, --help` | 显示帮助 | — |

## 工作原理

1. 打开输入文件并探测流信息，获取视频流与真实帧率（`av_guess_frame_rate`）
2. 创建 RTSP 输出上下文与 H.264 编码器（`ultrafast` / `zerolatency` 低延迟配置，无 B 帧）
3. 构建滤镜链：`scale=<宽>:<高> → format=yuv420p → drawtext(时间戳水印)`
4. 主循环：读帧 → 解码 → 实时同步（默认按输入帧率）→ 滤镜 → 编码 → 交织写入 RTSP
5. 每秒刷新推流状态（已解码/已推流帧数、轮次、实时帧率、运行时长）
6. `Ctrl+C` 安全退出并打印最终统计

## 注意事项

- 需要本地运行 RTSP 服务器（如 mediamtx / rtsp-simple-server），并保证推流路径存在
- 输入文件 PTS 异常（如起始时间戳非 0）时，程序按目标帧率统一重排时间戳，保证 RTSP 时间轴稳定
- 默认只显示 ERROR 级别 FFmpeg 日志，`-v` 可查看完整日志
- `build/` 目录与生成的可执行文件不纳入版本管理（见 `.gitignore`）
