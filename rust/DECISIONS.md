# 迁移决策记录

## ADR-001：在 `refactor/rust` 分支重构

Status: accepted (2026-11-26)

Context: 现有实现是 `main` 上的 C++ 单文件工具，需要与线上版本隔离。

Decision: 从 `main`（`bca5151`）新建 `refactor/rust`，在分支内完成 Rust 实现、验证与 C++ 删除。

Consequences: `main` 保留 C++ 可回退；分支最终只保留 Rust。

## ADR-002：目标 FFmpeg 6.1 + ffmpeg-next 6.1.1

Status: accepted (2026-11-26)

Context: 旧 C++ 二进制链接容器系统 FFmpeg 4.4；宿主机只有 FFmpeg 6.1 运行库；容器 `LD_LIBRARY_PATH` 已包含 `/app/ffmpeg/lib`（6.1）；`ffmpeg-next` 6.1.1 对 libavcodec 60 有完整绑定。

Decision: Rust 版链接 FFmpeg 6.1，使用 `ffmpeg-next = "=6.1.1"` + `ffmpeg-sys-next = "=6.1.0"`；构建时用 FFmpeg 6.1 头文件与宿主系统 6.1 库生成 `FFMPEG_DIR` sysroot。

Consequences:

- 宿主机与容器都能加载同一二进制（容器使用 `/app/ffmpeg/lib` 的 6.1）。
- FFmpeg 4.4→6.1 的行为差异需 E2E 对比；`stimeout` 被移除，见 ADR-008。
- 不再兼容只有 FFmpeg 4.4 运行库的环境。

## ADR-003：代码放在 `rust/`，验证后删除 C++

Status: accepted (2026-11-26)

Context: rust-anything whole-app 约定把移植产物放在 `<repo>/rust/`；用户要求最终只保留 Rust。

Decision: crate 位于 `rust/`，构建脚本把二进制复制到仓库根目录；等价性通过后删除 `src/`、`include/`、`CMakeLists.txt`。

Consequences: `main` 与 git 历史保留 C++；分支根目录仍是同名 `rtsp_pusher` 可执行文件。

## ADR-004：等价口径

Status: accepted (2026-11-26)

Context: 水印含实时墙钟，且 FFmpeg 4.4/6.1 编码器输出不保证逐字节一致。

Decision: 以 CLI 契约、退出码、日志结构、编码参数、帧管线行为、`ffprobe` 流属性、水印像素结构为等价依据；时间戳/fps 数值在对比中归一化。

Consequences: 不声称 RTSP 字节级一致；水印用单测 + 抽取帧结构对比证明。

## ADR-005：保持 C++ 的写流失败语义

Status: accepted (2026-11-26)

Context: C++ 在 `av_interleaved_write_frame` 失败时只打印错误并继续，最终退出码仍为 0；README 的“推流失败返回非零”只覆盖初始化与 seek 失败。

Decision: Rust 保持可观察行为：写帧失败打印 `[错误] 写入 RTSP 失败: ...` 后继续，最终退出码 0；README 同步修正描述。

Consequences: 与 README 旧文案不再冲突；若未来要改为失败退出，需单独决策并更新用例。

## ADR-006：rust-anything 只做流程校验，不作为门禁

Status: accepted (2026-11-26)

Context: 本机未预装 `rust-anything`；其语言矩阵不包含 C++，无法对原实现执行 `analyze/bridge/record/diff/gate`。自动跑 README 示例还可能触发长时间推流。

Decision: 尝试安装并运行 `doctor/analyze`，把实际结果记录在此；不依赖 `gate` 声称通过，改用 CLI 用例、C++ 参考日志与 E2E 矩阵验证。

实测（2026-11-26）：

- `rust-anything 5.0.1` 安装成功；`doctor` 中 `cargo` 在补 PATH 后为 ok。
- `rust-anything analyze .` 把仓库误判为 TypeScript/JavaScript，报告 “0 source files / 0 units / no unit to port”（C++ 源不在其语言矩阵内）。
- `rust-anything rules .` 可读取 `rust/RULES.md` 的 11 条规则。

Consequences: 验收证据来自 `cargo test`、`scripts/e2e.sh`、`scripts/record-reference.sh`；不伪称 gate。

## ADR-007：构建时生成 FFmpeg sysroot

Status: accepted (2026-11-26)

Context: 宿主机有 FFmpeg 6.1 运行库但无开发头文件；仓库外部的 FFmpeg 6.1 prefix 不能被硬编码进项目。

