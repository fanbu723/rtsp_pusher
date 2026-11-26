use ffmpeg_next::codec::Id;
use ffmpeg_next::format::Pixel;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OutputCodec {
    H264,
    H265,
}

impl OutputCodec {
    /// 大小写不敏感；只接受 h264 / h265（与 C++ 一致）。
    pub fn parse(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("h264") {
            Some(OutputCodec::H264)
        } else if name.eq_ignore_ascii_case("h265") {
            Some(OutputCodec::H265)
        } else {
            None
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            OutputCodec::H264 => "H.264 (AVC)",
            OutputCodec::H265 => "H.265 (HEVC)",
        }
    }

    pub fn ffmpeg_id(self) -> Id {
        match self {
            OutputCodec::H264 => Id::H264,
            OutputCodec::H265 => Id::HEVC,
        }
    }
}

pub fn parse_output_codec(name: &str) -> Option<OutputCodec> {
    OutputCodec::parse(name)
}

pub fn is_software_encoder(name: &str) -> bool {
    name == "libx264" || name == "libx265"
}

/// 选择编码器支持的像素格式：优先 yuv420p，否则第一个支持的格式。
pub fn choose_pixel_format(supported: &[Pixel]) -> Pixel {
    supported
        .iter()
        .copied()
        .find(|pixel| *pixel == Pixel::YUV420P)
        .or_else(|| supported.first().copied())
        .unwrap_or(Pixel::YUV420P)
}

