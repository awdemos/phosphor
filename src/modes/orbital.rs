use super::Mode;
use crate::clock::draw_clock;
use crate::engine::Canvas;
use crate::glyph::ramp_char;
use crate::palette::{Palette, Rgb};
use rand::{Rng, SeedableRng};
use rand::seq::IndexedRandom;
use rand_chacha::ChaCha8Rng;

struct Star {
    x: f64,
    y: f64,
    layer: usize,
    twinkle: f64,
}

struct Ring {
    rx: f64,
    ry: f64,
    angle: f64,
    spin: f64,
    sats: Vec<(f64, f64)>,
}

const TICKER_BASE: &str = "tok/s {TOK} · gpu {GPU}C · p99 {P99}ms · agents {AGENTS} · entropy {ENT} · mood {MOOD} · orbit stable · phosphor v0.1";

pub struct Orbital {
    palette: Palette,
    stars: Vec<Star>,
    rings: Vec<Ring>,
    ticker: String,
    ticker_scroll: f64,
    t: f64,
}

impl Orbital {
    pub fn new(seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut stars = Vec::new();
        for layer in 0..3 {
            let count = 24 + layer * 18;
            for _ in 0..count {
                stars.push(Star {
                    x: rng.random::<f64>(),
                    y: rng.random::<f64>(),
                    layer,
                    twinkle: rng.random::<f64>() * std::f64::consts::TAU,
                });
            }
        }
        let mut rings = Vec::new();
        for i in 0..3 {
            let sats = (0..2 + i)
                .map(|_| {
                    (
                        rng.random::<f64>() * std::f64::consts::TAU,
                        0.15 + rng.random::<f64>() * 0.5,
                    )
                })
                .collect();
            rings.push(Ring {
                rx: 0.16 + 0.11 * i as f64,
                ry: 0.07 + 0.05 * i as f64,
                angle: rng.random::<f64>() * std::f64::consts::TAU,
                spin: (0.05 + rng.random::<f64>() * 0.1)
                    * if i % 2 == 0 { 1.0 } else { -1.0 },
                sats,
            });
        }
        let mut me = Self {
            palette: Palette::by_name("polaris").unwrap_or_else(|| Palette::all().remove(0)),
            stars,
            rings,
            ticker: TICKER_BASE.to_string(),
            ticker_scroll: 0.0,
            t: 0.0,
        };
        me.refresh_ticker(&mut rng);
        me
    }

    fn refresh_ticker(&mut self, rng: &mut ChaCha8Rng) {
        let mut s = TICKER_BASE.to_string();
        let tok = 500.0 + rng.random::<f64>() * 700.0;
        let gpu = 58.0 + rng.random::<f64>() * 24.0;
        let p99 = 40.0 + rng.random::<f64>() * 120.0;
        let agents = rng.random_range(3..24);
        let ent = rng.random::<f64>();
        let mood: &str = *[
            "luminous",
            "restless",
            "serene",
            "electric",
            "cryptic",
        ]
        .choose(rng)
        .unwrap_or(&"luminous");
        for (k, v) in [
            ("{TOK}", format!("{tok:.0}")),
            ("{GPU}", format!("{gpu:.0}")),
            ("{P99}", format!("{p99:.0}")),
            ("{AGENTS}", agents.to_string()),
            ("{ENT}", format!("{ent:.3}")),
            ("{MOOD}", mood.to_string()),
        ] {
            s = s.replace(k, &v);
        }
        self.ticker = s;
    }
}

