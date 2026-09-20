use super::Mode;
use crate::engine::Canvas;
use crate::glyph::ramp_char;
use crate::palette::Palette;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Dir {
    x: i32,
    y: i32,
}

struct Walker {
    x: i32,
    y: i32,
    dir: Dir,
    life: f64,
    phase: f64,
    step_acc: f64,
}

pub struct Pipes {
    palette: Palette,
    walkers: Vec<Walker>,
    grid: HashMap<(i32, i32), (char, f64)>,
    hue: f64,
}

impl Pipes {
    pub fn new(seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let walkers = (0..3).map(|_| Self::spawn(&mut rng, 80, 24)).collect();
        Self {
            palette: Palette::by_name("lagoon").unwrap_or_else(|| Palette::all().remove(0)),
            walkers,
            grid: HashMap::new(),
            hue: 0.0,
        }
    }

    fn spawn(rng: &mut ChaCha8Rng, w: i32, h: i32) -> Walker {
        let dirs = [
            Dir { x: 1, y: 0 },
            Dir { x: -1, y: 0 },
            Dir { x: 0, y: 1 },
            Dir { x: 0, y: -1 },
        ];
        Walker {
            x: rng.random_range(2..w.saturating_sub(2)),
            y: rng.random_range(2..h.saturating_sub(2)),
            dir: dirs[rng.random_range(0..4)],
            life: 8.0 + rng.random::<f64>() * 20.0,
            phase: rng.random::<f64>(),
            step_acc: 0.0,
        }
    }

    fn corner(d_in: Dir, d_out: Dir) -> char {
        match ((d_in.x, d_in.y), (d_out.x, d_out.y)) {
            ((1, 0), (0, 1)) | ((0, -1), (-1, 0)) => '┐',
            ((1, 0), (0, -1)) | ((0, 1), (-1, 0)) => '┘',
            ((-1, 0), (0, 1)) | ((0, -1), (1, 0)) => '┌',
            ((-1, 0), (0, -1)) | ((0, 1), (1, 0)) => '└',
            _ if d_out.x != 0 => '─',
            _ => '│',
        }
    }
}

impl Mode for Pipes {
    fn name(&self) -> &'static str {
        "pipes"
    }

    fn update(&mut self, dt: f64, size: (u16, u16), rng: &mut ChaCha8Rng, speed: f64) {
        let dt = dt * speed;
        self.hue = (self.hue + dt * 0.01) % 1.0;
        let (w, h) = (size.0 as i32, size.1 as i32);
        for walker in &mut self.walkers {
            walker.life -= dt;
            walker.step_acc += dt * 9.0;
            while walker.step_acc >= 1.0 {
                walker.step_acc -= 1.0;
                let old = walker.dir;
                // 25% chance to turn 90° at a grid step.
                if rng.random::<f64>() < 0.25 {
                    let turn = if rng.random::<bool>() { 1 } else { -1 };
                    walker.dir = Dir {
                        x: -old.y * turn,
                        y: old.x * turn,
                    };
                }
                let nx = walker.x + walker.dir.x;
                let ny = walker.y + walker.dir.y;
                if walker.life <= 0.0
                    || nx < 1
                    || ny < 1
                    || nx >= w - 1
                    || ny >= h - 1
                {
                    *walker = Self::spawn(rng, w, h);
                    break;
                }
                walker.x = nx;
                walker.y = ny;
                if walker.dir != old {
                    self.grid.insert(
                        (walker.x - old.x, walker.y - old.y),
                        (Self::corner(old, walker.dir), walker.phase),
                    );
                }
                let body = if walker.dir.x != 0 { '─' } else { '│' };
                self.grid.insert((walker.x, walker.y), (body, walker.phase));
            }
        }
        // Fade the oldest trails so the floor doesn't fill solid.
        if self.grid.len() > 4000 {
            let threshold = (self.hue % 1.0) - 0.5;
            let len = self.grid.len();
            self.grid.retain(|_, (_, ph)| *ph > threshold || len <= 4000);
        }
    }

    fn render(&self, canvas: &mut Canvas, t: f64) {
        let (w, h) = (canvas.width as f64, canvas.height as f64);
        let (cx, cy) = (w / 2.0, h / 2.0);
        // Dim plasma floor.
        for y in 0..canvas.height as i32 {
            for x in 0..canvas.width as i32 {
                let d = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt() * 0.14;
                let level = ((x as f64 * 0.09 + t * 0.5).sin()
                    + (y as f64 * 0.12 - t).sin()
                    + (d - t * 0.7).sin())
                    / 6.0
                    + 0.32;
                if level > 0.4 {
                    canvas.put(
                        x,
                        y,
                        ramp_char((level - 0.4) * 0.8),
                        self.palette.sample(self.hue + level * 0.4).scale(0.25),
                    );
                }
            }
        }
        // Pipe trails.
        for (&(x, y), &(ch, phase)) in &self.grid {
            canvas.put(
                x,
                y,
                ch,
                self.palette
                    .sample(phase * 0.5 + self.hue)
                    .hue_shift(phase * 60.0),
            );
        }
        // Walker heads.
        for walker in &self.walkers {
            canvas.put(walker.x, walker.y, '█', self.palette.sample(0.9));
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
    fn draws_over_floor_and_stays_sane() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(6);
        let mut m = Pipes::new(11);
        for _ in 0..120 {
            m.update(0.05, (60, 24), &mut rng, 2.0);
        }
        let mut c = Canvas::new(60, 24);
        m.render(&mut c, 6.0);
        assert!(!c.is_blank());
    }
}
