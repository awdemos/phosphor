use rand::Rng;
use rand::seq::IndexedRandom;
use rand_chacha::ChaCha8Rng;

/// Luminance ramp, dimmest to brightest. Used for density-mapped rendering
/// and as the shared alphabet for crossfade blending.
pub const RAMP: &[char] = &[' ', '·', ':', ';', 't', '+', 'n', 'N', 'M', '@'];

pub const KATAKANA: &[char] = &[
    'ｱ', 'ｶ', 'ｻ', 'ﾀ', 'ﾅ', 'ﾊ', 'ﾏ', 'ﾔ', 'ﾗ', 'ﾜ', 'ｦ', 'ﾝ', 'ｼ', 'ｷ', 'ｸ',
];
pub const BRAILLE: &[char] = &['⠁', '⠃', '⠇', '⠧', '⠷', '⠿', '⡿', '⣿'];
pub const ASCII_SET: &[char] = &['0', '1', '%', '$', '#', '@', '=', '+', '*', ':', '.'];
pub const BLOCKS: &[char] = &['░', '▒', '▓', '█'];

pub fn ramp_char(level: f64) -> char {
    let level = level.clamp(0.0, 1.0);
    let i = (level * (RAMP.len() - 1) as f64).round() as usize;
    RAMP[i]
}

/// Luminance of a ramp char in 0..=1; 0.0 for anything not in the ramp.
pub fn ramp_level(ch: char) -> f64 {
    RAMP.iter()
        .position(|&c| c == ch)
        .map_or(0.0, |i| i as f64 / (RAMP.len() - 1) as f64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphSet {
    Katakana,
    Braille,
    Ascii,
    Blocks,
    Mix,
}

impl GlyphSet {
    pub fn from_name(name: &str) -> GlyphSet {
        match name {
            "katakana" => GlyphSet::Katakana,
            "braille" => GlyphSet::Braille,
            "ascii" => GlyphSet::Ascii,
            "blocks" => GlyphSet::Blocks,
            _ => GlyphSet::Mix,
        }
    }

    pub fn random(rng: &mut ChaCha8Rng) -> GlyphSet {
        match rng.random_range(0..4) {
            0 => GlyphSet::Katakana,
            1 => GlyphSet::Braille,
            2 => GlyphSet::Ascii,
            _ => GlyphSet::Blocks,
        }
    }

    pub fn pick(self, rng: &mut ChaCha8Rng) -> char {
        let set: &[char] = match self {
            GlyphSet::Katakana => KATAKANA,
            GlyphSet::Braille => BRAILLE,
            GlyphSet::Ascii => ASCII_SET,
            GlyphSet::Blocks => BLOCKS,
            GlyphSet::Mix => [KATAKANA, BRAILLE, ASCII_SET, BLOCKS]
                .choose(rng)
                .copied()
                .unwrap_or(ASCII_SET),
        };
        set.choose(rng).copied().unwrap_or('·')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn ramp_endpoints() {
        assert_eq!(ramp_char(0.0), ' ');
        assert_eq!(ramp_char(1.0), RAMP[RAMP.len() - 1]);
        assert_eq!(ramp_char(-5.0), ' ');
        assert_eq!(ramp_char(9.9), RAMP[RAMP.len() - 1]);
    }

    #[test]
    fn ramp_monotonic() {
        let a = ramp_level(ramp_char(0.2));
        let b = ramp_level(ramp_char(0.6));
        assert!(a < b);
    }

    #[test]
    fn ramp_level_of_space_is_zero() {
        assert_eq!(ramp_level(' '), 0.0);
        assert!(ramp_level('@') > 0.9);
        assert_eq!(ramp_level('☆'), 0.0, "unknown chars are transparent");
    }

    #[test]
    fn glyph_sets_resolve_names() {
        assert_eq!(GlyphSet::from_name("braille"), GlyphSet::Braille);
        assert_eq!(GlyphSet::from_name("whatever"), GlyphSet::Mix);
    }

    #[test]
    fn glyph_pick_deterministic_with_seed() {
        let mut r1 = rand_chacha::ChaCha8Rng::seed_from_u64(3);
        let mut r2 = rand_chacha::ChaCha8Rng::seed_from_u64(3);
        assert_eq!(
            GlyphSet::Katakana.pick(&mut r1),
            GlyphSet::Katakana.pick(&mut r2)
        );
    }
}
