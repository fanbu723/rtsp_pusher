# 移植规则

这些是本次重构中确认、后续修改必须继续满足的规则。

- CLI 默认值、短/长参数、`-h`、非法 codec 的退出码必须与 C++ 一致；非法 codec 只打印错误，不打印“推流失败。”。
- 初始化失败、seek 失败返回 1；正常 SIGINT/SIGTERM 返回 0。
- 写帧失败只记录 `[错误] 写入 RTSP 失败: ...` 并继续，最终退出码保持 0（ADR-005）。
- 水印位置/颜色/字体表/裁剪必须与 C++ 的 `draw_timestamp` 一致；时间格式 `HH:MM:SS.mmm`，本地时区。
- 编码参数公式不得改动：软编默认 CRF、硬编默认 2–20 Mbps、GOP 25/10、无 B 帧、低延迟 RC 比例。
- 默认编码器按 NVENC → Ascend → QSV → libx264/libx265 的顺序选择；硬件打开失败只记录并继续，最终回退软件编码器；VAAPI 不自动选择（需要额外硬件帧上下文）。
- 输入探测参数保持 `max_analyze_duration=5s`、`probesize=20MB`。
- 只设置 FFmpeg 6.1 支持的 RTSP `timeout=5000000`，不要重新引入 `stimeout`。
- 编码器私有选项必须随 `avcodec_open2` 的 dictionary 传入；不要用 open 前的 `av_opt_set`。
- 编码器 `time_base` 必须是帧率的倒数（`{fps_den, fps_num}`），不能直接写成 `{fps_num, fps_den}`；RTSP 写头后必须重新读取输出流的 `time_base`，再执行 `av_packet_rescale_ts`。
- 滤镜图必须显式 `output("in", 0)` / `input("out", 0)` 连接端点。
- 不修改录制结果/参考日志来迁就实现；发现差异先修实现或记录到 DECISIONS。
- 不删除 `videos/`、`.agents/` 等用户未跟踪内容；不提交除非用户明确要求。
