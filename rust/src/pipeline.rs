use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::flag::Flags;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::media::Type;
use ffmpeg_next::Rational;

use crate::cli::Cli;
use crate::error::AppError;
use crate::ffmpeg_ext as fx;
use crate::pacing;
use crate::settings;
use crate::watermark;

struct NetworkGuard;

impl Drop for NetworkGuard {
    fn drop(&mut self) {
        ffmpeg::format::network::deinit();
    }
}

fn is_again_or_eof(error: &ffmpeg::Error) -> bool {
    matches!(*error, ffmpeg::Error::Eof)
        || matches!(*error, ffmpeg::Error::Other { errno } if errno == libc::EAGAIN)
}

struct OpenedEncoder {
    encoder: ffmpeg::codec::encoder::Video,
    name: String,
    is_hardware: bool,
    pixel_format: Pixel,
    settings: settings::EncoderSettings,
}

fn ffmpeg_error_message(error: &ffmpeg::Error) -> String {
    let message = error.to_string();
    if message.is_empty() {
        format!("FFmpeg 错误码 {}", i32::from(*error))
    } else {
        message
    }
}

fn try_open_encoder(
    encoder_codec: ffmpeg::codec::codec::Codec,
    cli: &Cli,
    source_fps: Rational,
) -> Result<OpenedEncoder, AppError> {
    let encoder_name = encoder_codec.name().to_string();
    let is_hardware = !settings::is_software_encoder(&encoder_name);
    let pixel_format = settings::choose_pixel_format(&fx::encoder_pixel_formats(&encoder_codec));
    let plan = settings::build_encoder_settings(settings::EncoderRequest {
        width: cli.width,
        height: cli.height,
        fps_num: source_fps.0,
        fps_den: source_fps.1,
        bitrate_kbps: cli.bitrate,
        low_latency: cli.low_latency,
        is_hardware,
        pix_fmt: pixel_format,
    });

    let mut encoder_context = fx::new_encoder_context(&encoder_codec)
        .map_err(|error| AppError::ctx("打开编码器失败", error))?
        .encoder()
        .video()
        .map_err(|error| AppError::ctx("打开编码器失败", error))?;
    encoder_context.set_width(cli.width as u32);
    encoder_context.set_height(cli.height as u32);
    encoder_context.set_format(pixel_format);
    encoder_context.set_time_base(source_fps);
    encoder_context.set_frame_rate(Some(source_fps));
    encoder_context.set_bit_rate(plan.bit_rate as usize);
    encoder_context.set_gop(plan.gop as u32);
    encoder_context.set_max_b_frames(plan.max_b_frames);
    let mut flags = Flags::GLOBAL_HEADER;
    if plan.low_latency {
        flags |= Flags::LOW_DELAY;
    }
    encoder_context.set_flags(flags);
    fx::set_keyint_min(&mut encoder_context, plan.keyint_min);
    if plan.rc_max_rate > 0 {
        fx::set_rate_control(&mut encoder_context, plan.rc_max_rate, plan.rc_buffer_size);
    }

    // 编码器私有 AVOption 必须随 avcodec_open2 传入：FFmpeg 6.1 的
    // AVCodecContext.priv_data 在 open 前不可用，直接 av_opt_set 会静默失败。
    let mut encoder_options = ffmpeg::Dictionary::new();
    if is_hardware {
        // gop/rc_mode/max_bit_rate/frame_rate 是 Ascend 插件的私有选项；
        // NVENC/QSV 使用 AVCodecContext 的通用字段即可。
        if encoder_name.ends_with("_ascend") {
            encoder_options.set("gop", &plan.hw_gop.to_string());
            encoder_options.set("rc_mode", &plan.hw_rc_mode.to_string());
            encoder_options.set("max_bit_rate", &plan.hw_max_bit_rate.to_string());
            encoder_options.set("frame_rate", &plan.hw_frame_rate.to_string());
            if cli.verbose {
                println!(
                    "{}: gop={} rc_mode=CBR bitrate={}kbps fps={}",
                    encoder_name, plan.hw_gop, plan.hw_max_bit_rate, plan.hw_frame_rate
                );
            }
        }
    } else {
        encoder_options.set("preset", "ultrafast");
        encoder_options.set("tune", "zerolatency");
        if !cli.verbose && encoder_name == "libx265" {
            encoder_options.set("x265-params", "log-level=error");
        }
    }

    let encoder = encoder_context
        .open_as_with(encoder_codec, encoder_options)
        .map_err(|error| {
            AppError::msg(format!(
                "打开编码器 {encoder_name} 失败: {}",
                ffmpeg_error_message(&error)
            ))
        })?;

    Ok(OpenedEncoder {
        encoder,
        name: encoder_name,
        is_hardware,
        pixel_format,
        settings: plan,
    })
}

