mod cli;
mod error;
mod ffmpeg_ext;
mod pacing;
mod pipeline;
mod settings;
mod watermark;

use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = match cli::parse_args() {
        Ok(cli) => cli,
        Err(code) => return ExitCode::from(code),
    };

    let output_codec = match settings::parse_output_codec(&cli.codec) {
        Some(codec) => codec,
        None => {
            eprintln!("不支持的编码协议: {}（可选值: h264 / h265）", cli.codec);
            return ExitCode::FAILURE;
        }
    };

    match pipeline::run(cli, output_codec) {
        Ok(()) => {
            println!("推流结束。");
            ExitCode::SUCCESS
        }
        Err(error) => {
            error.report();
            println!("推流失败。");
            ExitCode::FAILURE
        }
    }
}
