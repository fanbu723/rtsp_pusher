extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavutil/opt.h>
#include <libavutil/log.h>
#include <libavutil/time.h>
#include <libavutil/imgutils.h>
#include <libswscale/swscale.h>
}

#include <iostream>
#include <string>
#include <cstdio>
#include <cstdlib>
#include <csignal>
#include <cstring>
#include <chrono>
#include <ctime>
#include "cxxopts.hpp"

// 全局标志，用于安全退出
static volatile int keep_running = 1;

void signal_handler(int sig) {
    if (sig == SIGINT || sig == SIGTERM) {
        keep_running = 0;
    }
}

// 取当前墙钟时间字符串 HH:MM:SS.mmm
static void format_time(char* buf, size_t n) {
    auto now = std::chrono::system_clock::now();
    std::time_t sec = std::chrono::system_clock::to_time_t(now);
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                  now.time_since_epoch()) % 1000;
    std::tm* tmv = std::localtime(&sec);
    snprintf(buf, n, "%02d:%02d:%02d.%03d",
             tmv->tm_hour, tmv->tm_min, tmv->tm_sec, (int)ms.count());
}

// 5x7 点阵字体：'0'-'9'、':'、'.'，每字符 7 行 × 5 bit（bit4 为最左像素）
static const uint8_t FONT_5X7[12][7] = {
    {0x0E,0x11,0x13,0x15,0x19,0x11,0x0E}, // 0
    {0x04,0x0C,0x04,0x04,0x04,0x04,0x0E}, // 1
    {0x0E,0x11,0x01,0x02,0x04,0x08,0x1F}, // 2
    {0x1F,0x02,0x04,0x02,0x01,0x11,0x0E}, // 3
    {0x02,0x06,0x0A,0x12,0x1F,0x02,0x02}, // 4
    {0x1F,0x10,0x1E,0x01,0x01,0x11,0x0E}, // 5
    {0x06,0x08,0x10,0x1E,0x11,0x11,0x0E}, // 6
    {0x1F,0x01,0x02,0x04,0x08,0x08,0x08}, // 7
    {0x0E,0x11,0x11,0x0E,0x11,0x11,0x0E}, // 8
    {0x0E,0x11,0x11,0x0F,0x01,0x02,0x0C}, // 9
    {0x00,0x0C,0x0C,0x00,0x0C,0x0C,0x00}, // :
    {0x00,0x00,0x00,0x00,0x0C,0x0C,0x00}, // .
};

// 在帧的 Y 平面上绘制实时时间戳水印（不依赖 drawtext 滤镜）。
// 适用于 yuv420p / nv12 等任何 Y 平面在最前、每像素 1 字节的格式。
static void draw_timestamp(AVFrame* frame, const char* text, int scale) {
    const int W = frame->width, H = frame->height;
    uint8_t* y = frame->data[0];
    const int ls = frame->linesize[0];
    const int len = (int)strlen(text);
    const int char_w = 5 * scale, char_h = 7 * scale;
    const int gap = scale;
    const int pad = 2 * scale;
    const int text_w = len * (char_w + gap) - gap;
    const int box_w = text_w + 2 * pad;
    const int box_h = char_h + 2 * pad;
    const int x0 = W - box_w - 4 * scale;   // 右上角
    const int y0 = 4 * scale;

    // 黑色背景框
    for (int yy = 0; yy < box_h; yy++) {
        const int py = y0 + yy;
        if (py < 0 || py >= H) continue;
        for (int xx = 0; xx < box_w; xx++) {
            const int px = x0 + xx;
            if (px < 0 || px >= W) continue;
            y[py * ls + px] = 16;
        }
    }

    // 逐字符绘制（白色）
    for (int c = 0; c < len; c++) {
        int glyph = -1;
        const char ch = text[c];
        if (ch >= '0' && ch <= '9') glyph = ch - '0';
        else if (ch == ':') glyph = 10;
        else if (ch == '.') glyph = 11;
        else continue;
        const int cx = x0 + pad + c * (char_w + gap);
        const int cy = y0 + pad;
        for (int r = 0; r < 7; r++) {
            const uint8_t bits = FONT_5X7[glyph][r];
            for (int col = 0; col < 5; col++) {
                if (!(bits & (0x10 >> col))) continue;
                for (int sy = 0; sy < scale; sy++) {
                    const int py = cy + r * scale + sy;
                    if (py < 0 || py >= H) continue;
                    for (int sx = 0; sx < scale; sx++) {
                        const int px = cx + col * scale + sx;
                        if (px < 0 || px >= W) continue;
                        y[py * ls + px] = 255;
                    }
                }
            }
        }
    }
}