impl Mode for Orbital {
    fn name(&self) -> &'static str {
        "orbital"
    }

    fn update(&mut self, dt: f64, _size: (u16, u16), rng: &mut ChaCha8Rng, speed: f64) {
        let dt = dt * speed;
        self.t += dt;
        for star in &mut self.stars {
            star.x -= dt * 0.004 * (star.layer + 1) as f64;
            if star.x < 0.0 {
                star.x += 1.0;
            }
        }
        for ring in &mut self.rings {
            ring.angle += ring.spin * dt;
        }
        self.ticker_scroll += dt * 12.0;
        let len = self.ticker.chars().count() as f64 + 40.0;
        if self.ticker_scroll > len {
            self.ticker_scroll -= len;
            self.refresh_ticker(rng);
        }
    }

    fn render(&self, canvas: &mut Canvas, t: f64) {
        let (w, h) = (canvas.width as i32, canvas.height as i32);
        // Nebula floor: layered sines, very dim.
        for y in 0..h.saturating_sub(1) {
            for x in 0..w {
                let u = x as f64 / w.max(1) as f64 * 6.0;
                let v = y as f64 / h.max(1) as f64 * 4.0;
                let n = (u.sin() * (v * 0.7 + t * 0.11).cos()
                    + (u * 0.6 - t * 0.07).sin() * v.sin())
                    * 0.25
                    + 0.5;
                if n > 0.72 {
                    let level = ((n - 0.72) / 0.28) * 0.35;
                    canvas.put(
                        x,
                        y,
                        ramp_char(level),
                        self.palette.sample(n * 0.6).scale(0.35),
                    );
                }
            }
        }
        // Stars.
        for star in &self.stars {
            let x = (star.x * w as f64) as i32;
            let y = (star.y * (h - 1) as f64) as i32;
            let tw = (t * (1.0 + star.layer as f64) + star.twinkle).sin() * 0.5 + 0.5;
            let ch = if star.layer == 2 { '*' } else { '.' };
            canvas.put(
                x,
                y,
                ch,
                self.palette
                    .sample(0.15 + 0.1 * star.layer as f64 + tw * 0.2)
                    .scale(0.4 + tw * 0.6),
            );
        }
        // Rings + satellites, centered in the sky region (above the ticker).
        let cx = w as f64 / 2.0;
        let cy = (h - 1) as f64 / 2.0;
        let rw = w as f64;
        let rh = (h - 1) as f64;
        for ring in &self.rings {
            let ca = ring.angle.cos();
            let sa = ring.angle.sin();
            let steps = 90;
            for i in 0..steps {
                let a = i as f64 / steps as f64 * std::f64::consts::TAU;
                let px = a.cos() * ring.rx * rw;
                let py = a.sin() * ring.ry * rh;
                let x = (cx + px * ca - py * sa) as i32;
                let y = (cy + px * sa + py * ca) as i32;
                canvas.put(x, y, '·', self.palette.sample(0.4).scale(0.55));
            }
            for (phase, spd) in &ring.sats {
                let a = phase + t * spd;
                let px = a.cos() * ring.rx * rw;
                let py = a.sin() * ring.ry * rh;
                let x = (cx + px * ca - py * sa) as i32;
                let y = (cy + px * sa + py * ca) as i32;
                canvas.put(x, y, '◆', self.palette.sample(0.75));
                canvas.put(
                    x + 1,
                    y,
                    '»',
                    self.palette.sample(0.75).scale(0.5),
                );
            }
        }
        // Clock, top center.
        let hhmm = 4 * 8 - 2; // "HH:MM" = 4 digits * 8 + colon gap
        let clock_x = (w - hhmm) / 2;
        let local = t % 86400.0;
        draw_clock(
            canvas,
            clock_x.max(0),
            1,
            local,
            Rgb::new(220, 230, 255),
            false,
        );
        // Ticker along the bottom row.
        let ty = h - 1;
        let chars: Vec<char> = self.ticker.chars().collect();
        let len = chars.len() as i32;
        let off = self.ticker_scroll as i32;
        for x in 0..w {
            let i = (x + off) % (len + 8);
            if i < len {
                canvas.put(x, ty, chars[i as usize], self.palette.sample(0.85).scale(0.9));
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
    fn renders_nonblank() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        for (w, h) in [(80u16, 24u16), (40, 10), (120, 40)] {
            let mut m = Orbital::new(42);
            m.update(0.5, (w, h), &mut rng, 1.0);
            m.update(0.5, (w, h), &mut rng, 1.0);
            let mut c = Canvas::new(w, h);
            m.render(&mut c, 1.0);
            assert!(!c.is_blank(), "orbital blank at {w}x{h}");
        }
    }

    #[test]
    fn deterministic_with_seed() {
        let mut rng1 = ChaCha8Rng::seed_from_u64(9);
        let mut rng2 = ChaCha8Rng::seed_from_u64(9);
        let mut a = Orbital::new(5);
        let mut b = Orbital::new(5);
        for _ in 0..5 {
            a.update(0.1, (60, 20), &mut rng1, 1.0);
            b.update(0.1, (60, 20), &mut rng2, 1.0);
        }
        let mut ca = Canvas::new(60, 20);
        let mut cb = Canvas::new(60, 20);
        a.render(&mut ca, 0.5);
        b.render(&mut cb, 0.5);
        assert_eq!(ca.get(10, 5), cb.get(10, 5));
        assert_eq!(ca.get(30, 12), cb.get(30, 12));
    }
}
