# 交接说明

## 当前状态

- 分支：`refactor/rust`
- C++ 源文件、cxxopts、CMake 已删除；`build.sh` 构建 Rust。
- Rust crate：`rust/`，二进制输出到仓库根目录 `./rtsp_pusher`。
- 目标 FFmpeg 6.1（libavcodec 60）；`ffmpeg-next 6.1.1` / `ffmpeg-sys-next 6.1.0`。
- 重构提交：`refactor: 用 Rust 重写 RTSP 推流工具`；具体 hash 见 git log。
- 编码器选择：默认 NVENC → Ascend → QSV → libx264/libx265；硬件打开失败记录后继续，最终回退软件。

## 构建

```bash
# 首次构建（无 Rust 或 cargo < 1.85）：工具链装到仓库内 .cargo-home/.rustup-home
bash scripts/install-rust.sh
bash build.sh

# 普通环境（已装 libav*-dev 6.1）：直接构建
bash build.sh

# 头文件不在 pkg-config 默认路径时
export FFMPEG_INCLUDE_DIR=/path/to/ffmpeg-6.1/include
bash build.sh

# 未安装 6.1 开发头文件时，scripts/ffmpeg-env.sh 会从官方仓库
# 拉取 n6.1.1 源码到 .ffmpeg-src/ 并 configure；无需其他项目预置目录。
```

`build.sh` 会运行 `scripts/ffmpeg-env.sh`，必要时生成 `.ffmpeg-sysroot`（gitignore），然后
`cargo build --release` 并把二进制复制到根目录。

容器内以 root 执行时，`scripts/repo-user.sh` 会切换到仓库所有者 UID，避免产物变成 root 所有；
`scripts/rust-env.sh` 优先使用仓库内 `.cargo-home`/`.rustup-home`（由 `install-rust.sh` 安装，
不随容器重建丢失），其次才查找 `/home/*/.cargo`、`/usr/local/cargo`；cargo 版本低于
`rust/Cargo.toml` 的 `rust-version` 时直接提示安装，不再报 `edition2024`。

`ffmpeg-env.sh` 链接时附加 `-Wl,--allow-shlib-undefined`：FFmpeg 6.1 共享库自身依赖的
`x264/x265/libva/libnpp/fdk-aac` 不一定在链接器默认搜索路径（如 `/app/libs`、`/usr/local/cuda/lib64`）。
该选项只放宽共享库内部符号检查，运行期仍由动态加载器解析，二进制需 `ldd` 无 `not found`。
容器内缺少 libclang 时先安装：

```bash
apt install -y libclang-dev
```

## 验证命令

```bash
bash scripts/test-rust.sh          # fmt + clippy + 单测，自动探测 FFmpeg 6.1
bash scripts/e2e.sh                # 需 rtsp_pusher 容器/mediamtx + ffmpeg/ffprobe
bash scripts/record-reference.sh   # 需 rtsp_pusher 容器 + mediamtx
```

## 实测结果（2026-11-26 至 2026-11-27）

- `cargo test`：24 个单元测试 + 5 个 CLI 集成测试全部通过。
- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`：通过。
- `scripts/e2e.sh`：8/8 通过；H.264/H.265 均新增 RTSP PTS 间隔断言（25fps 下 0.04s）。
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
- 硬件优先选择（2026-11-27）：默认 Docker 容器 `runtime: nvidia` 下 `h264_nvenc`/`hevc_nvenc` 已实测；无 GPU 环境下依次尝试并回退 libx264 的路径也已验证；Ascend/QSV 未验证。
- 构建工具链（2026-11-27）：仓库内 cargo 1.85.1 在 u22.04 宿主机完整编译通过，产物 `GLIBC` 需求最高 2.34；cargo 1.75 会因 `clap_lex` 的 `edition2024` 失效，现已由 `rust-env.sh` 提前拦截。容器内 `bash build.sh` 复测待执行。
- 时间基/PTS 修复（2026-11-27）：编码器时间基改为 `1/fps`，并在 RTSP 写头后重新读取 1/90000 输出时间基。20fps 流 `ffprobe` 的 `pts_time` 间隔从约 0.000011s 恢复为 0.05s；`scripts/e2e.sh` 已加回归断言。详见 ADR-012。
- 吞吐（`--no-realtime`，仅记录不作为门禁，受机器负载影响）：C++ H.264 ≈1960fps、H.265 ≈386fps；Rust H.264 约 800–2100fps、H.265 约 230–255fps。实时 25fps 均不受影响。

## 复现参考基线

C++ 二进制复制在 `target/reference/rtsp_pusher_cpp`（gitignore）。容器内运行：

```bash
docker exec rtsp_pusher bash -lc \
  'cd /home/dongfan/project/rtsp_pusher && ./target/reference/rtsp_pusher_cpp -h'
```

## 残余风险

- **mpv + HEVC + MediaMTX**：mpv 0.37 播放本机 MediaMTX 的 HEVC 流时仍可能打印 `Could not find ref with POC`、`No video PTS`；用 FFmpeg CLI 直接推同样的流也能复现，H.264 则正常。播放 H.265 可用 `mpv --no-correct-pts ...`，或改用 H.264。
- **硬件编码器**：默认优先选择已接入；NVENC 在容器内已实测可用，Ascend/QSV 的成功路径未在真实设备验证。私有选项按编码器族区分，Ascend 使用 gop/rc_mode/max_bit_rate/frame_rate，NVENC/QSV 使用通用字段。
- **FFmpeg 版本**：目标 6.1；只有 4.4 运行库的环境不能直接运行新二进制（容器通过 `/app/ffmpeg/lib` 提供 6.1）。
- **写流失败语义**：保持 C++ 的“记录后继续、退出 0”，与旧 README 的笼统描述不同；README 已修正。
- **性能**：Rust H.265 软编吞吐低于 C++（约 60%），但实时 25fps 有充足余量；如追求吞吐可再 profile 滤镜/包处理路径。
- **rust-anything**：C++ 不在其支持语言矩阵。5.0.1 安装成功，但 `analyze .` 误判为 TypeScript/JavaScript 并报告 0 units，因此未使用 `gate/diff` 作为门禁；`rules .` 可读取 `rust/RULES.md`。相关记录见 `DECISIONS.md` ADR-006。

## 回退

`main`（`bca5151`）保留完整 C++ 实现；`git switch main` 即可回到旧版本。
