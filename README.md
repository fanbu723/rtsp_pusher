# rtsp_pusher — RTSP H.264/H.265 视频推流工具

基于 Rust + FFmpeg 的 RTSP 推流程序：读取本地视频文件，解码后经滤镜链（缩放 + 格式转换）并直接绘制实时时间戳水印（含毫秒），重新编码为 H.264（默认）或 H.265（`--codec h265`）实时推送到 RTSP 服务器，并持续输出推流状态日志。

原 C++ 实现保留在 `main` 分支；本分支为行为等价的 Rust 重构，移植记录见 [`rust/HANDOFF.md`](rust/HANDOFF.md)。

## 功能特性

- 读取 FFmpeg 支持的任意视频文件，重新编码为 H.264（默认）或 H.265 推送 RTSP（`--codec h265` 切换）
- 默认优先使用硬件编码器（NVENC / Ascend / QSV），不可用时自动回退 libx264/libx265，并打印回退原因
- 自动叠加实时时间戳水印（内置点阵字体直接绘制，含毫秒；不依赖 drawtext 滤镜），默认开启，可用 `--no-watermark` 关闭
- 自动匹配输入帧率（可手动指定），避免长时间推流画面拖尾
- 输入文件播放完毕后自动循环播放
- 实时推流状态日志（已解码/已推流帧数、轮次、实时帧率、运行时长），每秒刷新
- RTSP 握手/读写超时保护，避免服务器无响应时程序无限卡死
- 命令行参数化（基于 clap）
- Docker Compose 一键并发多路推流（见“多路推流”）

## 依赖

- Rust 1.85+（clap 4.6 的最低要求，本项目在 1.100 上验证）
- FFmpeg 6.1 开发库：`libavcodec` / `libavformat` / `libavfilter` / `libavutil` / `libswscale` / `libswresample`
- 构建时需要 libclang（ffmpeg-sys-next 的 bindgen 依赖）；Ubuntu 可执行 `apt install -y libclang-dev`
- H.265 软件编码需要 FFmpeg 启用 `libx265`；硬件编码需要 FFmpeg 编译对应编码器并具备可用驱动/设备（如 NVIDIA NVENC、Ascend、Intel QSV）

Ubuntu 24.04 安装 FFmpeg 6.1 开发库：

```bash
apt install -y libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev libswresample-dev
```

Ubuntu 22.04 的 apt 默认是 FFmpeg 4.4，不能直接满足要求。若未安装 FFmpeg 6.1 开发库，`build.sh` 会自动从 FFmpeg 官方仓库拉取 `n6.1.1` 源码到 `.ffmpeg-src/` 并执行 configure 生成头文件，不依赖任何其他项目的预置目录。

如果 FFmpeg 6.1 头文件不在 pkg-config 默认路径（例如使用独立 prefix），构建时指定：

```bash
export FFMPEG_INCLUDE_DIR=/path/to/ffmpeg-6.1/include
# 可选：库目录与头文件不在同一 prefix 时
export FFMPEG_LIB_DIR=/path/to/ffmpeg-6.1/lib
```

## 构建

```bash
bash build.sh
```

生成的可执行文件为 `./rtsp_pusher`。

构建脚本会自动定位 Rust 工具链：优先使用当前 `CARGO_HOME`，否则查找 `$HOME/.cargo`、`/home/*/.cargo` 和 `/usr/local/cargo`，并匹配对应的 `RUSTUP_HOME`。因此在 `rtsp_pusher` 容器内以 root 执行时，也能复用 `/home` 下挂载的 Rust 工具链。

在容器内以 root 构建时，脚本会切换到仓库所有者 UID 执行，避免 `rust/target`、`.ffmpeg-sysroot` 和最终二进制变成 root 所有。容器内还需要安装 libclang：

```bash
apt install -y libclang-dev
```

`build.sh` 会调用 `scripts/ffmpeg-env.sh`，按以下顺序寻找 FFmpeg 6.1 头文件：`pkg-config` → `FFMPEG_INCLUDE_DIR` → 官方 FFmpeg 仓库源码（`.ffmpeg-src/`，自动 clone `n6.1.1` 并 configure）；并在 `.ffmpeg-sysroot/` 生成构建用 sysroot（头文件指向上述来源；库优先 `FFMPEG_LIB_DIR`，容器内自动使用 `/app/ffmpeg/lib`，否则用系统 FFmpeg 6.1 运行库）。

二进制动态链接 FFmpeg 6.1 运行库（`libavcodec.so.60` 等）。宿主机使用系统 FFmpeg 6.1；容器运行时由 `/app/ffmpeg` 提供 6.1 运行库（见 `docker-compose.yml`）。

