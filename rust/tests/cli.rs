use assert_cmd::Command;
use predicates::prelude::*;

fn command() -> Command {
    Command::cargo_bin("rtsp_pusher").expect("测试二进制应存在")
}

#[test]
fn help_exits_zero_and_lists_options() {
    command()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("--input"))
        .stdout(predicate::str::contains("--codec"))
        .stdout(predicate::str::contains("--no-watermark"));
}

#[test]
fn unknown_codec_exits_one() {
    command()
        .args(["-c", "h266"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("不支持的编码协议"))
        .stdout(predicate::str::is_empty());
}

#[test]
fn missing_input_exits_one() {
    command()
        .args(["-i", "/nonexistent/rtsp-pusher-input.mp4"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("无法打开输入文件"))
        .stdout(predicate::str::contains("推流失败。"));
}

#[test]
fn unknown_option_exits_one() {
    command()
        .arg("--definitely-not-an-option")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("参数解析错误"));
}

#[test]
fn non_numeric_width_exits_one() {
    command()
        .args(["-w", "not-a-number"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("参数解析错误"));
}
