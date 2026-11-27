# Rust 移植地图

目标：把 `rtsp_pusher` 从 C++/FFmpeg 4.4 单文件工具等价重构为 Rust/FFmpeg 6.1，保持 CLI、推流行为、日志结构、退出码与部署方式。

状态：**单单元 whole-app 移植已完成，默认硬件优先选择已接入**（NVENC 已实测，Ascend/QSV 未验证）。C++ 已删除；`main` 分支仍保留原实现。

## 单元

| 单元 | 状态 | 位置 | 证据 |
|---|---|---|---|
| CLI 契约 | done | `rust/src/cli.rs` | `cargo test` 的 5 个 CLI 集成用例 + `app-cases/cli.md` |
| 时间戳格式/水印 | done | `rust/src/watermark.rs` | 5 个像素级单测 + E2E 水印开/关 + C++/Rust 帧结构对比 |
| 编码器参数 | done | `rust/src/settings.rs` | 9 个单测覆盖码率/GOP/像素格式/编码器候选顺序/硬编码分支 |
| 实时节奏 | done | `rust/src/pacing.rs` | 3 个单测 + 实时模式 25fps E2E |
| FFmpeg 输入/解码 | done | `rust/src/ffmpeg_ext.rs`, `pipeline.rs` | H.264/H.265 E2E、无视频流失败路径 |
| 滤镜/水印/编码/RTSP | done | `rust/src/pipeline.rs` | H.264/H.265/UDP `ffprobe` 属性、循环轮次、C++ 对比 |
| 时间基/PTS | done | `rust/src/pipeline.rs` | 2 个单测 + E2E `pts_time` 间隔断言；ADR-012 |
| 信号与退出码 | done | `pipeline.rs` | SIGINT 退出 0、SIGTERM 退出 0、不可达 RTSP 退出 1 |
| 硬件编码器（NVENC/Ascend/QSV） | NVENC 已验证 | `pipeline.rs` | 默认顺序尝试；容器内 `h264_nvenc`/`hevc_nvenc` 实测，Ascend/QSV 未验证 |

## 检查点

1. 计划批准（分支/FFmpeg 版本/C++ 去留/写失败语义）。
2. Rust 单测 + CLI 测试通过。
3. H.264/H.265/水印/信号/失败路径 E2E 通过；C++ 参考日志已录制。
4. 删除 C++、更新 README、输出审核包。

## 未决问题

- 硬件编码器按 NVENC → Ascend → QSV → 软件顺序尝试；NVENC 已在容器内实测，Ascend/QSV 的成功路径仍无真实设备验证。
- mpv 0.37 + MediaMTX 播放 HEVC 时仍可能报 `No video PTS`/POC 缺失；FFmpeg CLI 推流可复现，H.264 正常，见 HANDOFF。
- Rust 在 `--no-realtime` 下 H.264 吞吐与 C++ 相当到略低，H.265 约低 35–40%（见 HANDOFF）；实时 25fps 不受影响。
- C++ 写帧失败仍返回 0 的历史行为已按用户确认保留。

## 关联文件

- 决策：`DECISIONS.md`
- 术语：`GLOSSARY.md`
- 交接与验证命令：`HANDOFF.md`
- 规则：`RULES.md`
- 依赖调研：`research/ffmpeg-next.md`
- 用例：`app-cases/cli.md`、`app-cases/reference/`