fn open_encoder_with_fallback(
    output_codec: settings::OutputCodec,
    cli: &Cli,
    source_fps: Rational,
) -> Result<OpenedEncoder, AppError> {
    let mut last_error = None;

    for name in settings::encoder_candidates(output_codec) {
        let Some(encoder_codec) = ffmpeg::codec::encoder::find_by_name(name) else {
            continue;
        };
        match try_open_encoder(encoder_codec, cli, source_fps) {
            Ok(opened) => {
                if !opened.is_hardware {
                    eprintln!("未找到可用的硬件编码器，回退到软件编码器: {}", opened.name);
                }
                return Ok(opened);
            }
            Err(error) => {
                if settings::is_software_encoder(name) {
                    return Err(error);
                }
                eprintln!("硬件编码器 {name} 不可用: {error}");
                last_error = Some(error.to_string());
            }
        }
    }

    let detail = last_error
        .map(|error| format!("；最后一次错误: {error}"))
        .unwrap_or_default();
    Err(AppError::msg(format!(
        "找不到可用的 {} 编码器{detail}（请确认 FFmpeg 已编译 NVENC/Ascend/QSV 或 libx264/libx265）",
        output_codec.display()
    )))
}

pub fn run(cli: Cli, output_codec: settings::OutputCodec) -> Result<(), AppError> {
    if !cli.verbose {
        fx::set_log_level_error();
    }
    let watermark_enabled = !cli.no_watermark;

    println!("输入文件: {}", cli.input);
    println!("输出地址: {}", cli.url);
    println!("输出尺寸: {}x{}", cli.width, cli.height);
    println!("编码协议: {}", output_codec.display());
    println!(
        "时间戳水印: {}",
        if watermark_enabled {
            "已启用"
        } else {
            "已关闭"
        }
    );

    let stop_requested = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop_requested.clone())
        .map_err(|error| AppError::msg(format!("注册 SIGINT 处理失败: {error}")))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop_requested.clone())
        .map_err(|error| AppError::msg(format!("注册 SIGTERM 处理失败: {error}")))?;

    ffmpeg::format::network::init();
    let _network_guard = NetworkGuard;

    let mut input =
        fx::open_input(&cli.input).map_err(|error| AppError::ctx("无法打开输入文件", error))?;

    let video_stream_index = input
        .streams()
        .enumerate()
        .find(|(_, stream)| stream.parameters().medium() == Type::Video)
        .map(|(index, _)| index)
        .ok_or_else(|| AppError::msg("未找到视频流"))?;

    let mut source_fps =
        fx::guess_frame_rate(&mut input, video_stream_index).unwrap_or(Rational(25, 1));
    if source_fps.0 <= 0 || source_fps.1 <= 0 {
        source_fps = Rational(25, 1);
    }
    if cli.fps > 0 {
        source_fps = Rational(cli.fps, 1);
    }
    println!("输入视频帧率: {}/{}", source_fps.0, source_fps.1);

    let mut decoder = {
        let stream = input
            .stream(video_stream_index)
            .ok_or_else(|| AppError::msg("未找到视频流"))?;
        let parameters = stream.parameters();
        let codec = ffmpeg::codec::decoder::find(parameters.id())
            .ok_or_else(|| AppError::msg("找不到解码器"))?;
        let context = ffmpeg::codec::context::Context::from_parameters(parameters)
            .map_err(|error| AppError::ctx("打开解码器失败", error))?;

        context
            .decoder()
            .open_as(codec)
            .map_err(|error| AppError::ctx("打开解码器失败", error))?
            .video()
            .map_err(|error| AppError::ctx("打开解码器失败", error))?
    };

    let decoder_pixel_format = match decoder.format() {
        Pixel::None => Pixel::YUV420P,
        format => format,
    };
    let decoder_width = decoder.width();
    let decoder_height = decoder.height();
    let mut decoder_time_base = decoder.time_base();
    if decoder_time_base.0 <= 0 || decoder_time_base.1 <= 0 {
        decoder_time_base = Rational(1, 25);
    }
    let mut sample_aspect_ratio = decoder.aspect_ratio();
    if sample_aspect_ratio.0 <= 0 || sample_aspect_ratio.1 <= 0 {
        sample_aspect_ratio = Rational(1, 1);
    }

    let mut output = fx::alloc_rtsp_output(&cli.url)
        .map_err(|error| AppError::ctx("创建输出上下文失败", error))?;
    let OpenedEncoder {
        mut encoder,
        name: encoder_name,
        is_hardware,
        pixel_format: encoder_pixel_format,
        settings: plan,
    } = open_encoder_with_fallback(output_codec, &cli, source_fps)?;

    println!(
        "使用编码器: {}{}，像素格式: {}",
        encoder_name,
        if is_hardware {
            "（硬件）"
        } else {
            "（软件）"
        },
        fx::pixel_name(encoder_pixel_format)
    );

    if cli.bitrate <= 0 && is_hardware {
        println!(
            "硬件编码器未指定码率，使用默认码率: {} kbps（可用 -b 调整）",
            plan.bit_rate / 1000
        );
    }
    if cli.low_latency {
        println!("低延迟模式已启用（GOP=10，缓冲≈0.25s）");
    }

    let encoder_time_base = fx::encoder_time_base(&encoder);
    let (output_stream_index, output_time_base) = {
        let mut output_stream = output
            .add_stream(None::<ffmpeg::Codec>)
            .map_err(|_| AppError::msg("创建输出流失败"))?;
        fx::set_stream_id(&mut output_stream, 0);
        output_stream.set_parameters(&encoder);
        output_stream.set_time_base(encoder_time_base);
        (output_stream.index(), output_stream.time_base())
    };

    let filter_description = format!(
        "scale={}:{},format={}",
        cli.width,
        cli.height,
        fx::pixel_name(encoder_pixel_format)
    );
    println!("滤镜链: {filter_description}");

    let buffer_args = format!(
        "video_size={}x{}:pix_fmt={}:time_base={}/{}:pixel_aspect={}/{}",
        decoder_width,
        decoder_height,
        fx::pixel_to_int(decoder_pixel_format),
        decoder_time_base.0,
        decoder_time_base.1,
        sample_aspect_ratio.0,
        sample_aspect_ratio.1
    );

    let mut filter_graph = ffmpeg::filter::Graph::new();
    let buffer_filter =
        ffmpeg::filter::find("buffer").ok_or_else(|| AppError::msg("创建 buffer 滤镜失败"))?;
    filter_graph
        .add(&buffer_filter, "in", &buffer_args)
        .map_err(|error| AppError::ctx("创建 buffer 滤镜失败", error))?;
    let buffersink_filter = ffmpeg::filter::find("buffersink")
        .ok_or_else(|| AppError::msg("创建 buffersink 滤镜失败"))?;
    filter_graph
        .add(&buffersink_filter, "out", "")
        .map_err(|error| AppError::ctx("创建 buffersink 滤镜失败", error))?;
    filter_graph
        .output("in", 0)
        .map_err(|error| AppError::ctx("解析滤镜描述失败", error))?
        .input("out", 0)
        .map_err(|error| AppError::ctx("解析滤镜描述失败", error))?
        .parse(&filter_description)
        .map_err(|error| AppError::ctx("解析滤镜描述失败", error))?;
    filter_graph
        .validate()
        .map_err(|error| AppError::ctx("配置滤镜图失败", error))?;

    println!("正在连接 RTSP 服务器: {} ...", cli.url);

    let mut options = ffmpeg::Dictionary::new();
    options.set("rtsp_transport", &cli.transport);
    // FFmpeg 6.1 的 RTSP 使用 timeout（微秒）；4.4 的 stimeout 已移除。
    options.set("timeout", "5000000");
    output
        .write_header_with(options)
        .map_err(|error| AppError::ctx("写头失败", error))?;

    println!("RTSP 连接成功，开始推流（Ctrl+C 退出）...");

    let mut packet = ffmpeg::Packet::empty();
    let mut encoded_packet = ffmpeg::Packet::empty();
    let mut frame = ffmpeg::frame::Video::empty();
    let mut filtered_frame = ffmpeg::frame::Video::empty();

    let start_time = ffmpeg::time::current();
    let mut last_log_time = start_time;
    let mut last_log_frames: i64 = 0;

    let mut decoded_frames: i64 = 0;
    let mut streamed_frames: i64 = 0;
    let mut output_frame_count: i64 = 0;
    let mut loop_count: i64 = 1;
    let mut seek_failed = false;

    while !stop_requested.load(Ordering::Relaxed) {
        if fx::read_frame(&mut input, &mut packet) < 0 {
            if fx::seek_start(&mut input) < 0 {
                eprintln!("循环播放 seek 失败");
                seek_failed = true;
                break;
            }
            loop_count += 1;
            continue;
        }

        if packet.stream() != video_stream_index {
            fx::packet_unref(&mut packet);
            continue;
        }

        if decoder.send_packet(&packet).is_err() {
            fx::packet_unref(&mut packet);
            continue;
        }

        loop {
            match decoder.receive_frame(&mut frame) {
                Ok(()) => {}
                Err(error) if is_again_or_eof(&error) => break,
                Err(_) => break,
            }

            decoded_frames += 1;

            let now = ffmpeg::time::current();
            let interval = pacing::frame_interval_us(source_fps.0, source_fps.1);
            let target = pacing::target_time_us(start_time, decoded_frames, interval);
            let delay = pacing::sleep_us(target, now, cli.no_realtime);
            if delay > 0 {
                let _ = ffmpeg::time::sleep(delay as u32);
            }

            let Some(mut source) = filter_graph.get("in") else {
                return Err(AppError::msg("buffer 滤镜丢失"));
            };
            if source.source().add(&frame).is_err() {
                break;
            }

            loop {
                let Some(mut sink) = filter_graph.get("out") else {
                    return Err(AppError::msg("buffersink 滤镜丢失"));
                };
                match sink.sink().frame(&mut filtered_frame) {
                    Ok(()) => {}
                    Err(error) if is_again_or_eof(&error) => break,
                    Err(_) => break,
                }

                if watermark_enabled {
                    let stride = filtered_frame.stride(0);
                    let width = filtered_frame.width() as i32;
                    let height = filtered_frame.height() as i32;
                    let text = watermark::now_string();
                    watermark::draw_timestamp(
                        filtered_frame.data_mut(0),
                        stride,
                        width,
                        height,
                        &text,
                        2,
                    );
                }

                filtered_frame.set_pts(Some(output_frame_count));
                output_frame_count += 1;

                if encoder.send_frame(&filtered_frame).is_err() {
                    break;
                }

                loop {
                    match encoder.receive_packet(&mut encoded_packet) {
                        Ok(()) => {}
                        Err(error) if is_again_or_eof(&error) => break,
                        Err(_) => break,
                    }

                    encoded_packet.rescale_ts(encoder_time_base, output_time_base);
                    encoded_packet.set_stream(output_stream_index);

                    match encoded_packet.write_interleaved(&mut output) {
                        Ok(()) => streamed_frames += 1,
                        Err(error) => {
                            eprintln!("\n[错误] 写入 RTSP 失败: {error}");
                        }
                    }
                    fx::packet_unref(&mut encoded_packet);
                }

                fx::frame_unref(&mut filtered_frame);
            }

            fx::frame_unref(&mut frame);
        }

        fx::packet_unref(&mut packet);

        let now = ffmpeg::time::current();
        if now - last_log_time >= 1_000_000 {
            let elapsed_seconds = (now - start_time) as f64 / 1_000_000.0;
            let interval_seconds = (now - last_log_time) as f64 / 1_000_000.0;
            let fps = if interval_seconds > 0.0 {
                (streamed_frames - last_log_frames) as f64 / interval_seconds
            } else {
                0.0
            };
            print!(
                "\r{} [推流中] 已解码: {} 帧 | 已推流: {} 帧 | 第 {} 轮 | 实时帧率: {:.2} fps | 运行时长: {:.1} s   ",
                watermark::now_string(),
                decoded_frames,
                streamed_frames,
                loop_count,
                fps,
                elapsed_seconds
            );
            let _ = std::io::stdout().flush();
            last_log_time = now;
            last_log_frames = streamed_frames;
        }
    }

    let elapsed_seconds = (ffmpeg::time::current() - start_time) as f64 / 1_000_000.0;
    println!(
        "\n{} [结束] 共推流 {} 帧 | 播放 {} 轮 | 运行时长 {:.1} s",
        watermark::now_string(),
        streamed_frames,
        loop_count,
        elapsed_seconds
    );

    let _ = output.write_trailer();

    if seek_failed {
        return Err(AppError::Reported);
    }
    Ok(())
}
