//! ffmpeg-next 未暴露、但原 C++ 实现依赖的少量 FFmpeg API。
//!
//! 这里只放薄封装，所有 `unsafe` 都限定在这些函数内，并在调用点保证
//! 指针来自仍然存活的 ffmpeg-next 对象。

use std::ffi::CString;
use std::ptr;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec::codec::Codec;
use ffmpeg_next::codec::encoder::Encoder;
use ffmpeg_next::codec::packet::{Mut, Packet};
use ffmpeg_next::ffi;
use ffmpeg_next::format::context::{Input, Output};
use ffmpeg_next::format::stream::StreamMut;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::Rational;

/// 打开输入文件，并在 `avformat_find_stream_info` 前设置探测参数。
///
/// `format::input()` 会在返回前调用 find_stream_info，无法插入这两个参数，
/// 因此这里直接走 FFmpeg 原生调用。
pub fn open_input(path: &str) -> Result<Input, ffmpeg::Error> {
    let path = CString::new(path).map_err(|_| ffmpeg::Error::InvalidData)?;
    let mut context = ptr::null_mut();

    // SAFETY: context 由 FFmpeg 分配；path 在调用期间有效。
    let ret = unsafe {
        ffi::avformat_open_input(
            &mut context,
            path.as_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if ret < 0 {
        return Err(ffmpeg::Error::from(ret));
    }

    // SAFETY: avformat_open_input 成功后 context 非空。
    unsafe {
        (*context).max_analyze_duration = 5 * ffi::AV_TIME_BASE as i64;
        (*context).probesize = 20 * 1024 * 1024;
    }

    // SAFETY: context 仍是刚打开的有效上下文。
    let ret = unsafe { ffi::avformat_find_stream_info(context, ptr::null_mut()) };
    if ret < 0 {
        // SAFETY: 失败路径上由我们负责关闭。
        unsafe { ffi::avformat_close_input(&mut context) };
        return Err(ffmpeg::Error::from(ret));
    }

    // SAFETY: context 已完整初始化，所有权转交给 Input。
    Ok(unsafe { Input::wrap(context) })
}

/// 只分配 RTSP 输出上下文，不提前 `avio_open`；连接与选项在写头时处理。
pub fn alloc_rtsp_output(url: &str) -> Result<Output, ffmpeg::Error> {
    let url = CString::new(url).map_err(|_| ffmpeg::Error::InvalidData)?;
    // INFALLIBLE: 静态字面量不含 NUL。
    let format = CString::new("rtsp").expect("静态字符串不含 NUL");
    let mut context = ptr::null_mut();

    // SAFETY: 参数均为有效 C 字符串，context 由 FFmpeg 分配。
    let ret = unsafe {
        ffi::avformat_alloc_output_context2(
            &mut context,
            ptr::null_mut(),
            format.as_ptr(),
            url.as_ptr(),
        )
    };
    if ret < 0 {
        return Err(ffmpeg::Error::from(ret));
    }

    // SAFETY: 分配成功，所有权转交给 Output。
    Ok(unsafe { Output::wrap(context) })
}

/// 用指定编码器分配 AVCodecContext。
///
/// `avcodec_alloc_context3(NULL)` 再在 open 时传 codec 时，FFmpeg 6.1 不会
/// 正确初始化 `priv_data`，导致 preset/tune 等私有选项被忽略；必须在分配时
/// 就传入 codec。
pub fn new_encoder_context(codec: &Codec) -> Result<ffmpeg::codec::Context, ffmpeg::Error> {
    // SAFETY: codec 是 avcodec_find_encoder 返回的静态对象。
    let context = unsafe { ffi::avcodec_alloc_context3(codec.as_ptr()) };
    if context.is_null() {
        return Err(ffmpeg::Error::Unknown);
    }
    // SAFETY: context 由 FFmpeg 分配，所有权转交给 Context。
    Ok(unsafe { ffmpeg::codec::Context::wrap(context, None) })
}

/// `av_guess_frame_rate` 的封装；未找到流时返回 None。
pub fn guess_frame_rate(input: &mut Input, stream_index: usize) -> Option<Rational> {
    let stream_ptr = {
        let stream = input.stream(stream_index)?;
        // SAFETY: stream 借用自 input，转换出的指针只在本次调用期间使用。
        unsafe { stream.as_ptr() as *mut _ }
    };
    // SAFETY: stream_ptr 指向 input 内部仍存活的流，input 在调用期间未被释放。
    let rate = unsafe { ffi::av_guess_frame_rate(input.as_mut_ptr(), stream_ptr, ptr::null_mut()) };
    Some(Rational(rate.num, rate.den))
}

/// 编码器声明的像素格式列表（`AVCodec.pix_fmts`）。
pub fn encoder_pixel_formats(codec: &Codec) -> Vec<Pixel> {
    // SAFETY: codec 是 avcodec_find_encoder 返回的静态对象。
    let ptr = unsafe { codec.as_ptr() };
    if ptr.is_null() {
        return Vec::new();
    }

    // SAFETY: ptr 非空；pix_fmts 要么为 NULL，要么以 AV_PIX_FMT_NONE 结尾。
    let mut formats = unsafe { (*ptr).pix_fmts };
    if formats.is_null() {
        return Vec::new();
    }

    let mut result = Vec::new();
    for _ in 0..64 {
        // SAFETY: 依赖 FFmpeg 以 AV_PIX_FMT_NONE 终止数组。
        let format = unsafe { *formats };
        if format == ffi::AVPixelFormat::AV_PIX_FMT_NONE {
            break;
        }
        result.push(Pixel::from(format));
        // SAFETY: 数组下一个元素仍在 FFmpeg 管理的静态内存内。
        formats = unsafe { formats.add(1) };
    }
    result
}

pub fn pixel_to_int(pixel: Pixel) -> i32 {
    ffi::AVPixelFormat::from(pixel) as i32
}

pub fn pixel_name(pixel: Pixel) -> &'static str {
    pixel
        .descriptor()
        .map(|descriptor| descriptor.name())
        .unwrap_or("none")
}

/// `AVCodecContext.time_base`（ffmpeg-next 没有公开 getter）。
pub fn encoder_time_base(encoder: &Encoder) -> Rational {
    // SAFETY: encoder 持有有效的 AVCodecContext。
    let time_base = unsafe { (*encoder.as_ptr()).time_base };
    Rational(time_base.num, time_base.den)
}

pub fn set_keyint_min(encoder: &mut Encoder, value: i32) {
    // SAFETY: encoder 持有有效的 AVCodecContext。
    unsafe { (*encoder.as_mut_ptr()).keyint_min = value };
}

pub fn set_rate_control(encoder: &mut Encoder, max_rate: i64, buffer_size: i64) {
    // SAFETY: encoder 持有有效的 AVCodecContext。
    unsafe {
        (*encoder.as_mut_ptr()).rc_max_rate = max_rate;
        (*encoder.as_mut_ptr()).rc_buffer_size = buffer_size as i32;
    }
}

pub fn set_stream_id(stream: &mut StreamMut<'_>, id: i32) {
    // SAFETY: stream 借用自有效的输出上下文。
    unsafe { (*stream.as_mut_ptr()).id = id };
}

pub fn read_frame(input: &mut Input, packet: &mut Packet) -> i32 {
    // SAFETY: input 与 packet 均有效；packet 由调用方在使用后 unref。
    unsafe { ffi::av_read_frame(input.as_mut_ptr(), packet.as_mut_ptr()) }
}

pub fn seek_start(input: &mut Input) -> i32 {
    // SAFETY: input 有效，seek 参数与 C++ 实现一致。
    unsafe { ffi::av_seek_frame(input.as_mut_ptr(), -1, 0, ffi::AVSEEK_FLAG_BACKWARD) }
}

pub fn packet_unref(packet: &mut Packet) {
    // SAFETY: packet 是有效的 AVPacket 包装。
    unsafe { ffi::av_packet_unref(packet.as_mut_ptr()) };
}

pub fn frame_unref(frame: &mut ffmpeg::frame::Video) {
    // SAFETY: frame 是有效的 AVFrame 包装。
    unsafe { ffi::av_frame_unref(frame.as_mut_ptr()) };
}

pub fn set_log_level_error() {
    // SAFETY: 纯全局日志级别设置。
    unsafe { ffi::av_log_set_level(ffi::AV_LOG_ERROR) };
}