/// 硬件编码器未指定码率时的默认值（2–20 Mbps）。
pub fn default_hw_bitrate(width: i32, height: i32, fps_num: i32, fps_den: i32) -> i64 {
    let fps = if fps_den > 0 {
        fps_num as f64 / fps_den as f64
    } else {
        25.0
    };
    let bitrate = (width as f64 * height as f64 * fps * 0.15) as i64;
    bitrate.clamp(2_000_000, 20_000_000)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderSettings {
    pub pix_fmt: Pixel,
    pub bit_rate: i64,
    pub gop: i32,
    pub keyint_min: i32,
    pub max_b_frames: usize,
    pub low_latency: bool,
    pub rc_max_rate: i64,
    pub rc_buffer_size: i64,
    pub hw_gop: i64,
    pub hw_rc_mode: i64,
    pub hw_max_bit_rate: i64,
    pub hw_frame_rate: i64,
}

pub struct EncoderRequest {
    pub width: i32,
    pub height: i32,
    pub fps_num: i32,
    pub fps_den: i32,
    pub bitrate_kbps: i32,
    pub low_latency: bool,
    pub is_hardware: bool,
    pub pix_fmt: Pixel,
}

pub fn build_encoder_settings(request: EncoderRequest) -> EncoderSettings {
    let EncoderRequest {
        width,
        height,
        fps_num,
        fps_den,
        bitrate_kbps,
        low_latency,
        is_hardware,
        pix_fmt,
    } = request;
    let bit_rate = if bitrate_kbps > 0 {
        bitrate_kbps as i64 * 1000
    } else if is_hardware {
        default_hw_bitrate(width, height, fps_num, fps_den)
    } else {
        0
    };

    let gop = if low_latency { 10 } else { 25 };
    let (rc_max_rate, rc_buffer_size) = if is_hardware && bit_rate > 0 {
        if low_latency {
            (bit_rate * 2, bit_rate / 4)
        } else {
            (bit_rate, bit_rate)
        }
    } else {
        (0, 0)
    };

    let fps_int = if fps_den > 0 { fps_num / fps_den } else { 25 };
    let hw_frame_rate = fps_int.clamp(1, 240) as i64;
    let hw_max_bit_rate = (bit_rate / 1000).clamp(2, 614_400);

    EncoderSettings {
        pix_fmt,
        bit_rate,
        gop,
        keyint_min: gop,
        max_b_frames: 0,
        low_latency,
        rc_max_rate,
        rc_buffer_size,
        hw_gop: if low_latency { 10 } else { 30 },
        hw_rc_mode: 0,
        hw_max_bit_rate,
        hw_frame_rate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codec_names_case_insensitively() {
        assert_eq!(OutputCodec::parse("h264"), Some(OutputCodec::H264));
        assert_eq!(OutputCodec::parse("H264"), Some(OutputCodec::H264));
        assert_eq!(OutputCodec::parse("h265"), Some(OutputCodec::H265));
        assert_eq!(OutputCodec::parse("HeVc"), None);
        assert_eq!(OutputCodec::parse("h266"), None);
        assert_eq!(OutputCodec::H265.display(), "H.265 (HEVC)");
    }

    #[test]
    fn chooses_yuv420p_first_when_available() {
        assert_eq!(
            choose_pixel_format(&[Pixel::NV12, Pixel::YUV420P]),
            Pixel::YUV420P
        );
        assert_eq!(choose_pixel_format(&[Pixel::NV12]), Pixel::NV12);
        assert_eq!(choose_pixel_format(&[]), Pixel::YUV420P);
    }

    #[test]
    fn software_encoder_detection_matches_cpp() {
        assert!(is_software_encoder("libx264"));
        assert!(is_software_encoder("libx265"));
        assert!(!is_software_encoder("h264_ascend"));
        assert!(!is_software_encoder(""));
    }

    #[test]
    fn hardware_default_bitrate_is_clamped() {
        assert_eq!(default_hw_bitrate(1920, 1080, 25, 1), 7_776_000);
        assert_eq!(default_hw_bitrate(640, 360, 25, 1), 2_000_000);
        assert_eq!(default_hw_bitrate(3840, 2160, 60, 1), 20_000_000);
        assert_eq!(default_hw_bitrate(1920, 1080, 25, 0), 7_776_000);
    }

    #[test]
    fn software_default_uses_crf_and_no_rc() {
        let settings = build_encoder_settings(EncoderRequest {
            width: 640,
            height: 360,
            fps_num: 25,
            fps_den: 1,
            bitrate_kbps: 0,
            low_latency: false,
            is_hardware: false,
            pix_fmt: Pixel::YUV420P,
        });
        assert_eq!(settings.bit_rate, 0);
        assert_eq!(settings.gop, 25);
        assert_eq!(settings.keyint_min, 25);
        assert_eq!(settings.max_b_frames, 0);
        assert_eq!(settings.rc_max_rate, 0);
        assert_eq!(settings.rc_buffer_size, 0);
    }

    #[test]
    fn explicit_bitrate_wins() {
        let settings = build_encoder_settings(EncoderRequest {
            width: 640,
            height: 360,
            fps_num: 25,
            fps_den: 1,
            bitrate_kbps: 4000,
            low_latency: false,
            is_hardware: false,
            pix_fmt: Pixel::YUV420P,
        });
        assert_eq!(settings.bit_rate, 4_000_000);
        assert_eq!(settings.rc_max_rate, 0);
    }

    #[test]
    fn hardware_rc_and_low_latency_follow_cpp_formula() {
        let normal = build_encoder_settings(EncoderRequest {
            width: 1920,
            height: 1080,
            fps_num: 25,
            fps_den: 1,
            bitrate_kbps: 0,
            low_latency: false,
            is_hardware: true,
            pix_fmt: Pixel::YUV420P,
        });
        assert_eq!(normal.bit_rate, 7_776_000);
        assert_eq!(normal.rc_max_rate, 7_776_000);
        assert_eq!(normal.rc_buffer_size, 7_776_000);
        assert_eq!(normal.hw_gop, 30);
        assert_eq!(normal.hw_max_bit_rate, 7776);
        assert_eq!(normal.hw_frame_rate, 25);

        let low = build_encoder_settings(EncoderRequest {
            width: 1920,
            height: 1080,
            fps_num: 25,
            fps_den: 1,
            bitrate_kbps: 0,
            low_latency: true,
            is_hardware: true,
            pix_fmt: Pixel::YUV420P,
        });
        assert_eq!(low.gop, 10);
        assert_eq!(low.keyint_min, 10);
        assert_eq!(low.rc_max_rate, 15_552_000);
        assert_eq!(low.rc_buffer_size, 1_944_000);
        assert_eq!(low.hw_gop, 10);
    }

    #[test]
    fn hardware_frame_rate_is_integer_and_clamped() {
        let jitter = build_encoder_settings(EncoderRequest {
            width: 320,
            height: 180,
            fps_num: 30000,
            fps_den: 1001,
            bitrate_kbps: 0,
            low_latency: false,
            is_hardware: true,
            pix_fmt: Pixel::YUV420P,
        });
        assert_eq!(jitter.hw_frame_rate, 29);
        let slow = build_encoder_settings(EncoderRequest {
            width: 320,
            height: 180,
            fps_num: 0,
            fps_den: 1,
            bitrate_kbps: 0,
            low_latency: false,
            is_hardware: true,
            pix_fmt: Pixel::YUV420P,
        });
        assert_eq!(slow.hw_frame_rate, 1);
    }
}
