use serde::{Deserialize, Serialize};

#[cfg(test)]
fn hash_seed(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn lerp(self, other: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Rgb::new(f(self.r, other.r), f(self.g, other.g), f(self.b, other.b))
    }

    pub fn scale(self, f: f64) -> Rgb {
        Rgb::new(
            (self.r as f64 * f).clamp(0.0, 255.0) as u8,
            (self.g as f64 * f).clamp(0.0, 255.0) as u8,
            (self.b as f64 * f).clamp(0.0, 255.0) as u8,
        )
    }

    /// Rotate hue by `degrees` (HSL space, keeps lightness).
    pub fn hue_shift(self, degrees: f64) -> Rgb {
        let (h, s, l) = rgb_to_hsl(self);
        hsl_to_rgb((h + degrees / 360.0).rem_euclid(1.0), s, l)
    }

    pub fn to_color(self) -> ratatui::style::Color {
        ratatui::style::Color::Rgb(self.r, self.g, self.b)
    }
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub name: &'static str,
    pub stops: Vec<Rgb>,
}

impl Palette {
    /// Sample the gradient at t (wraps). Stops are evenly spaced.
    pub fn sample(&self, t: f64) -> Rgb {
        let n = self.stops.len();
        if n == 0 {
            return Rgb::default();
        }
        if n == 1 {
            return self.stops[0];
        }
        // Clamp to [0,1] rather than wrapping: callers can add their own phase
        // if they want cycling. This makes sample(1.0) exactly the last stop.
        let t = t.clamp(0.0, 1.0);
        let pos = t * (n - 1) as f64;
        let i = pos.floor() as usize;
        let j = (i + 1).min(n - 1);
        self.stops[i.min(n - 1)].lerp(self.stops[j], pos.fract())
    }

    pub fn all() -> Vec<Palette> {
        vec![
            Palette {
                name: "ember",
                stops: vec![
                    Rgb::new(255, 60, 0),
                    Rgb::new(255, 180, 40),
                    Rgb::new(120, 10, 60),
                    Rgb::new(20, 4, 30),
                ],
            },
            Palette {
                name: "lagoon",
                stops: vec![
                    Rgb::new(0, 220, 190),
                    Rgb::new(0, 120, 255),
                    Rgb::new(60, 0, 120),
                    Rgb::new(0, 20, 40),
                ],
            },
            Palette {
                name: "orchard",
                stops: vec![
                    Rgb::new(250, 90, 160),
                    Rgb::new(140, 60, 220),
                    Rgb::new(40, 200, 180),
                    Rgb::new(250, 220, 90),
                ],
            },
            Palette {
                name: "polaris",
                stops: vec![
                    Rgb::new(200, 220, 255),
                    Rgb::new(90, 140, 255),
                    Rgb::new(20, 40, 120),
                    Rgb::new(4, 8, 30),
                ],
            },
            Palette {
                name: "rainforest",
                stops: vec![
                    Rgb::new(40, 220, 100),
                    Rgb::new(0, 140, 90),
                    Rgb::new(200, 230, 60),
                    Rgb::new(10, 40, 20),
                ],
            },
            Palette {
                name: "monolith",
                stops: vec![
                    Rgb::new(240, 240, 245),
                    Rgb::new(150, 150, 160),
                    Rgb::new(60, 60, 70),
                    Rgb::new(12, 12, 16),
                ],
            },
        ]
    }

    pub fn by_name(name: &str) -> Option<Palette> {
        Palette::all().into_iter().find(|p| p.name == name)
    }

    #[cfg(test)]
    pub fn from_prompt(name: &str) -> Option<Palette> {
        use rand::{SeedableRng, seq::IndexedRandom};
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let seed = {
            let mut h = DefaultHasher::new();
            name.hash(&mut h);
            h.finish()
        };
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
        let all = Palette::all();
        all.choose(&mut rng).cloned()
    }

    pub fn random(rng: &mut rand_chacha::ChaCha8Rng) -> Palette {
        use rand::seq::IndexedRandom;
        let all = Palette::all();
        all.choose(rng).cloned().unwrap_or_else(|| all[0].clone())
    }
}

fn rgb_to_hsl(c: Rgb) -> (f64, f64, f64) {
    let r = c.r as f64 / 255.0;
    let g = c.g as f64 / 255.0;
    let b = c.b as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Rgb {
    // Standard HSL-to-RGB using a helper over k ∈ {0, 8, 4}.
    let a = s * l.min(1.0 - l);
    let f = |n: f64| {
        let k = (n + h * 12.0) % 12.0;
        let q = |t: f64| t.clamp(-1.0, 1.0);
        l - a * (q(k - 3.0).min(q(9.0 - k).min(1.0)) - q(k))
    };
    Rgb::new(
        (f(0.0) * 255.0).clamp(0.0, 255.0) as u8,
        (f(8.0) * 255.0).clamp(0.0, 255.0) as u8,
        (f(4.0) * 255.0).clamp(0.0, 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn lerp_midpoint() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(100, 200, 50);
        assert_eq!(a.lerp(b, 0.5), Rgb::new(50, 100, 25));
    }

    #[test]
    fn lerp_clamps_t() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(10, 10, 10);
        assert_eq!(a.lerp(b, 2.0), b);
        assert_eq!(a.lerp(b, -1.0), a);
    }

    #[test]
    fn sample_wraps_gradient() {
        let p = Palette {
            name: "t",
            stops: vec![Rgb::new(0, 0, 0), Rgb::new(255, 255, 255)],
        };
        assert_eq!(p.sample(0.0), Rgb::new(0, 0, 0));
        assert_eq!(p.sample(0.5), Rgb::new(128, 128, 128));
        assert_eq!(p.sample(1.0), Rgb::new(255, 255, 255));
        // Negative / out-of-range values clamp rather than wrap.
        assert_eq!(p.sample(-0.25), p.sample(0.0));
        assert_eq!(p.sample(1.5), p.sample(1.0));
    }

    #[test]
    fn by_name_finds_known_palette() {
        assert!(Palette::by_name("ember").is_some());
        assert!(Palette::by_name("nope").is_none());
    }

    #[test]
    fn random_is_deterministic_with_seed() {
        let mut r1 = rand_chacha::ChaCha8Rng::seed_from_u64(7);
        let mut r2 = rand_chacha::ChaCha8Rng::seed_from_u64(7);
        assert_eq!(Palette::random(&mut r1).name, Palette::random(&mut r2).name);
    }

    #[test]
    fn hue_shift_preserves_value_broadly() {
        // 0° red, 120° green, 240° blue in a healthy HSL conversion.
        let red = Rgb::new(255, 0, 0);
        let greenish = red.hue_shift(120.0);
        assert!(
            greenish.g > greenish.r,
            "expected greenish shift, got {greenish:?}"
        );
        assert!(greenish.g > greenish.b);
    }

    #[test]
    fn hash_seed_is_stable() {
        assert_eq!(hash_seed("neon tidepool"), hash_seed("neon tidepool"));
        assert_ne!(hash_seed("a"), hash_seed("b"));
    }
}
