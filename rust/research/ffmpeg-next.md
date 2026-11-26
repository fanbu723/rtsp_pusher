# ffmpeg-next 6.1.1 / ffmpeg-sys-next 6.1.0 调研

调研方式：直接下载 crates.io 的 `.crate` 源码并阅读实现（本机 docs.rs 不可达），再用 C harness 验证 FFmpeg 6.1 行为。

## 版本与构建

- `ffmpeg-next 6.1.1` 依赖 `ffmpeg-sys-next 6.1.0`；后者自动探测 FFmpeg 版本，支持 `ffmpeg_6_1` 条件编译。
- `ffmpeg-sys-next` 支持 `FFMPEG_DIR`：设置后直接用 `<dir>/lib` 与 `<dir>/include`，不走 pkg-config。本项目用它指向生成的 sysroot。
- `ffmpeg-sys-next` 用 bindgen 0.64 + libclang；Ubuntu 上需要 libclang 运行库，必要时设置 `LIBCLANG_PATH`。
- `ffmpeg-next` 默认 feature 含 `device`；本项目不需要 avdevice，关闭默认 feature，只启用 codec/filter/format/software-scaling/software-resampling。

## 已用到的安全 API

- `format::context::Input/Output`、`format::Stream`、`codec::Context`、`decoder::Video`、`encoder::Video`、`filter::Graph`、`Packet`、`frame::Video`、`Dictionary`、`time`。
- 解码：`send_packet` / `receive_frame`；编码：`send_frame` / `receive_packet`。
- 帧 Y 平面：`frame::Video::data_mut(0)` + `stride(0)`，无需直接操作 `AVFrame`。
- 包时间基：`Packet::rescale_ts`、`set_stream`、`write_interleaved`。
- 信号：`signal-hook::flag::register`（注意：它把 flag 置 **true**，语义是“收到信号”，不是“继续运行”）。

## 必须走 FFI 的缺口（`rust/src/ffmpeg_ext.rs`）

| 缺口 | 原因 |
|---|---|
| 带 `max_analyze_duration`/`probesize` 的输入打开 | `format::input` 在返回前已调用 `find_stream_info` |
| 只分配不 `avio_open` 的 RTSP 输出上下文 | `format::output_as` 会提前 `avio_open`，无法把 RTSP 选项留到写头 |
| `av_guess_frame_rate` | 安全层未暴露 |
| 编码器 `pix_fmts` 列表 | 安全层未暴露 `AVCodec.pix_fmts` |
| `keyint_min` / `rc_max_rate` / `rc_buffer_size` 字段 | 安全层只有部分 setter |
| 用 codec 分配 `AVCodecContext` | `Context::new()` 传 NULL，FFmpeg 6.1 下 `priv_data` 不初始化 |
| `av_read_frame` / `av_seek_frame` | 需要按 C++ 方式复用 packet、EOF 后 seek |
| `av_packet_unref` / `av_frame_unref` | 保持 C++ 的复用与释放节奏 |
| `AVStream.id = 0` | 安全层无 setter |
| `av_log_set_level` | 直接用 FFI 更简单 |

## FFmpeg 6.1 语义陷阱（已实测）

1. **`stimeout` 已移除**：6.1 的 RTSP 只接受 `timeout`（微秒）；设置 `stimeout` 会得到 “Unrecognized option”。
2. **NULL codec 分配 + open 时传 codec**：`avcodec_alloc_context3(NULL)` 后 `avcodec_open2(ctx, codec, &dict)` 不会正确初始化 `priv_data`，`preset/tune` 被忽略，libx264 报 “broken ffmpeg default settings detected”。必须在 `avcodec_alloc_context3(codec)` 时传入 codec。
3. **私有选项**：`preset/tune/x265-params` 以及硬件编码器的 `gop/rc_mode/max_bit_rate/frame_rate` 应放进 `AVDictionary` 随 `avcodec_open2` 传入；open 前 `av_opt_set` 在 6.1 下不可靠。
4. **滤镜图端点**：`avfilter_graph_parse_ptr` 需要显式传入 buffer/buffersink 的 `AVFilterInOut`；`ffmpeg-next::Graph::parse` 不带端点，必须用 `graph.output("in", 0)?.input("out", 0)?.parse(...)`。
5. **`Source::add` 帧所有权**：`av_buffersrc_add_frame` 会消费帧；本项目送入后立即不再使用该解码帧，行为与 C++ `av_buffersrc_write_frame` + `av_frame_unref` 等价。

## 结论

FFmpeg 6.1 与 `ffmpeg-next 6.1.1` 足以覆盖原工具所需能力；风险集中在私有选项与版本差异，已用 ADR/规则固定。
