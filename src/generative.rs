use crate::glyph::GlyphSet;
use crate::palette::Palette;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// A generative scene: enough structure for the procedural renderers to be
/// re-parameterized by an LLM (or by the offline synthesizer).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SceneSpec {
    pub palette: String,
    pub mode: String,
    pub density: f64,
    pub speed: f64,
    pub glyphs: String,
    pub hue_drift: f64,
}

impl SceneSpec {
    /// Clamp everything into valid ranges and fix unknown names with seeded
    /// fallbacks so a hallucinated spec can never crash the renderer.
    pub fn sanitize(mut self) -> SceneSpec {
        if Palette::by_name(&self.palette).is_none() {
            self.palette = SceneSpec::from_prompt(&self.palette).palette;
        }
        if !["orbital", "rain", "plasma", "pipes"].contains(&self.mode.as_str()) {
            self.mode = "plasma".to_string();
        }
        self.density = self.density.clamp(0.05, 1.0);
        self.speed = self.speed.clamp(0.05, 1.0);
        self.hue_drift = self.hue_drift.clamp(0.0, 1.0);
        let _ = GlyphSet::from_name(&self.glyphs);
        self
    }

    /// Offline synthesizer: derive a valid scene from prompt text alone via
    /// seeded hashing. Same prompt → same scene.
    pub fn from_prompt(prompt: &str) -> SceneSpec {
        use rand::{Rng, SeedableRng};
        use std::collections::hash_map::DefaultHasher;
        let mut hash = DefaultHasher::new();
        prompt.hash(&mut hash);
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(hash.finish());
        let palettes = Palette::all();
        let mode = ["orbital", "rain", "plasma", "pipes"][rng.random_range(0..4)];
        let glyphs = ["katakana", "braille", "ascii", "blocks"][rng.random_range(0..4)];
        SceneSpec {
            palette: palettes[rng.random_range(0..palettes.len())]
                .name
                .to_string(),
            mode: mode.to_string(),
            density: 0.2 + rng.random::<f64>() * 0.8,
            speed: 0.2 + rng.random::<f64>() * 0.8,
            glyphs: glyphs.to_string(),
            hue_drift: rng.random::<f64>(),
        }
        .sanitize()
    }
}

/// Extract the first JSON object from free text (LLMs love fences and prose),
/// parse it leniently, and backfill anything missing.
pub fn extract_spec(text: &str) -> Option<SceneSpec> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut end = None;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(start + i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let value: serde_json::Value = serde_json::from_str(&text[start..end]).ok()?;
    let spec = SceneSpec {
        palette: value
            .get("palette")
            .and_then(|v| v.as_str())
            .unwrap_or("polaris")
            .to_string(),
        mode: value
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("plasma")
            .to_string(),
        density: value.get("density").and_then(|v| v.as_f64()).unwrap_or(0.5),
        speed: value.get("speed").and_then(|v| v.as_f64()).unwrap_or(0.5),
        glyphs: value
            .get("glyphs")
            .and_then(|v| v.as_str())
            .unwrap_or("mix")
            .to_string(),
        hue_drift: value
            .get("hue_drift")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.3),
    };
    Some(spec.sanitize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_ranges_and_fixes_names() {
        let s = SceneSpec {
            palette: "unknown-palette".into(),
            mode: "bogus".into(),
            density: 7.0,
            speed: -1.0,
            glyphs: "weird".into(),
            hue_drift: 42.0,
        }
        .sanitize();
        assert!(Palette::by_name(&s.palette).is_some());
        assert!(["orbital", "rain", "plasma", "pipes"].contains(&s.mode.as_str()));
        assert!((0.0..=1.0).contains(&s.density));
        assert!((0.0..=1.0).contains(&s.speed));
        assert!((0.0..=1.0).contains(&s.hue_drift));
    }

    #[test]
    fn from_prompt_is_deterministic_and_valid() {
        let a = SceneSpec::from_prompt("neon tidepool");
        let b = SceneSpec::from_prompt("neon tidepool");
        assert_eq!(a, b);
        assert!(!a.palette.is_empty());
    }

    #[test]
    fn extract_spec_pulls_json_from_fenced_llm_output() {
        let text = "Here you go:\n```json\n{\"palette\":\"ember\",\"mode\":\"rain\",\"density\":0.7,\"speed\":0.5,\"glyphs\":\"katakana\",\"hue_drift\":0.2}\n```\nEnjoy.";
        let s = extract_spec(text).unwrap();
        assert_eq!(s.palette, "ember");
        assert_eq!(s.mode, "rain");
        assert!((s.density - 0.7).abs() < 1e-9);
    }

    #[test]
    fn extract_spec_rejects_garbage() {
        assert!(extract_spec("no json here").is_none());
        assert!(
            extract_spec("{\"mode\": \"rain\"}").is_some(),
            "missing fields are backfilled by sanitize"
        );
    }
}
