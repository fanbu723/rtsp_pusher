# 交接说明

## 当前状态

- 分支：`refactor/rust`
- C++ 源文件、cxxopts、CMake 已删除；`build.sh` 构建 Rust。
- Rust crate：`rust/`，二进制输出到仓库根目录 `./rtsp_pusher`。
- 目标 FFmpeg 6.1（libavcodec 60）；`ffmpeg-next 6.1.1` / `ffmpeg-sys-next 6.1.0`。
- 重构提交：`refactor: 用 Rust 重写 RTSP 推流工具`；具体 hash 见 git log。

## 构建

```bash
# 普通环境（已装 libav*-dev 6.1）：直接构建
bash build.sh

# 头文件不在 pkg-config 默认路径时
export FFMPEG_INCLUDE_DIR=/path/to/ffmpeg-6.1/include
bash build.sh

# 本机：scripts/ffmpeg-env.sh 会自动从运行中的 rtsp_pusher 容器
# 查询 /app/ffmpeg 的挂载源；无需手动设置。
```

`build.sh` 会运行 `scripts/ffmpeg-env.sh`，必要时生成 `.ffmpeg-sysroot`（gitignore），然后
`cargo build --release` 并把二进制复制到根目录。

## 验证命令

```bash
bash scripts/test-rust.sh          # fmt + clippy + 单测，自动探测 FFmpeg 6.1
bash scripts/e2e.sh                # 需 rtsp_pusher 容器/mediamtx + ffmpeg/ffprobe
bash scripts/record-reference.sh   # 需 rtsp_pusher 容器 + mediamtx
```

## 实测结果（2026-11-26）

- `cargo test`：21 个单元测试 + 5 个 CLI 集成测试全部通过。
- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`：通过。
- `scripts/e2e.sh`：8/8 通过。
  - H.264：`codec_name=h264, width=640, height=360, avg_frame_rate=25/1`，SIGTERM 退出 0，多轮循环。
  - H.265：`codec_name=hevc`，同上尺寸/帧率。
  - UDP 传输：同样通过（`-t udp`，`ffprobe -rtsp_transport udp`）。
  - 水印开启：Y 平面 16 背景 + 255 字形；关闭：区域无 16/255。
  - SIGINT：退出 0，日志含 `[结束]` 与“推流结束。”。
  - 不可达 RTSP：8 秒内退出 1。
  - 无视频流：退出 1，提示“未找到视频流”。
- 与 C++ 参考对比（同输入/同 640x360/25fps）：
  - `ffprobe` 属性完全一致（h264 / hevc、640x360、25/1）。
  - 水印 16 背景 bbox 均为 `(482,8)-(631,29)`；255 字形 bbox 起点均为 `(488,12)`，末端因捕获时刻的秒数不同而相差 1–2 像素。
  - C++ 日志/退出码见 `rust/app-cases/reference/`。
- 实时模式：默认节奏 ≈25fps（实测 25.0–25.9），`--low-latency -b 4000` 正常出流。
- 吞吐（`--no-realtime`，仅记录不作为门禁，受机器负载影响）：C++ H.264 ≈1960fps、H.265 ≈386fps；Rust H.264 约 800–2100fps、H.265 约 230–255fps。实时 25fps 均不受影响。

## 复现参考基线

C++ 二进制复制在 `target/reference/rtsp_pusher_cpp`（gitignore）。容器内运行：

```bash
docker exec rtsp_pusher bash -lc \
  'cd /home/dongfan/project/rtsp_pusher && ./target/reference/rtsp_pusher_cpp -h'
```

## 残余风险

- **硬件编码器**（Ascend 等）分支未在真实硬件验证；私有选项改为随 `avcodec_open2` 传入，需现场复测。
- **FFmpeg 版本**：目标 6.1；只有 4.4 运行库的环境不能直接运行新二进制（容器通过 `/app/ffmpeg/lib` 提供 6.1）。
- **写流失败语义**：保持 C++ 的“记录后继续、退出 0”，与旧 README 的笼统描述不同；README 已修正。
- **性能**：Rust H.265 软编吞吐低于 C++（约 60%），但实时 25fps 有充足余量；如追求吞吐可再 profile 滤镜/包处理路径。
- **rust-anything**：C++ 不在其支持语言矩阵。5.0.1 安装成功，但 `analyze .` 误判为 TypeScript/JavaScript 并报告 0 units，因此未使用 `gate/diff` 作为门禁；`rules .` 可读取 `rust/RULES.md`。相关记录见 `DECISIONS.md` ADR-006。

## 回退

`main`（`bca5151`）保留完整 C++ 实现；`git switch main` 即可回到旧版本。
