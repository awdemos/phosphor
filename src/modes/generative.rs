use super::Mode;
use crate::engine::Canvas;
use crate::generative::SceneSpec;
use crate::llm::{self, LlmConfig};
use crate::modes::{orbital::Orbital, pipes::Pipes, plasma::Plasma, rain::Rain};
use crate::palette::Palette;
use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

const EPOCH_SECS: f64 = 40.0;

/// Wraps a concrete mode and periodically re-themes it from a SceneSpec,
/// sourced from an LLM when configured, else the offline synthesizer.
pub struct Generative {
    spec: SceneSpec,
    inner: Box<dyn Mode>,
    epoch: f64,
    prompt: String,
    llm: Option<LlmConfig>,
    offline: bool,
    synth_rng: ChaCha8Rng,
}

impl Generative {
    pub fn new(
        seed: u64,
        prompt: Option<String>,
        llm: Option<LlmConfig>,
        offline: bool,
    ) -> Self {
        let prompt = prompt.unwrap_or_else(|| "slow aurora over a data center".to_string());
        let spec = SceneSpec::from_prompt(&prompt);
        let inner = Self::instantiate(&spec, seed);
        Self {
            spec,
            inner,
            epoch: EPOCH_SECS,
            prompt,
            llm,
            offline,
            synth_rng: ChaCha8Rng::seed_from_u64(seed ^ 0x9e3779b97f4a7c15),
        }
    }

    pub fn spec(&self) -> &SceneSpec {
        &self.spec
    }

    fn instantiate(spec: &SceneSpec, seed: u64) -> Box<dyn Mode> {
        let mut m: Box<dyn Mode> = match spec.mode.as_str() {
            "orbital" => Box::new(Orbital::new(seed)),
            "rain" => Box::new(Rain::new(seed)),
            "pipes" => Box::new(Pipes::new(seed)),
            _ => Box::new(Plasma::new(seed)),
        };
        if let Some(p) = Palette::by_name(&spec.palette) {
            m.set_palette(p);
        }
        m
    }

    fn retheme(&mut self, _t: f64, seed: u64) {
        let variation = format!(
            "{} (variation {}, drift={})",
            self.prompt, self.spec.hue_drift, self.spec.speed
        );
        self.spec = if !self.offline {
            self.llm
                .as_ref()
                .and_then(|cfg| llm::fetch_spec(cfg, &variation))
                .unwrap_or_else(|| SceneSpec::from_prompt(&variation))
        } else {
            SceneSpec::from_prompt(&variation)
        };
        self.inner = Self::instantiate(&self.spec, seed);
    }
}

impl Mode for Generative {
    fn name(&self) -> &'static str {
        "generative"
    }

    fn update(&mut self, dt: f64, size: (u16, u16), rng: &mut ChaCha8Rng, speed: f64) {
        self.epoch -= dt;
        if self.epoch <= 0.0 {
            self.epoch = EPOCH_SECS;
            let seed = self.synth_rng.next_u64().wrapping_add((dt * 1000.0) as u64);
            self.retheme(0.0, seed);
        }
        let _ = rng;
        self.inner
            .update(dt, size, &mut self.synth_rng, speed * self.spec.speed.max(0.05));
    }

    fn render(&self, canvas: &mut Canvas, t: f64) {
        self.inner.render(canvas, t);
    }

    fn palette_name(&self) -> String {
        self.inner.palette_name()
    }

    fn set_palette(&mut self, palette: Palette) {
        self.inner.set_palette(palette);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Canvas;
    use rand::SeedableRng;

    #[test]
    fn generative_mode_rethemes_offline() {
        let mut rng = ChaCha8Rng::seed_from_u64(8);
        let mut g = Generative::new(42, Some("coral reef at night".into()), None, true);
        assert_eq!(g.name(), "generative");
        for _ in 0..10 {
            g.update(5.0, (60, 20), &mut rng, 1.0); // 50s > epoch: forces retheme
        }
        let mut c = Canvas::new(60, 20);
        g.render(&mut c, 3.0);
        assert!(!c.is_blank());
        assert_eq!(g.spec().palette, g.palette_name());
    }
}
