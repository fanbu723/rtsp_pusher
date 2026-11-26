/// 按输入帧率计算单帧间隔（微秒），保留整数除法语义。
pub fn frame_interval_us(fps_num: i32, fps_den: i32) -> i64 {
    if fps_num <= 0 || fps_den <= 0 {
        return 40_000; // 原实现的时间基兜底 25/1
    }
    (1_000_000_i64 * fps_den as i64) / fps_num as i64
}

/// 第 `decoded_frames` 帧（从 1 开始）应到达的绝对时间（微秒）。
pub fn target_time_us(start_time_us: i64, decoded_frames: i64, interval_us: i64) -> i64 {
    start_time_us + (decoded_frames - 1) * interval_us
}

/// 需要休眠的微秒数；`--no-realtime` 时永不休眠。
pub fn sleep_us(target_us: i64, now_us: i64, no_realtime: bool) -> i64 {
    if no_realtime {
        return 0;
    }
    (target_us - now_us).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_match_common_rates() {
        assert_eq!(frame_interval_us(25, 1), 40_000);
        assert_eq!(frame_interval_us(30, 1), 33_333);
        assert_eq!(frame_interval_us(30000, 1001), 33_366);
        assert_eq!(frame_interval_us(0, 1), 40_000);
        assert_eq!(frame_interval_us(25, 0), 40_000);
    }

    #[test]
    fn first_frame_targets_start_time() {
        assert_eq!(target_time_us(1_000, 1, 40_000), 1_000);
        assert_eq!(target_time_us(1_000, 2, 40_000), 41_000);
    }

    #[test]
    fn sleep_is_never_negative() {
        assert_eq!(sleep_us(1_000, 5_000, false), 0);
        assert_eq!(sleep_us(5_000, 1_000, false), 4_000);
        assert_eq!(sleep_us(5_000, 1_000, true), 0);
    }
}