// 错误检查宏
#define CHECK_ERR(expr, msg) \
    if ((expr) < 0) { \
        char errbuf[128]; \
        av_strerror(expr, errbuf, sizeof(errbuf)); \
        std::cerr << msg << ": " << errbuf << std::endl; \
        goto fail; \
    }

int main(int argc, char* argv[]) {
    // 注册信号处理（Ctrl+C 退出）
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    avformat_network_init();

    // 所有资源句柄统一在此声明，并在 fail 处统一安全释放。
    // 硬件编码器（如 Ascend h264_ascend）若未正确关闭就退出会导致崩溃。
    AVFormatContext* ifmt_ctx = nullptr;
    AVCodecContext* dec_ctx = nullptr;
    AVFormatContext* ofmt_ctx = nullptr;
    AVCodecContext* enc_ctx = nullptr;
    AVFilterGraph* filter_graph = nullptr;
    AVDictionary* options = nullptr;
    AVFilterInOut* inputs = nullptr;
    AVFilterInOut* outputs = nullptr;
    AVPacket* pkt = nullptr;
    AVPacket* enc_pkt = nullptr;
    AVFrame* frame = nullptr;
    AVFrame* filtered_frame = nullptr;
    bool header_written = false;
    bool is_hw_enc = false;              // 是否硬件编码器（h264_ascend 等）
    std::string scale_str, filter_desc;
    // 以下变量会被 goto fail 跨过，必须提前声明并初始化
    int video_stream_idx = -1;
    AVRational src_fps{0, 0};
    AVCodecParameters* in_codecpar = nullptr;
    const AVCodec* dec = nullptr;
    const AVCodec* enc = nullptr;
    AVStream* out_stream = nullptr;
    AVPixelFormat enc_pix_fmt = AV_PIX_FMT_NONE;
    AVFilterContext* buffersrc_ctx = nullptr;
    AVFilterContext* buffersink_ctx = nullptr;
    const AVFilter* buffersrc = nullptr;
    const AVFilter* buffersink = nullptr;
    AVRational tb{1, 25}, sar{1, 1};
    const char* pix_fmt_name = nullptr;
    // 推流统计（同样需提前声明，避免被 goto fail 跨过）
    int64_t start_time = 0;
    int64_t streamed_frames = 0;       // 已成功推流帧数
    int64_t decoded_frames = 0;        // 已解码帧数
    int64_t out_frame_count = 0;       // 已输出（编码）帧数，用于生成 PTS
    int64_t loop_count = 1;            // 播放轮次
    int64_t last_log_time = 0;         // 上次刷新日志的时间
    int64_t last_log_frames = 0;       // 上次刷新日志时的帧数

    // ---------- 0. 解析命令行参数 ----------
    cxxopts::Options opts("rtsp_pusher", "FFmpeg RTSP 推流工具（H.264，支持时间戳水印）");
    opts.add_options()
        ("i,input", "输入视频文件", cxxopts::value<std::string>()->default_value("o2.mp4"))
        ("u,url", "RTSP 输出地址", cxxopts::value<std::string>()->default_value("rtsp://127.0.0.1:8554/live3"))
        ("w,width", "输出宽度", cxxopts::value<int>()->default_value("2560"))
        ("height", "输出高度", cxxopts::value<int>()->default_value("1440"))
        ("f,fps", "目标帧率（0 表示自动使用输入帧率）", cxxopts::value<int>()->default_value("0"))
        ("t,transport", "RTSP 传输协议（tcp/udp）", cxxopts::value<std::string>()->default_value("tcp"))
        ("b,bitrate", "编码码率 kbps（0 表示 CRF 自动）", cxxopts::value<int>()->default_value("0"))
        ("no-realtime", "关闭实时速度控制（默认按实时速度推流）", cxxopts::value<bool>()->default_value("false"))
        ("no-watermark", "关闭实时时间戳水印（默认开启）", cxxopts::value<bool>()->default_value("false"))
        ("low-latency", "低延迟模式：减小 GOP 与编码缓冲，降低端到端延迟", cxxopts::value<bool>()->default_value("false"))
        ("v,verbose", "显示详细日志（默认只显示错误）", cxxopts::value<bool>()->default_value("false"))
        ("h,help", "显示帮助", cxxopts::value<bool>()->default_value("false"));

    std::string input_file, output_url, transport;
    int width = 2560, height = 1440;
    int fps_override = 0, bitrate_kbps = 0;
    bool no_realtime = false, verbose = false, low_latency = false;
    bool no_watermark = false;   // 时间戳水印开关（默认开启，--no-watermark 关闭）

    try {
        auto result = opts.parse(argc, argv);
        if (result["help"].as<bool>()) {
            std::cout << opts.help() << std::endl;
            return 0;
        }
        input_file = result["input"].as<std::string>();
        output_url = result["url"].as<std::string>();
        width = result["width"].as<int>();
        height = result["height"].as<int>();
        fps_override = result["fps"].as<int>();
        transport = result["transport"].as<std::string>();
        bitrate_kbps = result["bitrate"].as<int>();
        no_realtime = result["no-realtime"].as<bool>();
        no_watermark = result["no-watermark"].as<bool>();
        low_latency = result["low-latency"].as<bool>();
        verbose = result["verbose"].as<bool>();
    } catch (const cxxopts::exceptions::exception& e) {
        std::cerr << "参数解析错误: " << e.what() << std::endl;
        std::cout << opts.help() << std::endl;
        return 1;
    }

    // 日志级别：默认只显示错误，-v 显示详细信息
    if (!verbose) {
        av_log_set_level(AV_LOG_ERROR);
    }

    // 时间戳水印开关：默认开启，--no-watermark 可关闭
    const bool watermark = !no_watermark;

    std::cout << "输入文件: " << input_file << std::endl;
    std::cout << "输出地址: " << output_url << std::endl;
    std::cout << "输出尺寸: " << width << "x" << height << std::endl;
    std::cout << "时间戳水印: " << (watermark ? "已启用" : "已关闭") << std::endl;

    // ---------- 1. 打开输入 ----------
    CHECK_ERR(avformat_open_input(&ifmt_ctx, input_file.c_str(), nullptr, nullptr),
              "无法打开输入文件");
    // 增加探测时长与探测大小，确保能解析出像素格式等参数
    ifmt_ctx->max_analyze_duration = 5 * AV_TIME_BASE;
    ifmt_ctx->probesize = 20 * 1024 * 1024;
    CHECK_ERR(avformat_find_stream_info(ifmt_ctx, nullptr),
              "无法获取流信息");

    // 寻找视频流
    video_stream_idx = -1;
    for (unsigned int i = 0; i < ifmt_ctx->nb_streams; i++) {
        if (ifmt_ctx->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO) {
            video_stream_idx = i;
            break;
        }
    }
    if (video_stream_idx == -1) {
        std::cerr << "未找到视频流" << std::endl;
        goto fail;
    }

    // 获取输入视频真实帧率（用于时间戳与实时同步，
    // 避免硬编码 25fps 与文件帧率不匹配导致长时间后画面拖尾/卡顿）
    src_fps = av_guess_frame_rate(ifmt_ctx, ifmt_ctx->streams[video_stream_idx], nullptr);
    if (src_fps.num <= 0 || src_fps.den <= 0) {
        src_fps = {25, 1};
    }
    if (fps_override > 0) {
        src_fps = {fps_override, 1};
    }
    std::cout << "输入视频帧率: " << src_fps.num << "/" << src_fps.den << std::endl;

    in_codecpar = ifmt_ctx->streams[video_stream_idx]->codecpar;
    dec = avcodec_find_decoder(in_codecpar->codec_id);
    if (!dec) {
        std::cerr << "找不到解码器" << std::endl;
        goto fail;
    }
    dec_ctx = avcodec_alloc_context3(dec);
    avcodec_parameters_to_context(dec_ctx, in_codecpar);
    CHECK_ERR(avcodec_open2(dec_ctx, dec, nullptr), "打开解码器失败");

    // 兜底：若未探测到像素格式，默认按 YUV420P 处理（H.264 解码通常输出 4:2:0）
    if (dec_ctx->pix_fmt == AV_PIX_FMT_NONE) {
        dec_ctx->pix_fmt = AV_PIX_FMT_YUV420P;
    }

    // ---------- 2. 创建输出 RTSP 上下文 ----------
    CHECK_ERR(avformat_alloc_output_context2(&ofmt_ctx, nullptr, "rtsp", output_url.c_str()),
              "创建输出上下文失败");

    // 创建视频流
    out_stream = avformat_new_stream(ofmt_ctx, nullptr);
    if (!out_stream) {
        std::cerr << "创建输出流失败" << std::endl;
        goto fail;
    }
    out_stream->id = 0;

    // 编码器配置（H.264，超低延迟）
    // AV_CODEC_ID_HEVC
    enc = avcodec_find_encoder(AV_CODEC_ID_H264);
    if (!enc) {
        std::cerr << "找不到 H.264 编码器" << std::endl;
        goto fail;
    }
    enc_ctx = avcodec_alloc_context3(enc);
    enc_ctx->width = width;
    enc_ctx->height = height;

    // 选择编码器支持的像素格式：
    // - 优先 yuv420p（软编码器通用）
    // - 若编码器不支持（如 Ascend 硬编码器 h264_ascend 需要 NV12 等），
    //   则使用编码器声明支持的第一个格式，
    //   避免 "Specified pixel format yuv420p is invalid or not supported" 报错
    enc_pix_fmt = AV_PIX_FMT_NONE;
    if (enc->pix_fmts) {
        for (const AVPixelFormat* p = enc->pix_fmts; *p != AV_PIX_FMT_NONE; ++p) {
            if (enc_pix_fmt == AV_PIX_FMT_NONE) {
                enc_pix_fmt = *p;   // 兜底：取第一个支持的格式
            }
            if (*p == AV_PIX_FMT_YUV420P) {
                enc_pix_fmt = *p;
                break;
            }
        }
    }
    if (enc_pix_fmt == AV_PIX_FMT_NONE) {
        enc_pix_fmt = AV_PIX_FMT_YUV420P;
    }
    enc_ctx->pix_fmt = enc_pix_fmt;
    std::cout << "使用编码器: " << enc->name
              << "，像素格式: " << av_get_pix_fmt_name(enc_pix_fmt) << std::endl;

    enc_ctx->time_base = {src_fps.den, src_fps.num};  // 按输入帧率设置时间基
    enc_ctx->framerate = src_fps;                     // 与输入帧率一致

    // 码率设置：
    // - 用户指定 -b 时用指定值
    // - libx264 等软编未指定时走 CRF（bit_rate=0），画质优先
    // - 硬件编码器（h264_ascend 等）未指定时给默认码率：该插件依赖 bit_rate
    //   分配内部输出缓冲，为 0 时缓冲过小，会出现 "Ascend memory D2H failed, ret 507899"
    is_hw_enc = (strcmp(enc->name, "libx264") != 0);
    if (bitrate_kbps > 0) {
        enc_ctx->bit_rate = (int64_t)bitrate_kbps * 1000;
    } else if (is_hw_enc) {
        const double fps_d = src_fps.den ? (double)src_fps.num / src_fps.den : 25.0;
        int64_t def_bps = (int64_t)((double)width * height * fps_d * 0.15);
        if (def_bps < 2000000)  def_bps = 2000000;
        if (def_bps > 20000000) def_bps = 20000000;
        enc_ctx->bit_rate = def_bps;
        std::cout << "硬件编码器未指定码率，使用默认码率: "
                  << (def_bps / 1000) << " kbps（可用 -b 调整）" << std::endl;
    }

    // GOP：默认 25（约 1 秒），低延迟模式减到 10（约 0.4 秒），
    // 拉流端更快等到关键帧，降低延迟
    enc_ctx->gop_size = low_latency ? 10 : 25;
    enc_ctx->keyint_min = low_latency ? 10 : 25;
    enc_ctx->max_b_frames = 0;               // 无 B 帧（低延迟）
    enc_ctx->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    if (low_latency) {
        enc_ctx->flags |= AV_CODEC_FLAG_LOW_DELAY;   // 低延迟：减少编码器缓冲
    }
    // 硬件编码器缓冲：默认给约 1 秒（稳妥，避免 D2H 拷贝失败）；
    // 低延迟模式压缩到约 0.25 秒并放宽峰值码率，代价是更易出现缓冲不足
    if (is_hw_enc && enc_ctx->bit_rate > 0) {
        enc_ctx->rc_max_rate = low_latency ? enc_ctx->bit_rate * 2 : enc_ctx->bit_rate;
        enc_ctx->rc_buffer_size = low_latency ? enc_ctx->bit_rate / 4 : enc_ctx->bit_rate;
    }
    av_opt_set(enc_ctx, "preset", "ultrafast", 0);
    av_opt_set(enc_ctx, "tune", "zerolatency", 0);

    // h264_ascend 专属参数（软编会静默忽略）：
    // 从 ffmpeg -h encoder=h264_ascend 得知：插件默认 gop=30（≈1.3s@23fps）、
    // frame_rate=25、max_bit_rate=20000kbps，且编码器自带 delay。
    // 这里显式设置，避免插件用默认大 GOP 导致的高延迟。
    if (is_hw_enc) {
        // GOP：低延迟 10，否则 30（与插件默认一致）
        av_opt_set_int(enc_ctx, "gop", low_latency ? 10 : 30, 0);
        // 码控：0=CBR（直播推荐，码率平稳）
        av_opt_set_int(enc_ctx, "rc_mode", 0, 0);
        // CBR 平均码率 = 我们设置的 bit_rate（kbps），避免插件默认 20Mbps
        int64_t br_kbps = enc_ctx->bit_rate / 1000;
        if (br_kbps < 2)       br_kbps = 2;
        if (br_kbps > 614400)  br_kbps = 614400;
        av_opt_set_int(enc_ctx, "max_bit_rate", (int)br_kbps, 0);
        // 帧率与输入一致，避免插件默认 25 与输入 23 错位
        int fr = src_fps.den ? src_fps.num / src_fps.den : 25;
        if (fr < 1)   fr = 1;
        if (fr > 240) fr = 240;
        av_opt_set_int(enc_ctx, "frame_rate", fr, 0);
        if (verbose) {
            std::cout << "h264_ascend: gop=" << (low_latency ? 10 : 30)
                      << " rc_mode=CBR bitrate=" << br_kbps
                      << "kbps fps=" << fr << std::endl;
        }
    }

    if (low_latency) {
        std::cout << "低延迟模式已启用（GOP=10，缓冲≈0.25s）" << std::endl;
    }

    CHECK_ERR(avcodec_open2(enc_ctx, enc, nullptr), "打开编码器失败");

    // 拷贝编码参数到输出流
    avcodec_parameters_from_context(out_stream->codecpar, enc_ctx);
    out_stream->time_base = enc_ctx->time_base;

    // 设置 RTSP 传输协议
    av_dict_set(&options, "rtsp_transport", transport.c_str(), 0);
    // 设置 RTSP 握手/读写超时（微秒），避免服务器无响应时无限卡住
    av_dict_set(&options, "stimeout", "5000000", 0);   // 5 秒 socket 超时
    av_dict_set(&options, "timeout", "5000000", 0);    // 5 秒 IO 超时

    // ---------- 3. 构建滤镜图 ----------
    // 滤镜链只做缩放 + 转成编码器支持的像素格式；
    // 时间戳水印由程序直接绘制在 Y 平面（不依赖 FFmpeg 的 drawtext 滤镜，
    // 该滤镜在部分精简版 FFmpeg 中未编译，且 %{localtime} 无毫秒精度）
    scale_str = "scale=" + std::to_string(width) + ":" + std::to_string(height);
    pix_fmt_name = av_get_pix_fmt_name(enc_pix_fmt);
    filter_desc = scale_str + ",format=" + pix_fmt_name;
    std::cout << "滤镜链: " << filter_desc << std::endl;

    filter_graph = avfilter_graph_alloc();
    buffersrc_ctx = nullptr;
    buffersink_ctx = nullptr;

    buffersrc = avfilter_get_by_name("buffer");
    // 确保 time_base 和 SAR 有效
    tb = dec_ctx->time_base;
    if (tb.num <= 0 || tb.den <= 0) {
        tb = {1, 25};  // 默认时间基
    }
    sar = dec_ctx->sample_aspect_ratio;
    if (sar.num <= 0 || sar.den <= 0) {
        sar = {1, 1};  // 默认像素宽高比
    }

    char args[512];
    snprintf(args, sizeof(args),
            "video_size=%dx%d:pix_fmt=%d:time_base=%d/%d:pixel_aspect=%d/%d",
            dec_ctx->width, dec_ctx->height, dec_ctx->pix_fmt,
            tb.num, tb.den, sar.num, sar.den);
    CHECK_ERR(avfilter_graph_create_filter(&buffersrc_ctx, buffersrc, "in",
                                           args, nullptr, filter_graph),
              "创建 buffer 滤镜失败");

    buffersink = avfilter_get_by_name("buffersink");
    CHECK_ERR(avfilter_graph_create_filter(&buffersink_ctx, buffersink, "out",
                                           nullptr, nullptr, filter_graph),
              "创建 buffersink 滤镜失败");

    outputs = avfilter_inout_alloc();
    outputs->name = av_strdup("in");
    outputs->filter_ctx = buffersrc_ctx;
    outputs->pad_idx = 0;
    outputs->next = nullptr;

    inputs = avfilter_inout_alloc();
    inputs->name = av_strdup("out");
    inputs->filter_ctx = buffersink_ctx;
    inputs->pad_idx = 0;
    inputs->next = nullptr;

    CHECK_ERR(avfilter_graph_parse_ptr(filter_graph, filter_desc.c_str(),
                                       &inputs, &outputs, nullptr),
              "解析滤镜描述失败");
    CHECK_ERR(avfilter_graph_config(filter_graph, nullptr), "配置滤镜图失败");

    // 释放临时结构
    avfilter_inout_free(&inputs);
    avfilter_inout_free(&outputs);

    // ---------- 4. 打开输出并写头 ----------
    // CHECK_ERR(avio_open(&ofmt_ctx->pb, output_url, AVIO_FLAG_WRITE),
    //           "无法打开输出 URL");
    std::cout << "正在连接 RTSP 服务器: " << output_url << " ..." << std::endl;
    CHECK_ERR(avformat_write_header(ofmt_ctx, &options), "写头失败");
    header_written = true;
    av_dict_free(&options);
    std::cout << "RTSP 连接成功，开始推流（Ctrl+C 退出）..." << std::endl;

    // ---------- 5. 主循环 ----------
    pkt = av_packet_alloc();
    enc_pkt = av_packet_alloc();
    frame = av_frame_alloc();
    filtered_frame = av_frame_alloc();

    start_time = av_gettime();          // 用于实时同步
    last_log_time = av_gettime();       // 上次刷新日志的时间

    while (keep_running) {
        // 读取帧
        if (av_read_frame(ifmt_ctx, pkt) < 0) {
            // 循环播放：seek 到开头
            av_seek_frame(ifmt_ctx, -1, 0, AVSEEK_FLAG_BACKWARD);
            loop_count++;
            continue;
        }

        if (pkt->stream_index != video_stream_idx) {
            av_packet_unref(pkt);
            continue;
        }

        // 解码
        int ret = avcodec_send_packet(dec_ctx, pkt);
        if (ret < 0) {
            av_packet_unref(pkt);
            continue;
        }

        while (ret >= 0) {
            ret = avcodec_receive_frame(dec_ctx, frame);
            if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) break;
            if (ret < 0) break;

            decoded_frames++;

            // 实时速度控制（-re 效果）：
            // 不依赖文件 PTS（o2.mp4 的 PTS 从 5120 秒起，异常），
            // 改为按解码帧计数固定 25fps（40ms/帧）推进
            int64_t now = av_gettime();
            // 按真实帧率计算单帧间隔（微秒）
            int64_t frame_interval_us = (int64_t)1000000 * src_fps.den / src_fps.num;
            int64_t target = start_time + (decoded_frames - 1) * frame_interval_us;
            int64_t delay = target - now;
            if (delay > 0 && !no_realtime) {
                av_usleep(delay);
            }

            // 送入滤镜
            if (av_buffersrc_write_frame(buffersrc_ctx, frame) < 0) break;

            // 取出滤镜后的帧
            while (true) {
                ret = av_buffersink_get_frame(buffersink_ctx, filtered_frame);
                if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) break;
                if (ret < 0) break;

                // 实时时间戳水印（含毫秒，直接画在 Y 平面，不依赖 drawtext 滤镜）
                if (watermark) {
                    char ts[32];
                    format_time(ts, sizeof(ts));
                    draw_timestamp(filtered_frame, ts, 2);
                }

                // 设置 PTS：按输出帧序号从 0 开始递增（在 enc_ctx->time_base 下）
                filtered_frame->pts = out_frame_count++;

                // 编码
                ret = avcodec_send_frame(enc_ctx, filtered_frame);
                if (ret < 0) {
                    av_frame_unref(filtered_frame);
                    break;
                }

                while (ret >= 0) {
                    ret = avcodec_receive_packet(enc_ctx, enc_pkt);
                    if (ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) break;
                    if (ret < 0) break;

                    // 调整时间基并推流
                    av_packet_rescale_ts(enc_pkt, enc_ctx->time_base,
                                         out_stream->time_base);
                    enc_pkt->stream_index = out_stream->index;

                    ret = av_interleaved_write_frame(ofmt_ctx, enc_pkt);
                    if (ret >= 0) {
                        streamed_frames++;   // 统计成功推流的帧
                    } else {
                        char errbuf[128];
                        av_strerror(ret, errbuf, sizeof(errbuf));
                        fprintf(stderr, "\n[错误] 写入 RTSP 失败: %s\n", errbuf);
                    }
                    av_packet_unref(enc_pkt);
                    if (ret < 0) break;
                }
                av_frame_unref(filtered_frame);
            }
            av_frame_unref(frame);
        }

        // 实时刷新推流进度（约每秒一次，\r 覆盖式显示）
        int64_t now = av_gettime();
        if (now - last_log_time >= 1000000) {
            double elapsed = (now - start_time) / 1e6;
            double fps = (streamed_frames - last_log_frames) /
                         ((now - last_log_time) / 1e6);
            char tbuf[32];
            format_time(tbuf, sizeof(tbuf));
            printf("\r%s [推流中] 已解码: %lld 帧 | 已推流: %lld 帧 | 第 %lld 轮 | 实时帧率: %.2f fps | 运行时长: %.1f s   ",
                   tbuf, (long long)decoded_frames, (long long)streamed_frames,
                   (long long)loop_count, fps, elapsed);
            fflush(stdout);
            last_log_time = now;
            last_log_frames = streamed_frames;
        }

        av_packet_unref(pkt);
    }

    // 退出时打印最终统计
    {
        char tbuf[32];
        format_time(tbuf, sizeof(tbuf));
        double elapsed = (av_gettime() - start_time) / 1e6;
        printf("\n%s [结束] 共推流 %lld 帧 | 播放 %lld 轮 | 运行时长 %.1f s\n",
               tbuf, (long long)streamed_frames, (long long)loop_count, elapsed);
    }

    // ---------- 6. 清理 ----------
    // 写尾（对 RTSP 可能无实际作用，但安全）
    if (header_written) {
        av_write_trailer(ofmt_ctx);
    }

fail:
    // 统一安全清理：错误路径也会走到这里，
    // 确保硬件编码器（h264_ascend 等）被正确关闭后再退出
    av_packet_free(&pkt);
    av_packet_free(&enc_pkt);
    av_frame_free(&frame);
    av_frame_free(&filtered_frame);
    avfilter_graph_free(&filter_graph);
    avcodec_free_context(&dec_ctx);
    avcodec_free_context(&enc_ctx);
    av_dict_free(&options);
    avformat_close_input(&ifmt_ctx);
    if (ofmt_ctx) {
        if (ofmt_ctx->pb) {
            avio_close(ofmt_ctx->pb);
        }
        avformat_free_context(ofmt_ctx);
    }
    avfilter_inout_free(&inputs);
    avfilter_inout_free(&outputs);
    avformat_network_deinit();

    std::cout << "推流结束。" << std::endl;
    return 0;
}