## 项目结构

```text
rtsp_pusher/
├── rust/
│   ├── src/
│   │   ├── main.rs          # 入口、退出码
│   │   ├── cli.rs           # 命令行参数
│   │   ├── pipeline.rs      # 输入/解码/滤镜/水印/编码/RTSP 主循环
│   │   ├── watermark.rs     # 5x7 点阵时间戳
│   │   ├── settings.rs      # 编码器参数决策
│   │   ├── pacing.rs        # 实时节奏
│   │   ├── ffmpeg_ext.rs    # 少量 FFmpeg FFI 封装
│   │   └── error.rs
│   ├── tests/cli.rs         # CLI 契约测试
│   ├── MAP.md / DECISIONS.md / HANDOFF.md / RULES.md
│   └── app-cases/           # C++ 参考日志与用例矩阵
├── scripts/
│   ├── build-rust.sh        # cargo 构建 + 复制二进制
│   ├── ffmpeg-env.sh        # FFmpeg 6.1 探测 / sysroot
│   ├── rust-env.sh          # 定位 cargo/rustup
│   ├── repo-user.sh         # root 构建时切换仓库所有者
│   ├── test-rust.sh         # fmt / clippy / cargo test
│   ├── e2e.sh               # 端到端验证
│   ├── streams.sh           # 容器内多路推流启动脚本
│   └── record-reference.sh  # 录制 C++ 基线
├── build.sh                 # 一键构建
├── docker-compose.yml       # 容器编排（rtsp_pusher + mediamtx）
├── mediamtx/config.yml      # mediamtx 容器配置
├── README.md
└── .gitignore
```

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

# 关闭实时时间戳水印（默认开启）
./rtsp_pusher --no-watermark

# 使用 H.265/HEVC 推流（不传 --codec 时默认 H.264）
./rtsp_pusher --codec h265 -i video.mp4 -u rtsp://127.0.0.1:8554/test
```

### 启动 RTSP 服务器（mediamtx）

推流前需要先启动一个 RTSP 服务器接收推流，这里以轻量服务器 [mediamtx](https://github.com/bluenviron/mediamtx)（原 rtsp-simple-server）为例。

仓库自带的 `docker-compose.yml` 已定义 mediamtx 服务（配置见 `mediamtx/config.yml`，仅启用 RTSP 且允许任意路径推流）：

```bash
docker compose up -d
```

该命令会同时启动 mediamtx 和 rtsp_pusher；rtsp_pusher 按
`scripts/streams.sh` 中的 `STREAMS` 并发推流。两者共用主机网络，推流地址为
`rtsp://127.0.0.1:8554/<path>`。首次执行会拉取
`bluenviron/mediamtx:1.20.0` 镜像；此时无需再单独执行下面的 docker run。

**方式一：Docker（推荐）**

```bash
docker run --rm -d \
  -p 8554:8554 -p 1935:1935 -p 8888:8888 \
  bluenviron/mediamtx:latest
```

- `8554`：RTSP 服务端口（与默认推流地址 `rtsp://127.0.0.1:8554/...` 对应）
- `1935`：RTMP 端口（可选）
- `8888`：Web 控制台 / API 端口（可选）

**方式二：直接运行二进制**

