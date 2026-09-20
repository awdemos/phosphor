use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ModeStat {
    pub dwell_s: f64,
    pub skips: u32,
    pub likes: u32,
    pub dislikes: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Prefs {
    pub modes: HashMap<String, ModeStat>,
    pub palettes: HashMap<String, f64>,
    /// Learned playback speed multiplier, 0.25..=4.0, default 1.0.
    pub speed: f64,
    pub samples: u64,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            modes: HashMap::new(),
            palettes: HashMap::new(),
            speed: 1.0,
            samples: 0,
        }
    }
}

/// Laplace smoothing α: keeps unobserved modes reachable.
const ALPHA: f64 = 0.5;
/// Vote factor per net vote, applied exponentially.
const VOTE_STEP: f64 = 0.35;

impl Prefs {
    pub fn load(path: &Path) -> Prefs {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Prefs::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Atomic write: temp file + rename, so a crash mid-save can't corrupt state.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn record_dwell(&mut self, mode: &str, palette: Option<&str>, seconds: f64) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }
        self.modes.entry(mode.to_string()).or_default().dwell_s += seconds;
        if let Some(p) = palette {
            *self.palettes.entry(p.to_string()).or_default() += seconds;
        }
        self.samples += 1;
    }

    pub fn record_skip(&mut self, mode: &str) {
        self.modes.entry(mode.to_string()).or_default().skips += 1;
    }

    pub fn record_vote(&mut self, mode: &str, positive: bool) {
        let stat = self.modes.entry(mode.to_string()).or_default();
        if positive {
            stat.likes += 1;
        } else {
            stat.dislikes += 1;
        }
    }

    pub fn adjust_speed(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.speed = (self.speed * factor).clamp(0.25, 4.0);
        }
    }

    /// Smoothed, normalized selection weights for `names`, in name order.
    pub fn weights(&self, names: &[&str]) -> Vec<f64> {
        let mut raw: Vec<f64> = names
            .iter()
            .map(|n| {
                let stat = self.modes.get(*n).cloned().unwrap_or_default();
                let votes = stat.likes as f64 - stat.dislikes as f64;
                let skip_drag = 1.0 / (1.0 + 0.1 * stat.skips as f64);
                (stat.dwell_s + ALPHA) * skip_drag * (VOTE_STEP * votes).exp()
            })
            .collect();
        let total: f64 = raw.iter().sum();
        if total <= 0.0 {
            raw.fill(1.0);
        }
        let total: f64 = raw.iter().sum();
        raw.into_iter().map(|w| w / total).collect()
    }
}

/// `PHOSPHOR_STATE_DIR` override, else XDG data dir (`~/.local/share/phosphor`).
pub fn state_dir() -> Option<PathBuf> {
    let env_override = std::env::var("PHOSPHOR_STATE_DIR");
    match env_override {
        Ok(d) if !d.is_empty() => Some(PathBuf::from(d)),
        _ => dirs::data_dir().map(|d| d.join("phosphor")),
    }
}

pub fn prefs_path() -> Option<PathBuf> {
    state_dir().map(|d| d.join("prefs.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn roundtrip_save_load() {
        let dir = tmp();
        let path = dir.path().join("prefs.json");
        let mut p = Prefs::default();
        p.record_dwell("orbital", Some("ember"), 12.5);
        p.record_skip("rain");
        p.record_vote("plasma", true);
        p.adjust_speed(1.1);
        p.save(&path).unwrap();
        let q = Prefs::load(&path);
        assert_eq!(q.modes["orbital"].dwell_s, 12.5);
        assert_eq!(q.modes["rain"].skips, 1);
        assert_eq!(q.modes["plasma"].likes, 1);
        assert!((q.speed - 1.1).abs() < 1e-9);
    }

    #[test]
    fn load_missing_or_corrupt_is_default() {
        let dir = tmp();
        let path = dir.path().join("nope.json");
        assert_eq!(Prefs::load(&path).samples, 0);
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Prefs::load(&path).samples, 0);
    }

    #[test]
    fn save_is_atomic_no_leftover_tmp() {
        let dir = tmp();
        let path = dir.path().join("p.json");
        Prefs::default().save(&path).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".tmp")
            })
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn weights_are_positive_and_sum_to_one() {
        let mut p = Prefs::default();
        p.record_dwell("orbital", None, 100.0);
        p.record_dwell("rain", None, 10.0);
        let names = ["orbital", "rain", "plasma", "pipes", "generative"];
        let w = p.weights(&names);
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(
            w.iter().all(|x| *x > 0.0),
            "smoothing keeps every mode reachable"
        );
        assert!(w[0] > w[1], "dwelled mode outranks the other");
    }

    #[test]
    fn likes_boost_and_dislikes_sink() {
        let mut p = Prefs::default();
        p.record_dwell("a", None, 50.0);
        p.record_dwell("b", None, 50.0);
        p.record_vote("b", false);
        p.record_vote("b", false);
        let names = ["a", "b"];
        let w = p.weights(&names);
        assert!(w[0] > w[1]);
    }

    #[test]
    fn speed_clamps() {
        let mut p = Prefs::default();
        for _ in 0..100 {
            p.adjust_speed(1.5);
        }
        assert_eq!(p.speed, 4.0);
        for _ in 0..100 {
            p.adjust_speed(0.5);
        }
        assert_eq!(p.speed, 0.25);
    }

    #[test]
    fn state_dir_env_override() {
        unsafe {
            std::env::set_var("PHOSPHOR_STATE_DIR", "/tmp/phosphor-test-state");
            assert_eq!(
                state_dir(),
                Some(std::path::PathBuf::from("/tmp/phosphor-test-state"))
            );
            std::env::remove_var("PHOSPHOR_STATE_DIR");
        }
    }
}
