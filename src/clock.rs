use crate::engine::Canvas;
use crate::palette::Rgb;

/// 3x5 pixel font: index 0..=9 digits, 10 = ':'.
pub const GLYPHS: [[&str; 5]; 11] = [
    ["###", "# #", "# #", "# #", "###"], // 0
    ["  #", "  #", "  #", "  #", "  #"], // 1
    ["###", "  #", "###", "#  ", "###"], // 2
    ["###", "  #", "###", "  #", "###"], // 3
    ["# #", "# #", "###", "  #", "  #"], // 4
    ["###", "#  ", "###", "  #", "###"], // 5
    ["###", "#  ", "###", "# #", "###"], // 6
    ["###", "  #", "  #", "  #", "  #"], // 7
    ["###", "# #", "###", "# #", "###"], // 8
    ["###", "# #", "###", "  #", "###"], // 9
    ["   ", " # ", "   ", " # ", "   "], // :
];

/// Draw HH:MM (or HH:MM:SS) with top-left at (cx, cy), doubled horizontally
/// for terminal aspect ratio.
pub fn draw_clock(canvas: &mut Canvas, cx: i32, cy: i32, t_secs: f64, fg: Rgb, show_seconds: bool) {
    let h = (t_secs / 3600.0) as u32 % 24;
    let m = (t_secs / 60.0) as u32 % 60;
    let s = t_secs as u32 % 60;
    let digits: Vec<usize> = if show_seconds {
        vec![
            (h / 10) as usize,
            (h % 10) as usize,
            10,
            (m / 10) as usize,
            (m % 10) as usize,
            10,
            (s / 10) as usize,
            (s % 10) as usize,
        ]
    } else {
        vec![
            (h / 10) as usize,
            (h % 10) as usize,
            10,
            (m / 10) as usize,
            (m % 10) as usize,
        ]
    };
    let mut x = cx;
    for d in digits {
        let g = &GLYPHS[d];
        for (row, line) in g.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                if ch == '#' {
                    canvas.put(x + col as i32, cy + row as i32, '█', fg);
                    canvas.put(x + col as i32 + 1, cy + row as i32, '█', fg);
                }
            }
        }
        x += 8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_hh_mm_with_colon() {
        let mut c = Canvas::new(40, 7);
        draw_clock(
            &mut c,
            1,
            1,
            9.0 * 3600.0 + 5.0 * 60.0,
            Rgb::new(255, 255, 255),
            false,
        );
        let mut lit = 0;
        for y in 0..7 {
            for x in 0..40 {
                if c.get(x, y).map(|c| c.ch).unwrap_or(' ') != ' ' {
                    lit += 1;
                }
            }
        }
        assert!(
            lit > 40,
            "clock should light up a bunch of cells, got {lit}"
        );
    }

    #[test]
    fn every_digit_glyph_is_three_wide_five_tall() {
        for g in GLYPHS {
            assert_eq!(g.len(), 5);
            for row in g {
                assert_eq!(row.chars().count(), 3, "row {row:?} not 3 wide");
            }
        }
    }
}
