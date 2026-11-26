use clap::{error::ErrorKind, CommandFactory, Parser};

/// FFmpeg RTSP 推流工具（H.264/H.265，支持时间戳水印）
#[derive(Parser, Debug, Clone)]
#[command(
    name = "rtsp_pusher",
    bin_name = "rtsp_pusher",
    about = "FFmpeg RTSP 推流工具（H.264/H.265，支持时间戳水印）",
    disable_version_flag = true,
    disable_help_flag = true
)]
pub struct Cli {
    #[arg(
        short = 'i',
        long = "input",
        default_value = "o2.mp4",
        help = "输入视频文件"
    )]
    pub input: String,

    #[arg(
        short = 'u',
        long = "url",
        default_value = "rtsp://127.0.0.1:8554/live3",
        help = "RTSP 输出地址"
    )]
    pub url: String,

    #[arg(short = 'w', long = "width", default_value_t = 2560, help = "输出宽度")]
    pub width: i32,

    #[arg(long = "height", default_value_t = 1440, help = "输出高度")]
    pub height: i32,

    #[arg(
        short = 'f',
        long = "fps",
        default_value_t = 0,
        help = "目标帧率（0 表示自动使用输入帧率）"
    )]
    pub fps: i32,

    #[arg(
        short = 't',
        long = "transport",
        default_value = "tcp",
        help = "RTSP 传输协议（tcp/udp）"
    )]
    pub transport: String,

    #[arg(
        short = 'b',
        long = "bitrate",
        default_value_t = 0,
        help = "编码码率 kbps（0 表示 CRF 自动）"
    )]
    pub bitrate: i32,

    #[arg(
        short = 'c',
        long = "codec",
        default_value = "h264",
        help = "输出视频编码协议（h264/h265，默认 h264）"
    )]
    pub codec: String,

    #[arg(long = "no-realtime", help = "关闭实时速度控制（默认按实时速度推流）")]
    pub no_realtime: bool,

    #[arg(long = "no-watermark", help = "关闭实时时间戳水印（默认开启）")]
    pub no_watermark: bool,

    #[arg(
        long = "low-latency",
        help = "低延迟模式：减小 GOP 与编码缓冲，降低端到端延迟"
    )]
    pub low_latency: bool,

    #[arg(short = 'v', long = "verbose", help = "显示详细日志（默认只显示错误）")]
    pub verbose: bool,

    #[arg(short = 'h', long = "help", help = "显示帮助")]
    pub help: bool,
}

/// 解析命令行；`Err(0)` 表示帮助已打印，`Err(1)` 表示参数错误。
pub fn parse_args() -> Result<Cli, u8> {
    match Cli::try_parse() {
        Ok(cli) if cli.help => {
            let mut command = Cli::command();
            print!("{}", command.render_help());
            Err(0)
        }
        Ok(cli) => Ok(cli),
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                print!("{error}");
                Err(0)
            }
            _ => {
                eprintln!("参数解析错误: {error}");
                let mut command = Cli::command();
                print!("{}", command.render_help());
                Err(1)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("参数应能解析")
    }

    #[test]
    fn defaults_match_cpp_tool() {
        let cli = parse(&["rtsp_pusher"]);
        assert_eq!(cli.input, "o2.mp4");
        assert_eq!(cli.url, "rtsp://127.0.0.1:8554/live3");
        assert_eq!(cli.width, 2560);
        assert_eq!(cli.height, 1440);
        assert_eq!(cli.fps, 0);
        assert_eq!(cli.transport, "tcp");
        assert_eq!(cli.bitrate, 0);
        assert_eq!(cli.codec, "h264");
        assert!(!cli.no_realtime);
        assert!(!cli.no_watermark);
        assert!(!cli.low_latency);
        assert!(!cli.verbose);
        assert!(!cli.help);
    }

    #[test]
    fn help_flag_is_parsed() {
        let cli = parse(&["rtsp_pusher", "-h"]);
        assert!(cli.help);
    }

    #[test]
    fn short_and_long_options_work() {
        let cli = parse(&[
            "rtsp_pusher",
            "-i",
            "in.mp4",
            "-u",
            "rtsp://x/y",
            "-w",
            "640",
            "--height",
            "360",
            "-f",
            "30",
            "-t",
            "udp",
            "-b",
            "4000",
            "-c",
            "h265",
            "--no-realtime",
            "--no-watermark",
            "--low-latency",
            "-v",
        ]);
        assert_eq!(cli.input, "in.mp4");
        assert_eq!(cli.url, "rtsp://x/y");
        assert_eq!(cli.width, 640);
        assert_eq!(cli.height, 360);
        assert_eq!(cli.fps, 30);
        assert_eq!(cli.transport, "udp");
        assert_eq!(cli.bitrate, 4000);
        assert_eq!(cli.codec, "h265");
        assert!(cli.no_realtime);
        assert!(cli.no_watermark);
        assert!(cli.low_latency);
        assert!(cli.verbose);
    }

    #[test]
    fn unknown_option_is_rejected() {
        assert!(Cli::try_parse_from(["rtsp_pusher", "--does-not-exist"]).is_err());
    }

    #[test]
    fn non_numeric_width_is_rejected() {
        assert!(Cli::try_parse_from(["rtsp_pusher", "-w", "abc"]).is_err());
    }
}
