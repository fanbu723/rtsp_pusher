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
#include "cxxopts.hpp"

// 全局标志，用于安全退出
static volatile int keep_running = 1;

void signal_handler(int sig) {
    if (sig == SIGINT || sig == SIGTERM) {
        keep_running = 0;
    }
}

// 错误检查宏
#define CHECK_ERR(expr, msg) \
    if ((expr) < 0) { \
        char errbuf[128]; \
        av_strerror(expr, errbuf, sizeof(errbuf)); \
        std::cerr << msg << ": " << errbuf << std::endl; \
        return -1; \
    }

int main(int argc, char* argv[]) {
    // 注册信号处理（Ctrl+C 退出）
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    avformat_network_init();

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
        ("v,verbose", "显示详细日志（默认只显示错误）", cxxopts::value<bool>()->default_value("false"))
        ("h,help", "显示帮助", cxxopts::value<bool>()->default_value("false"));

    std::string input_file, output_url, transport;
    int width = 2560, height = 1440;
    int fps_override = 0, bitrate_kbps = 0;
    bool no_realtime = false, verbose = false;

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

    std::cout << "输入文件: " << input_file << std::endl;
    std::cout << "输出地址: " << output_url << std::endl;
    std::cout << "输出尺寸: " << width << "x" << height << std::endl;

    // ---------- 1. 打开输入 ----------
    AVFormatContext* ifmt_ctx = nullptr;
    CHECK_ERR(avformat_open_input(&ifmt_ctx, input_file.c_str(), nullptr, nullptr),
              "无法打开输入文件");
    // 增加探测时长与探测大小，确保能解析出像素格式等参数
    ifmt_ctx->max_analyze_duration = 5 * AV_TIME_BASE;
    ifmt_ctx->probesize = 20 * 1024 * 1024;
    CHECK_ERR(avformat_find_stream_info(ifmt_ctx, nullptr),
              "无法获取流信息");

    // 寻找视频流
    int video_stream_idx = -1;
    for (unsigned int i = 0; i < ifmt_ctx->nb_streams; i++) {
        if (ifmt_ctx->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO) {
            video_stream_idx = i;
            break;
        }
    }
    if (video_stream_idx == -1) {
        std::cerr << "未找到视频流" << std::endl;
        return -1;
    }

    // 获取输入视频真实帧率（用于时间戳与实时同步，
    // 避免硬编码 25fps 与文件帧率不匹配导致长时间后画面拖尾/卡顿）
    AVRational src_fps = av_guess_frame_rate(ifmt_ctx, ifmt_ctx->streams[video_stream_idx], nullptr);
    if (src_fps.num <= 0 || src_fps.den <= 0) {
        src_fps = {25, 1};
    }
    if (fps_override > 0) {
        src_fps = {fps_override, 1};
    }
    std::cout << "输入视频帧率: " << src_fps.num << "/" << src_fps.den << std::endl;

    AVCodecParameters* in_codecpar = ifmt_ctx->streams[video_stream_idx]->codecpar;
    const AVCodec* dec = avcodec_find_decoder(in_codecpar->codec_id);
    if (!dec) {
        std::cerr << "找不到解码器" << std::endl;
        return -1;
    }
    AVCodecContext* dec_ctx = avcodec_alloc_context3(dec);
    avcodec_parameters_to_context(dec_ctx, in_codecpar);
    CHECK_ERR(avcodec_open2(dec_ctx, dec, nullptr), "打开解码器失败");

    // 兜底：若未探测到像素格式，默认按 YUV420P 处理（H.264 解码通常输出 4:2:0）
    if (dec_ctx->pix_fmt == AV_PIX_FMT_NONE) {
        dec_ctx->pix_fmt = AV_PIX_FMT_YUV420P;
    }

    // ---------- 2. 创建输出 RTSP 上下文 ----------
    AVFormatContext* ofmt_ctx = nullptr;
    CHECK_ERR(avformat_alloc_output_context2(&ofmt_ctx, nullptr, "rtsp", output_url.c_str()),
              "创建输出上下文失败");

    // 创建视频流
    AVStream* out_stream = avformat_new_stream(ofmt_ctx, nullptr);
    if (!out_stream) {
        std::cerr << "创建输出流失败" << std::endl;
        return -1;
    }
    out_stream->id = 0;

    // 编码器配置（H.264，超低延迟）
    // AV_CODEC_ID_HEVC
    const AVCodec* enc = avcodec_find_encoder(AV_CODEC_ID_H264);
    if (!enc) {
        std::cerr << "找不到 H.264 编码器" << std::endl;
        return -1;
    }
    AVCodecContext* enc_ctx = avcodec_alloc_context3(enc);
    enc_ctx->width = width;
    enc_ctx->height = height;
    enc_ctx->pix_fmt = AV_PIX_FMT_YUV420P;
    enc_ctx->time_base = {src_fps.den, src_fps.num};  // 按输入帧率设置时间基
    enc_ctx->framerate = src_fps;                     // 与输入帧率一致
    if (bitrate_kbps > 0) {
        enc_ctx->bit_rate = bitrate_kbps * 1000;      // 指定码率，否则用 CRF 自动
    }
    enc_ctx->gop_size = 25;
    enc_ctx->keyint_min = 25;
    enc_ctx->max_b_frames = 0;               // 无 B 帧（低延迟）
    enc_ctx->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    av_opt_set(enc_ctx, "preset", "ultrafast", 0);
    av_opt_set(enc_ctx, "tune", "zerolatency", 0);
    av_opt_set(enc_ctx, "rc_buffer_size", "0", 0);

    CHECK_ERR(avcodec_open2(enc_ctx, enc, nullptr), "打开编码器失败");

    // 拷贝编码参数到输出流
    avcodec_parameters_from_context(out_stream->codecpar, enc_ctx);
    out_stream->time_base = enc_ctx->time_base;

    // 设置 RTSP 传输协议
    AVDictionary* options = nullptr;
    av_dict_set(&options, "rtsp_transport", transport.c_str(), 0);
    // 设置 RTSP 握手/读写超时（微秒），避免服务器无响应时无限卡住
    av_dict_set(&options, "stimeout", "5000000", 0);   // 5 秒 socket 超时
    av_dict_set(&options, "timeout", "5000000", 0);    // 5 秒 IO 超时

    // ---------- 3. 构建滤镜图 ----------
    // 使用原始字符串字面量避免繁琐转义
    // 注意：%{localtime} 不带格式参数，避免 FFmpeg 4.x drawtext 解析失败
    std::string scale_str = "scale=" + std::to_string(width) + ":" + std::to_string(height);
    std::string filter_desc = scale_str + ",format=yuv420p,drawtext=fontsize=40:fontcolor=white:box=1:boxcolor=black@0.5:boxborderw=5:x=w-tw-10:y=10:text='%{localtime}'";

    AVFilterGraph* filter_graph = avfilter_graph_alloc();
    AVFilterContext* buffersrc_ctx = nullptr;
    AVFilterContext* buffersink_ctx = nullptr;

    const AVFilter* buffersrc = avfilter_get_by_name("buffer");
    // 确保 time_base 和 SAR 有效
    AVRational tb = dec_ctx->time_base;
    if (tb.num <= 0 || tb.den <= 0) {
        tb = {1, 25};  // 默认时间基
    }
    AVRational sar = dec_ctx->sample_aspect_ratio;
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

    const AVFilter* buffersink = avfilter_get_by_name("buffersink");
    CHECK_ERR(avfilter_graph_create_filter(&buffersink_ctx, buffersink, "out",
                                           nullptr, nullptr, filter_graph),
              "创建 buffersink 滤镜失败");

    AVFilterInOut* outputs = avfilter_inout_alloc();
    outputs->name = av_strdup("in");
    outputs->filter_ctx = buffersrc_ctx;
    outputs->pad_idx = 0;
    outputs->next = nullptr;

    AVFilterInOut* inputs = avfilter_inout_alloc();
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
    av_dict_free(&options);
    std::cout << "RTSP 连接成功，开始推流（Ctrl+C 退出）..." << std::endl;

    // ---------- 5. 主循环 ----------
    AVPacket* pkt = av_packet_alloc();
    AVPacket* enc_pkt = av_packet_alloc();
    AVFrame* frame = av_frame_alloc();
    AVFrame* filtered_frame = av_frame_alloc();

    int64_t start_time = av_gettime();  // 用于实时同步

    // 推流实时统计
    int64_t streamed_frames = 0;       // 已成功推流帧数
    int64_t decoded_frames = 0;        // 已解码帧数
    int64_t out_frame_count = 0;       // 已输出（编码）帧数，用于生成 PTS
    int64_t loop_count = 1;            // 播放轮次
    int64_t last_log_time = av_gettime();  // 上次刷新日志的时间
    int64_t last_log_frames = 0;           // 上次刷新日志时的帧数

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
            printf("\r[推流中] 已解码: %lld 帧 | 已推流: %lld 帧 | 第 %lld 轮 | 实时帧率: %.2f fps | 运行时长: %.1f s   ",
                   (long long)decoded_frames, (long long)streamed_frames,
                   (long long)loop_count, fps, elapsed);
            fflush(stdout);
            last_log_time = now;
            last_log_frames = streamed_frames;
        }

        av_packet_unref(pkt);
    }

    // 退出时打印最终统计
    {
        double elapsed = (av_gettime() - start_time) / 1e6;
        printf("\n[结束] 共推流 %lld 帧 | 播放 %lld 轮 | 运行时长 %.1f s\n",
               (long long)streamed_frames, (long long)loop_count, elapsed);
    }

    // ---------- 6. 清理 ----------
    // 写尾（对 RTSP 可能无实际作用，但安全）
    av_write_trailer(ofmt_ctx);

    av_packet_free(&pkt);
    av_packet_free(&enc_pkt);
    av_frame_free(&frame);
    av_frame_free(&filtered_frame);
    avfilter_graph_free(&filter_graph);
    avcodec_free_context(&dec_ctx);
    avcodec_free_context(&enc_ctx);
    avformat_close_input(&ifmt_ctx);
    if (ofmt_ctx) {
        avio_close(ofmt_ctx->pb);
        avformat_free_context(ofmt_ctx);
    }
    avformat_network_deinit();

    std::cout << "推流结束。" << std::endl;
    return 0;
}