从 [mediamtx releases](https://github.com/bluenviron/mediamtx/releases) 下载对应平台的压缩包：

```bash
tar -xzf mediamtx_*.tar.gz
./mediamtx
```

mediamtx 默认允许向任意路径推流，无需预先创建 `path`。服务器启动后直接运行本推流工具即可：

```bash
./rtsp_pusher -u rtsp://127.0.0.1:8554/live3
```

验证是否推流成功（另开一个终端播放）：

```bash
ffplay -fflags nobuffer -flags low_delay -framedrop rtsp://127.0.0.1:8554/live3
```

### 多路推流

`docker-compose.yml` 中 `rtsp_pusher` 的 `command` 是 `bash scripts/streams.sh`。
脚本读取顶部的 `STREAMS` 数组，在同一个容器内为每条配置启动一个 `rtsp_pusher`
子进程，因此只需要 `docker compose up -d` 就能同时推多路流。

`STREAMS` 的每个元素直接就是一组 `rtsp_pusher` 命令行参数，不需要额外的字段格式：

```bash
STREAMS=(
    "-i videos/1_烟火.mp4 -u rtsp://127.0.0.1:8554/cam1 -w 1280 --height 720 -f 25 -c h264 -b 4000 --low-latency"
    "-i videos/2_加热设备无人值守_无人.mp4 -u rtsp://127.0.0.1:8554/cam2 -w 1280 --height 720 -f 25 -c h264 -b 4000 --low-latency"
)
```

每个元素按空白拆分为参数，因此输入路径不能包含空格；以 `#` 开头的元素会被忽略。
所有流共用容器的镜像、挂载、GPU 和主机网络配置。

```bash
# 启动 mediamtx 和所有流
docker compose up -d

# 查看容器状态
docker compose ps

# 跟踪所有流日志（多路日志会合并输出）
docker compose logs -f rtsp_pusher

# 只重启推流容器
docker compose restart rtsp_pusher

# 停止所有服务
docker compose down
```

任意一路推流退出时，脚本会停止其余流并退出，随后由 compose 的
`restart: unless-stopped` 重新拉起整个推流容器，避免只剩部分流在运行。
默认示例使用 H.264 + NVENC；如播放器对 H.265 支持不好，可继续使用 `-c h264`，
需要时再改成 `-c h265`。

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
| `-c, --codec` | 输出视频编码协议（`h264` / `h265`） | `h264` |
| `--no-realtime` | 关闭实时速度控制（默认按实时速度推流） | 关闭 |
| `--no-watermark` | 关闭实时时间戳水印 | 关闭（即水印默认开启） |
| `--low-latency` | 低延迟模式：减小 GOP（10）与编码缓冲（≈0.25s），降低端到端延迟 | 关闭 |
| `-v, --verbose` | 显示详细 FFmpeg 日志（默认仅显示错误） | 关闭 |
| `-h, --help` | 显示帮助 | — |

## 工作原理

1. 打开输入文件并探测流信息（`max_analyze_duration=5s`、`probesize=20MB`），获取视频流与真实帧率（`av_guess_frame_rate`）
2. 创建 RTSP 输出上下文与编码器：默认优先 NVENC → Ascend → QSV，均不可用时回退 libx264/libx265；`--codec h265` 时选择 H.265/HEVC 对应编码器（无 B 帧、低延迟配置）
3. 构建滤镜链：`scale=<宽>:<高> → format=<编码器像素格式>`，并在编码前于帧上直接绘制实时时间戳水印（含毫秒）
4. 主循环：读帧 → 解码 → 实时同步（默认按输入帧率）→ 滤镜 → 编码 → 交织写入 RTSP
5. 每秒刷新推流状态；`Ctrl+C` / SIGTERM 安全退出并打印最终统计

## 验证

```bash
# fmt + clippy + 单测（自动探测 FFmpeg 6.1）
bash scripts/test-rust.sh

# 需要 mediamtx（127.0.0.1:8554）与 ffmpeg/ffprobe
bash scripts/e2e.sh
```

`scripts/e2e.sh` 覆盖 H.264/H.265、UDP、循环、水印开/关、SIGINT 退出、不可达 RTSP、无视频流输入；实测结果与 C++ 对比见 [`rust/HANDOFF.md`](rust/HANDOFF.md)。

## 注意事项

- 需要本地运行 RTSP 服务器（如 mediamtx / rtsp-simple-server），并保证推流路径存在
- 目标运行库为 **FFmpeg 6.1**；只有 FFmpeg 4.4 运行库的环境不能直接运行本二进制
- H.265 需要服务端与播放器支持（mediamtx 已支持）；可用 `ffprobe -rtsp_transport tcp -show_streams <rtsp-url>` 确认 `codec_name=hevc`
- 默认优先硬件编码；日志中的 `使用编码器: ...（硬件）` 表示实际生效的编码器。硬件不可用（无设备、驱动或 FFmpeg 未编译）时会自动回退并打印“回退到软件编码器”，不会静默切换
- H.265 软件编码（libx265）的 CPU 开销高于 H.264；高分辨率推流建议用 `-b` 限制码率、降低输出分辨率，或使用硬件 HEVC 编码器
- 输入文件 PTS 异常（如起始时间戳非 0）时，程序按目标帧率统一重排时间戳，保证 RTSP 时间轴稳定
- 正常退出（Ctrl+C / SIGTERM）返回 0；初始化失败、循环 seek 失败返回 1；单次写流失败记录 `[错误] 写入 RTSP 失败: ...` 后继续，最终退出码保持 0（与原 C++ 行为一致）
- 默认只显示 ERROR 级别 FFmpeg 日志，`-v` 可查看完整日志
- `rust/target/`、`.ffmpeg-src/`、`.ffmpeg-sysroot/` 与生成的可执行文件不纳入版本管理（见 `.gitignore`）
