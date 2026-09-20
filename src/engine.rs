use crate::palette::Rgb;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
}

pub struct Canvas {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
}

impl Canvas {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::default(); width as usize * height as usize],
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.cells.clear();
        self.cells
            .resize(width as usize * height as usize, Cell::default());
    }

    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some(y as usize * self.width as usize + x as usize)
    }

    pub fn put(&mut self, x: i32, y: i32, ch: char, fg: Rgb) {
        if let Some(i) = self.idx(x, y) {
            self.cells[i] = Cell { ch, fg, bg: None };
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Cell> {
        self.idx(x, y).map(|i| self.cells[i])
    }

    #[cfg(test)]
    pub fn is_blank(&self) -> bool {
        self.cells.iter().all(|c| c.ch == ' ' || c.ch == '\0')
    }

    pub fn scale_colors(&mut self, factor: f64) {
        for cell in &mut self.cells {
            cell.fg = cell.fg.scale(factor);
        }
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, fg: Rgb) {
        for (i, ch) in s.chars().enumerate() {
            self.put(x + i as i32, y, ch, fg);
        }
    }

    /// Crossfade with `other` (t=0 → self, t=1 → other) by blending
    /// per-cell luminance through the glyph ramp.
    pub fn blend(&self, other: &Canvas, t: f64) -> Canvas {
        let t = t.clamp(0.0, 1.0);
        let (w, h) = (self.width.min(other.width), self.height.min(other.height));
        let mut out = Canvas::new(self.width, self.height);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let a = self.get(x, y).unwrap_or_default();
                let b = other.get(x, y).unwrap_or_default();
                let la = crate::glyph::ramp_level(a.ch);
                let lb = crate::glyph::ramp_level(b.ch);
                let level = la + (lb - la) * t;
                let fg = a.fg.lerp(b.fg, t);
                let ch = if level <= 0.0 && a.ch == ' ' && b.ch == ' ' {
                    ' '
                } else {
                    crate::glyph::ramp_char(level)
                };
                out.put(x, y, ch, fg);
            }
        }
        out
    }

    pub fn paint(&self, frame: &mut ratatui::Frame) {
        let buf = frame.buffer_mut();
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                let Some(cell) = self.get(x, y) else { continue };
                if cell.ch == ' ' || cell.ch == '\0' {
                    continue;
                }
                if let Some(c) = buf.cell_mut((x as u16, y as u16)) {
                    c.set_char(cell.ch).set_fg(cell.fg.to_color());
                    if let Some(bg) = cell.bg {
                        c.set_bg(bg.to_color());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_get_roundtrip() {
        let mut c = Canvas::new(10, 4);
        let fg = crate::palette::Rgb::new(255, 0, 0);
        c.put(3, 2, '#', fg);
        assert_eq!(c.get(3, 2).map(|c| c.ch), Some('#'));
        assert_eq!(c.get(3, 2).map(|c| c.fg), Some(fg));
        assert_eq!(c.get(99, 99), None, "out of bounds reads return None");
    }

    #[test]
    fn text_writes_run() {
        let mut c = Canvas::new(20, 2);
        c.text(5, 1, "hi", crate::palette::Rgb::new(0, 255, 0));
        assert_eq!(c.get(5, 1).map(|c| c.ch), Some('h'));
        assert_eq!(c.get(6, 1).map(|c| c.ch), Some('i'));
        assert_eq!(c.get(7, 1), Some(Cell::default()));
    }

    #[test]
    fn blank_canvas_is_blank() {
        assert!(Canvas::new(8, 3).is_blank());
        let mut c = Canvas::new(8, 3);
        c.put(0, 0, 'x', crate::palette::Rgb::new(1, 2, 3));
        assert!(!c.is_blank());
    }

    #[test]
    fn blend_crossfades_cells() {
        let red = crate::palette::Rgb::new(255, 0, 0);
        let blue = crate::palette::Rgb::new(0, 0, 255);
        let mut a = Canvas::new(2, 1);
        a.put(0, 0, '@', red);
        let mut b = Canvas::new(2, 1);
        b.put(0, 0, ' ', blue);
        let half = a.blend(&b, 0.5);
        let cell = half.get(0, 0).unwrap();
        assert_eq!(cell.fg, crate::palette::Rgb::new(128, 0, 128));
        let full = a.blend(&b, 1.0);
        assert_eq!(full.get(0, 0).map(|c| c.ch), Some(' '));
    }

    #[test]
    fn resize_keeps_size_consistent() {
        let mut c = Canvas::new(4, 4);
        c.resize(6, 2);
        assert_eq!((c.width, c.height), (6, 2));
        assert!(c.is_blank());
    }

    #[test]
    fn scale_colors_dim() {
        let mut c = Canvas::new(2, 1);
        c.put(0, 0, '#', crate::palette::Rgb::new(100, 100, 100));
        c.scale_colors(0.5);
        assert_eq!(
            c.get(0, 0).unwrap().fg,
            crate::palette::Rgb::new(50, 50, 50)
        );
    }
}
