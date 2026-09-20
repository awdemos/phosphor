use super::Mode;
use crate::engine::Canvas;
use crate::glyph::{BRAILLE, KATAKANA, ramp_char};
use crate::palette::Palette;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

struct Column {
    head: f64,
    speed: f64,
    len: f64,
}

pub struct Rain {
    palette: Palette,
    columns: Vec<Column>,
    hue: f64,
    glitch: f64,
}

impl Rain {
    pub fn new(seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut columns = Vec::new();
        for _ in 0..256 {
            columns.push(Column {
                head: rng.random::<f64>() * -40.0,
                speed: 2.0 + rng.random::<f64>() * 6.0,
                len: 6.0 + rng.random::<f64>() * 14.0,
            });
        }
        Self {
            palette: Palette::by_name("rainforest").unwrap_or_else(|| Palette::all().remove(0)),
            columns,
            hue: 0.0,
            glitch: 0.0,
        }
    }

    /// Deterministic glyph for a cell so rain shimmers without extra state.
    fn cell_glyph(&self, x: i32, y: i32, t: f64) -> char {
        let bucket = ((t * 6.0) as i64 ^ (x as i64 * 31) ^ (y as i64 * 17)).unsigned_abs() as usize;
        let set = if bucket % 3 == 0 {
            KATAKANA
        } else {
            BRAILLE
        };
        set[bucket % set.len()]
    }
}

impl Mode for Rain {
    fn name(&self) -> &'static str {
        "rain"
    }

    fn update(&mut self, dt: f64, size: (u16, u16), rng: &mut ChaCha8Rng, speed: f64) {
        let dt = dt * speed;
        self.hue = (self.hue + dt * 0.015) % 1.0;
        self.glitch = (self.glitch - dt).max(0.0);
        if rng.random::<f64>() < dt * 0.06 {
            self.glitch = 0.35;
        }
        let h = size.1 as f64;
        for col in &mut self.columns {
            col.head += col.speed * dt * (h / 24.0);
            if col.head - col.len > h {
                col.head = -rng.random::<f64>() * 30.0;
                col.speed = 2.0 + rng.random::<f64>() * 6.0;
                col.len = 6.0 + rng.random::<f64>() * 14.0;
            }
        }
    }

    fn render(&self, canvas: &mut Canvas, t: f64) {
        let (w, h) = (canvas.width as i32, canvas.height as i32);
        for x in 0..w {
            let col = &self.columns[x as usize % self.columns.len()];
            let head = col.head as i32;
            for i in 0..col.len as i32 {
                let y = head - i;
                if y < 0 || y >= h {
                    continue;
                }
                let fade = 1.0 - i as f64 / col.len;
                let boosted = if self.glitch > 0.0 {
                    (fade + self.glitch * ((x ^ y) % 3) as f64).min(1.0)
                } else {
                    fade
                };
                let ch = if i == 0 {
                    ramp_char(1.0)
                } else {
                    self.cell_glyph(x, y, t)
                };
                let color = self
                    .palette
                    .sample(self.hue + x as f64 / w.max(1) as f64 * 0.25)
                    .hue_shift(self.hue * 360.0 + t * 4.0)
                    .scale(0.15 + boosted * 0.85);
                canvas.put(x, y, ch, color);
            }
        }
    }

    fn palette_name(&self) -> String {
        self.palette.name.to_string()
    }

    fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    fn current_palette(&self) -> &Palette {
        &self.palette
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Canvas;
    use rand::SeedableRng;

    #[test]
    fn renders_nonblank() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(2);
        let mut m = Rain::new(42);
        for _ in 0..4 {
            m.update(0.1, (70, 22), &mut rng, 1.0);
        }
        let mut c = Canvas::new(70, 22);
        m.render(&mut c, 0.4);
        assert!(!c.is_blank());
    }

    #[test]
    fn columns_stay_in_bounds() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(3);
        let mut m = Rain::new(1);
        for _ in 0..200 {
            m.update(0.05, (30, 10), &mut rng, 4.0);
        }
        let mut c = Canvas::new(30, 10);
        m.render(&mut c, 9.9);
        for y in 0..10 {
            for x in 0..30 {
                assert!(c.get(x, y).is_some());
            }
        }
    }
}
