# CLI 与端到端用例

参考实现：`main` 的 C++ 二进制（容器内可运行）已录制到 `reference/`。

## CLI

| 用例 | 命令 | 期望 |
|---|---|---|
| 帮助 | `./rtsp_pusher -h` | 退出 0，列出全部参数 |
| 非法 codec | `./rtsp_pusher -c h266` | 退出 1，stderr 含“不支持的编码协议”，stdout 只有该错误（无“推流失败。”） |
| 缺失输入 | `./rtsp_pusher -i /nonexistent/x.mp4` | 退出 1，打印启动信息 + “无法打开输入文件”，再“推流失败。” |
| 未知参数 | `./rtsp_pusher --nope` | 退出 1，stderr 含“参数解析错误” |
| 非法数字 | `./rtsp_pusher -w abc` | 退出 1，stderr 含“参数解析错误” |

## 推流

| 用例 | 参数 | 期望 |
|---|---|---|
| H.264 | `-i testsrc.mp4 -u rtsp://127.0.0.1:8554/x -w 640 --height 360 --no-realtime` | `ffprobe`: h264/640x360/25fps；多轮循环；SIGTERM 退出 0 |
| H.265 | 追加 `-c h265` | `ffprobe`: hevc/640x360/25fps |
| 水印开启 | 灰色输入 + 默认 | Y 平面右上角 `x0=482,y0=8` 有 16 背景与 255 字形 |
| 水印关闭 | 追加 `--no-watermark` | 右上角区域无 16/255 |
| 实时节奏 | 不传 `--no-realtime` | 日志实时帧率 ≈25fps |
| 低延迟/码率 | `--low-latency -b 4000` | 可连接、可播放；GOP/RC 由单测覆盖 |
| UDP | 追加 `-t udp`，用 `ffprobe -rtsp_transport udp` 探测 | h264/640x360/25fps |
| SIGINT | `timeout --preserve-status -s INT 3 ...` | 退出 0，日志含 `[结束]` 与“推流结束。” |
| 不可达 RTSP | `-u rtsp://127.0.0.1:9/none` | 8 秒内退出 1，含“推流失败。” |
| 无视频流 | 输入纯音频 m4a | 退出 1，含“未找到视频流” |

## 参考产物

`reference/` 下为 C++ 录制结果：

- `cpp_help.txt`
- `cpp_bad_codec.txt`
- `cpp_missing_input.txt`
- `cpp_h264.probe` / `cpp_h264.log` / `cpp_h264.exit`
- `cpp_h265.probe` / `cpp_h265.log` / `cpp_h265.exit`

复现命令：`bash scripts/record-reference.sh`（需 `rtsp_pusher` 容器与 mediamtx）。
