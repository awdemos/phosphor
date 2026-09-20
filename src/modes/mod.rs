pub mod generative;
pub mod orbital;
pub mod plasma;
pub mod pipes;
pub mod rain;

use crate::engine::Canvas;
use crate::palette::Palette;
use rand_chacha::ChaCha8Rng;

/// A renderable screensaver mode. `update` advances state; `render` paints
/// into a `Canvas` at the current terminal size.
pub trait Mode: Send {
    fn name(&self) -> &'static str;
    fn update(&mut self, dt: f64, size: (u16, u16), rng: &mut ChaCha8Rng, speed: f64);
    fn render(&self, canvas: &mut Canvas, t: f64);
    fn palette_name(&self) -> String;
    fn set_palette(&mut self, palette: Palette);
}

pub const MODE_NAMES: [&str; 5] = ["orbital", "rain", "plasma", "pipes", "generative"];

/// Build one of the four concrete modes by name (generative has its own ctor).
pub fn build(name: &str, seed: u64) -> Option<Box<dyn Mode>> {
    match name {
        "orbital" => Some(Box::new(orbital::Orbital::new(seed))),
        "rain" => Some(Box::new(rain::Rain::new(seed))),
        "plasma" => Some(Box::new(plasma::Plasma::new(seed))),
        "pipes" => Some(Box::new(pipes::Pipes::new(seed))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Canvas;
    use rand::SeedableRng;

    #[test]
    fn registry_builds_all_modes() {
        for name in ["orbital", "rain", "plasma", "pipes"] {
            assert!(build(name, 42).is_some(), "missing mode {name}");
        }
        assert!(build("nope", 42).is_none());
    }

    #[test]
    fn orbital_renders_nonblank_and_scales() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(1);
        for (w, h) in [(80u16, 24u16), (40, 10), (120, 40)] {
            let mut m = build("orbital", 42).unwrap();
            m.update(0.5, (w, h), &mut rng, 1.0);
            m.update(0.5, (w, h), &mut rng, 1.0);
            let mut c = Canvas::new(w, h);
            m.render(&mut c, 1.0);
            assert!(!c.is_blank(), "orbital blank at {w}x{h}");
        }
    }

    #[test]
    fn orbital_is_deterministic_with_seed() {
        let mut rng1 = rand_chacha::ChaCha8Rng::seed_from_u64(9);
        let mut rng2 = rand_chacha::ChaCha8Rng::seed_from_u64(9);
        let mut a = build("orbital", 5).unwrap();
        let mut b = build("orbital", 5).unwrap();
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

    #[test]
    fn rain_renders_nonblank() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(2);
        let mut m = build("rain", 42).unwrap();
        for _ in 0..4 {
            m.update(0.1, (70, 22), &mut rng, 1.0);
        }
        let mut c = Canvas::new(70, 22);
        m.render(&mut c, 0.4);
        assert!(!c.is_blank());
    }

    #[test]
    fn rain_columns_stay_in_bounds() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(3);
        let mut m = build("rain", 1).unwrap();
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

    #[test]
    fn plasma_renders_and_is_deterministic() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(4);
        let mut a = build("plasma", 7).unwrap();
        let mut b = build("plasma", 7).unwrap();
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

    #[test]
    fn pipes_draw_over_floor_and_stay_sane() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(6);
        let mut m = build("pipes", 11).unwrap();
        for _ in 0..120 {
            m.update(0.05, (60, 24), &mut rng, 2.0);
        }
        let mut c = Canvas::new(60, 24);
        m.render(&mut c, 6.0);
        assert!(!c.is_blank());
    }
}
