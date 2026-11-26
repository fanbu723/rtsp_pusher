use std::time::{SystemTime, UNIX_EPOCH};

/// 5x7 点阵字体：'0'-'9'、':'、'.'，每字符 7 行 × 5 bit（bit4 为最左像素）。
pub const FONT_5X7: [[u8; 7]; 12] = [
    [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E], // 0
    [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E], // 1
    [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F], // 2
    [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E], // 3
    [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02], // 4
    [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E], // 5
    [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E], // 6
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08], // 7
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E], // 8
    [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C], // 9
    [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00], // :
    [0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x00], // .
];

/// 取当前墙钟时间字符串 `HH:MM:SS.mmm`（本地时区，与 C++ `localtime` 语义一致）。
pub fn format_time(now: SystemTime) -> String {
    let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds: libc::time_t = duration.as_secs() as libc::time_t;
    let millis = duration.subsec_millis();

    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `seconds` 与 `tm` 都是调用期间有效的栈/局部值，localtime_r 是线程安全变体。
    let result = unsafe { libc::localtime_r(&seconds, &mut tm) };
    if result.is_null() {
        return format!("00:00:00.{millis:03}");
    }

    format!(
        "{:02}:{:02}:{:02}.{:03}",
        tm.tm_hour, tm.tm_min, tm.tm_sec, millis
    )
}

pub fn now_string() -> String {
    format_time(SystemTime::now())
}

fn glyph_index(byte: u8) -> Option<usize> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as usize),
        b':' => Some(10),
        b'.' => Some(11),
        _ => None,
    }
}

/// 在帧的 Y 平面上绘制实时时间戳水印（不依赖 drawtext 滤镜）。
///
/// `y` 是 Y 平面数据，`linesize` 是每行字节数；适用于 yuv420p / nv12 等
/// Y 平面在最前、每像素 1 字节的格式。越界部分按原实现裁剪。
pub fn draw_timestamp(
    y: &mut [u8],
    linesize: usize,
    width: i32,
    height: i32,
    text: &str,
    scale: i32,
) {
    if scale <= 0 || width <= 0 || height <= 0 || linesize == 0 {
        return;
    }

    let len = text.len() as i32;
    let char_w = 5 * scale;
    let char_h = 7 * scale;
    let gap = scale;
    let pad = 2 * scale;
    let text_w = len * (char_w + gap) - gap;
    let box_w = text_w + 2 * pad;
    let box_h = char_h + 2 * pad;
    let x0 = width - box_w - 4 * scale;
    let y0 = 4 * scale;

    // 黑色背景框
    for yy in 0..box_h {
        let py = y0 + yy;
        if !(0..height).contains(&py) {
            continue;
        }
        for xx in 0..box_w {
            let px = x0 + xx;
            if !(0..width).contains(&px) {
                continue;
            }
            let index = py as usize * linesize + px as usize;
            if index < y.len() {
                y[index] = 16;
            }
        }
    }

    // 逐字符绘制（白色）
    for (column, byte) in text.bytes().enumerate() {
        let Some(glyph) = glyph_index(byte) else {
            continue;
        };
        let cx = x0 + pad + column as i32 * (char_w + gap);
        let cy = y0 + pad;
        for (row, &bits) in FONT_5X7[glyph].iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for sy in 0..scale {
                    let py = cy + row as i32 * scale + sy;
                    if !(0..height).contains(&py) {
                        continue;
                    }
                    for sx in 0..scale {
                        let px = cx + col * scale + sx;
                        if !(0..width).contains(&px) {
                            continue;
                        }
                        let index = py as usize * linesize + px as usize;
                        if index < y.len() {
                            y[index] = 255;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn time_has_millisecond_precision() {
        let text = format_time(UNIX_EPOCH + Duration::from_millis(1_700_000_000_123));
        assert_eq!(text.len(), 12);
        let bytes = text.as_bytes();
        assert_eq!(bytes[2], b':');
        assert_eq!(bytes[5], b':');
        assert_eq!(bytes[8], b'.');
        assert!(bytes[9..12].iter().all(|b| b.is_ascii_digit()));
    }

    #[test]
    fn draws_box_and_white_glyphs() {
        let (width, height) = (200, 100);
        let mut y = vec![0u8; width * height];
        draw_timestamp(
            &mut y,
            width,
            width as i32,
            height as i32,
            "12:34:56.789",
            2,
        );

        // len=12, char_w=10, gap=2, pad=4 -> box 150x22 at (42, 8)
        assert_eq!(y[8 * width + 42], 16);
        assert_eq!(y[(8 + 21) * width + (42 + 149)], 16);
        assert_eq!(y[0], 0);
        assert!(y.contains(&255), "应绘制白色字形");
    }

    #[test]
    fn clips_out_of_bounds_without_panicking() {
        let (width, height) = (20, 10);
        let mut y = vec![0u8; width * height];
        draw_timestamp(
            &mut y,
            width,
            width as i32,
            height as i32,
            "12:34:56.789",
            2,
        );
        // 水印框大部分在画面外，不崩溃即可；边界内应至少画出背景。
        assert!(y.contains(&16));
    }

    #[test]
    fn empty_text_still_draws_background_box() {
        let (width, height) = (100, 50);
        let mut y = vec![0u8; width * height];
        draw_timestamp(&mut y, width, width as i32, height as i32, "", 1);
        assert!(y.contains(&16));
        assert!(!y.contains(&255));
    }

    #[test]
    fn non_font_bytes_are_skipped() {
        let (width, height) = (100, 50);
        let mut y = vec![0u8; width * height];
        draw_timestamp(&mut y, width, width as i32, height as i32, "中", 1);
        assert!(y.contains(&16));
        assert!(!y.contains(&255));
    }
}