Decision: `scripts/ffmpeg-env.sh` 依次尝试 pkg-config（要求 libavcodec 60）、`FFMPEG_INCLUDE_DIR`、容器内 `/app/ffmpeg/include`、运行中容器的 `/app/ffmpeg` 挂载源；随后生成 `.ffmpeg-sysroot`，头文件软链到来源，库优先用同前缀 `lib`，否则用 `ldconfig` 的 6.1 系统库。

Consequences: 普通装了 `libav*-dev` 的机器零配置；本机通过容器挂载源自动发现；不提交 sysroot。

## ADR-008：FFmpeg 6.1 移除 `stimeout`

Status: accepted (2026-11-26)

Context: FFmpeg 6.1 的 RTSP 选项只接受 `timeout`；C++ 同时设置了 `stimeout` 和 `timeout`。

Decision: 只设置 `timeout=5000000`（微秒），保留 5 秒 IO 超时。

Consequences: 超时语义不变；不再向 6.1 传未知选项。

## ADR-009：私有编码器选项随 `avcodec_open2` 传入

Status: accepted (2026-11-26)

Context: `av_opt_set(enc_ctx, "preset", ...)` 在 FFmpeg 6.1 且 `avcodec_alloc_context3(NULL)` 的场景下不会生效；`avcodec_open2` 也不会为 NULL 分配的 context 正确初始化 `priv_data`。用 C harness 复现了 `broken ffmpeg default settings detected`。

Decision: 用 `avcodec_alloc_context3(codec)` 创建编码器 context；`preset/tune/x265-params`（软编）或 `gop/rc_mode/max_bit_rate/frame_rate`（硬编）放进 `Dictionary`，随 `avcodec_open2` 传入。

Consequences: libx264/libx265 正常；硬件编码器的未知选项不再通过 `av_opt_set` 静默忽略，而是由 open 处理，需在真实硬件上复测。

## ADR-010：滤镜图必须显式连接端点

Status: accepted (2026-11-26)

Context: `ffmpeg-next::filter::Graph::parse` 不带 in/out 端点，直接把 `buffer`/`buffersink` 加进图后 parse 会报 “Output pad default … not connected”。

Decision: 使用 `graph.output("in", 0)?.input("out", 0)?.parse(desc)?`，与 C++ 的 `avfilter_graph_parse_ptr(..., &inputs, &outputs, ...)` 对齐。

Consequences: 滤镜链 `scale=W:H,format=<pix>` 正常配置。

## ADR-011：默认优先硬件编码，打开失败回退软件

Status: accepted (2026-11-27)

Context: 部署容器使用 NVIDIA 4090（`runtime: nvidia`），但旧实现只调用 `avcodec_find_encoder`，在当前 FFmpeg 中返回 libx264/libx265，硬件编码器从未被选中。目标环境也可能使用 Ascend 或 QSV；无 GPU 的开发机需要继续可运行。

Decision: 默认按 h264_nvenc/hevc_nvenc → h264_ascend/hevc_ascend → h264_qsv/hevc_qsv → libx264/libx265 的顺序尝试。每个候选都完整配置并调用 avcodec_open2，失败时记录原因并继续；只有软件候选也失败才返回错误。VAAPI 需要显式创建硬件帧上下文，暂不列入自动候选。

Consequences: 有可用硬件时默认硬编；无硬件/驱动时自动回退软件并打印原因，不静默降级。NVENC 已在当前容器实测；Ascend/QSV 的成功编码路径仍需在对应真实设备上复测。

## ADR-012：RTSP PTS 使用帧率倒数时间基，写头后刷新输出时间基

Status: accepted (2026-11-27)

Context: Rust 移植把 `encoder.time_base` 设成了 `{fps_num, fps_den}`（如 20/1），而 C++ 是 `{fps_den, fps_num}`（1/20）。此外 RTSP/RTP muxer 在 `avformat_write_header` 时会把输出流时间基改成 1/90000，而代码在写头前缓存了输出时间基。两处叠加后，每帧 PTS 只前进 1 个 tick，实际间隔约 11 微秒；播放器报 `No video PTS`、POC 参考帧缺失并大量丢帧。

Decision: 编码器时间基固定为帧率的倒数；写头后重新读取 `AVStream.time_base`，再用 `av_packet_rescale_ts(encoder_time_base, output_time_base)`。E2E 增加 RTSP `pts_time` 间隔断言。

Consequences: 20fps 流的 PTS 间隔应为 0.05s（90000 时间基下 4500 tick）；H.264/H.265 E2E 均验证通过。mpv 对 HEVC 仍可能打印 `No video PTS`，该现象用 FFmpeg CLI 直接推流到 MediaMTX 也能复现，属于播放器/服务端组合问题，不是推流端时间基回归。
