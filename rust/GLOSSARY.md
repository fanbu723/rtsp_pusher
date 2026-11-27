# 术语表

| 术语 | 含义 | Rust 名称 |
|---|---|---|
| 输入帧率 | 从输入流探测的帧率；`-f` 可覆盖；非法时回落 25/1 | `source_fps: Rational` |
| 输出时间基 | 编码器时间基，等于输入帧率倒数；用于 PTS | `encoder_time_base` / `output_time_base` |
| 输出帧序号 | 从 0 开始、每编码一帧加一，直接作为 PTS | `output_frame_count` |
| 实时节奏 | 按已解码帧数与起始时间控制 sleep，近似 `-re` | `pacing::sleep_us` |
| 循环轮次 | 输入 EOF 后 seek 回开头并加一 | `loop_count` |
| 水印 | 右上角 5x7 点阵实时时间戳，黑底白字 | `watermark::draw_timestamp` |
| 硬件编码器 | 名称不是 `libx264`/`libx265` 的编码器；默认候选为 NVENC、Ascend、QSV，有默认码率与 RC 逻辑 | `is_hardware` / `settings::encoder_candidates` |
| 软件编码器 | `libx264` 或 `libx265`；未指定码率时走 CRF | `settings::is_software_encoder` |
| 写流失败 | `av_interleaved_write_frame` 返回错误；记录后继续 | `encoded_packet.write_interleaved` 错误分支 |
| FFmpeg 6.1 | 目标运行库（libavcodec 60 / libavformat 60 / libavfilter 9） | `ffmpeg-next 6.1.1` |
