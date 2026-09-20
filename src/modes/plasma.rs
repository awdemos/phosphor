use super::Mode;
use crate::engine::Canvas;
use crate::glyph::ramp_char;
use crate::palette::Palette;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

pub struct Plasma {
    palette: Palette,
    hue: f64,
}

impl Plasma {
    pub fn new(seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let palette = Palette::random(&mut rng);
        Self { palette, hue: 0.0 }
    }
}

impl Mode for Plasma {
    fn name(&self) -> &'static str {
        "plasma"
    }

    fn update(&mut self, dt: f64, _size: (u16, u16), _rng: &mut ChaCha8Rng, speed: f64) {
        self.hue = (self.hue + dt * speed * 0.02) % 1.0;
    }

    fn render(&self, canvas: &mut Canvas, t: f64) {
        let (w, h) = (canvas.width as f64, canvas.height as f64);
        let (cx, cy) = (w / 2.0, h / 2.0);
        for y in 0..canvas.height as i32 {
            for x in 0..canvas.width as i32 {
                let u = x as f64;
                let v = y as f64;
                let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt();
                let val = (u * 0.11 + t).sin()
                    + (v * 0.13 - t * 1.2).sin()
                    + ((u + v) * 0.07 + t * 0.6).sin()
                    + (d * 0.16 - t * 0.9).sin();
                let level = (val / 4.0) * 0.5 + 0.5;
                let ch = ramp_char(level);
                if ch != ' ' {
                    canvas.put(
                        x,
                        y,
                        ch,
                        self.palette
                            .sample(level * 0.5 + self.hue)
                            .hue_shift(self.hue * 360.0),
                    );
                }
            }
        }
    }

    fn palette_name(&self) -> String {
        self.palette.name.to_string()
    }

    fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Canvas;
    use rand::SeedableRng;

    #[test]
    fn renders_and_is_deterministic() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(4);
        let mut a = Plasma::new(7);
        let mut b = Plasma::new(7);
        for _ in 0..3 {
            a.update(0.2, (50, 18), &mut rng, 1.0);
        }
        let mut rng2 = rand_chacha::ChaCha8Rng::seed_from_u64(4);
        for _ in 0..3 {
            b.update(0.2, (50, 18), &mut rng2, 1.0);
        }
        let mut ca = Canvas::new(50, 18);
        let mut cb = Canvas::new(50, 18);
        a.render(&mut ca, 0.6);
        b.render(&mut cb, 0.6);
        assert_eq!(ca.get(25, 9), cb.get(25, 9));
        assert!(!ca.is_blank());
    }
}
