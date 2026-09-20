use crate::engine::Canvas;
use crate::lock::Secret;
use crate::modes::{self, generative::Generative, Mode};
use crate::prefs::{self, Prefs};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Option<String>,
    pub prompt: Option<String>,
    pub seed: u64,
    pub fps: f64,
    pub duration: Option<f64>,
    pub learn: bool,
    pub offline: bool,
    pub scripted: bool,
    pub locked: bool,
    pub password: Option<Secret>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: None,
            prompt: None,
            seed: 0x5eed,
            fps: 30.0,
            duration: None,
            learn: true,
            offline: false,
            scripted: false,
            locked: false,
            password: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Next,
    Prev,
    Like,
    Dislike,
    Faster,
    Slower,
    NextPalette,
    Pause,
    Char(char),
    Backspace,
    Submit,
    Quit,
}

pub struct App {
    config: Config,
    modes: Vec<Box<dyn Mode>>,
    current: usize,
    canvas: Canvas,
    fade_from: Option<(Canvas, f64)>,
    rng: ChaCha8Rng,
    prefs: Prefs,
    prefs_path: Option<PathBuf>,
    t: f64,
    since_rotate: f64,
    rotate_every: f64,
    dwell: f64,
    size: (u16, u16),
    quit: bool,
    locked: bool,
    entered: Secret,
    wrong_timer: f64,
    unlock_glow: f64,
    paused: bool,
    paused_t: f64,
}

const FADE_SECS: f64 = 1.2;
const MIN_W: u16 = 20;
const MIN_H: u16 = 8;

impl App {
    pub fn new(config: Config) -> Self {
        let rng = ChaCha8Rng::seed_from_u64(config.seed);
        let mode_list: Vec<Box<dyn Mode>> = match &config.mode {
            Some(name) => modes::build(name, config.seed).into_iter().collect(),
            None => {
                let mut v: Vec<Box<dyn Mode>> = ["orbital", "rain", "plasma", "pipes"]
                    .iter()
                    .filter_map(|n| modes::build(n, config.seed))
                    .collect();
                v.push(Box::new(Generative::new(
                    config.seed,
                    config.prompt.clone(),
                    crate::llm::LlmConfig::resolve(config.offline),
                    config.offline,
                )));
                v
            }
        };
        let prefs_path = prefs::prefs_path();
        let prefs = prefs_path
            .as_ref()
            .map(|p| Prefs::load(p))
            .unwrap_or_default();
        let locked = config.locked && config.password.is_some();
        let mut app = Self {
            rotate_every: if config.scripted { 3.3 } else { 45.0 },
            config,
            modes: mode_list,
            current: 0,
            canvas: Canvas::new(80, 24),
            fade_from: None,
            rng,
            prefs,
            prefs_path,
            t: 0.0,
            since_rotate: 0.0,
            dwell: 0.0,
            size: (80, 24),
            quit: false,
            locked,
            entered: Secret::new(""),
            wrong_timer: 0.0,
            unlock_glow: 0.0,
            paused: false,
            paused_t: 0.0,
        };
        if app.modes.is_empty() {
            app.modes
                .push(modes::build("plasma", app.config.seed).expect("plasma always builds"));
        }
        app
    }

    pub fn current_name(&self) -> &'static str {
        self.modes[self.current].name()
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.size = (w, h);
    }

    pub fn finished(&self) -> bool {
        self.quit
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    pub fn update(&mut self, dt: f64, inputs: &[Input]) {
        let dt = dt.clamp(0.0, 0.25);
        self.t += dt;

        if self.locked {
            self.update_locked(dt, inputs);
            return;
        }

        self.dwell += dt;
        self.since_rotate += dt;

        if self.paused {
            return;
        }

        for input in inputs {
            match input {
                Input::Quit => self.quit = true,
                Input::Next => self.rotate(true, 1),
                Input::Prev => self.rotate(true, -1),
                Input::Like => {
                    let name = self.current_name().to_string();
                    self.prefs.record_vote(&name, true);
                    // visual feedback: small glow to acknowledge the vote
                    self.unlock_glow = self.unlock_glow.max(0.5);
                }
                Input::Dislike => {
                    let name = self.current_name().to_string();
                    self.prefs.record_vote(&name, false);
                    self.rotate(true, 1);
                }
                Input::Faster => self.prefs.adjust_speed(1.25),
                Input::Slower => self.prefs.adjust_speed(0.8),
                Input::NextPalette => {
                    let p = crate::palette::Palette::random(&mut self.rng);
                    self.modes[self.current].set_palette(p);
                }
                Input::Pause => {
                    self.paused = !self.paused;
                    self.paused_t = self.t;
                }
                _ => {}
            }
        }

        if self.config.duration.is_some_and(|d| self.t >= d) {
            self.quit = true;
        }

        if self.modes.len() > 1 && self.since_rotate >= self.rotate_every {
            self.rotate(false, 1);
        }

        let speed = self.prefs.speed;
        self.modes[self.current].update(dt, self.size, &mut self.rng, speed);

        if let Some((_, ft)) = &mut self.fade_from {
            *ft += dt / FADE_SECS;
            if *ft >= 1.0 {
                self.fade_from = None;
            }
        }

        self.unlock_glow = (self.unlock_glow - dt).max(0.0);
    }

    fn update_locked(&mut self, dt: f64, inputs: &[Input]) {
        self.wrong_timer = (self.wrong_timer - dt).max(0.0);
        for input in inputs {
            match input {
                Input::Char(c) => self.entered.push(*c),
                Input::Backspace => self.entered.pop(),
                Input::Submit => {
                    let ok = self
                        .config
                        .password
                        .as_ref()
                        .is_some_and(|p| p.constant_time_eq(&self.entered));
                    if ok {
                        self.locked = false;
                        self.entered.clear();
                        self.unlock_glow = 1.0;
                        self.dwell = 0.0;
                    } else {
                        self.wrong_timer = 0.6;
                        self.entered.clear();
                    }
                }
                _ => {}
            }
        }
    }

    fn rotate(&mut self, manual: bool, _dir: i32) {
        if self.modes.len() < 2 {
            return;
        }
        let old = self.current;
        let old_name = self.modes[old].name().to_string();
        let old_palette = self.modes[old].palette_name();
        self.prefs
            .record_dwell(&old_name, Some(&old_palette), self.dwell);
        if manual {
            self.prefs.record_skip(&old_name);
        }
        self.persist_prefs();

        let next = if self.config.scripted {
            let n = self.modes.len();
            if n > 1 {
                let mut candidate = (self.current + 1) % n;
                // If the simple cycle wraps back to the same name (shouldn't
                // when all names are unique), keep moving until different.
                while self.modes[candidate].name() == self.modes[old].name() && candidate != old {
                    candidate = (candidate + 1) % n;
                }
                candidate
            } else {
                self.current
            }
        } else {
            self.weighted_pick()
        };
        if next == old {
            // Even if we can't switch, we still want to keep the timer from
            // piling up into a burst of rotations.
            self.since_rotate = 0.0;
            self.dwell = 0.0;
            return;
        }
        let mut snap = Canvas::new(self.size.0.max(1), self.size.1.max(1));
        self.modes[old].render(&mut snap, self.t);
        self.fade_from = Some((snap, 0.0));
        self.current = next;
        self.since_rotate = 0.0;
        self.dwell = 0.0;
        self.rotate_every = if self.config.scripted {
            3.3
        } else {
            40.0 + self.rng.random::<f64>() * 15.0
        };
    }

    fn weighted_pick(&mut self) -> usize {
        let names: Vec<&'static str> = self.modes.iter().map(|m| m.name()).collect();
        let weights = self.prefs.weights(&names);
        let current = self.current;
        // Exclude the current mode: zero its weight, rescale the rest to 1.
        let mut adjusted = weights.clone();
        if !adjusted.is_empty() {
            adjusted[current] = 0.0;
            let total: f64 = adjusted.iter().sum();
            let denom = if total > 0.0 { total } else { 1.0 };
            for w in &mut adjusted {
                *w /= denom;
            }
        }
        let roll: f64 = self.rng.random::<f64>();
        let mut acc = 0.0;
        for (i, w) in adjusted.iter().enumerate() {
            acc += w;
            if roll <= acc {
                return i;
            }
        }
        // Fallback: next slot wrapping, avoiding current if possible.
        let mut nxt = (current + 1) % self.modes.len();
        if nxt == current && self.modes.len() > 1 {
            nxt = (nxt + 1) % self.modes.len();
        }
        nxt
    }

    pub fn compose(&mut self) -> &Canvas {
        self.canvas.resize(self.size.0.max(1), self.size.1.max(1));
        self.canvas.clear();
        let (w, h) = self.size;
        if w < MIN_W || h < MIN_H {
            let fg = crate::palette::Rgb::new(200, 200, 210);
            self.canvas.text(1, 1, "phosphor", fg);
            self.canvas.text(1, 2, "terminal too small", fg.scale(0.6));
            self.canvas.text(
                1,
                3,
                &format!("need {MIN_W}x{MIN_H}, got {w}x{h}"),
                fg.scale(0.6),
            );
            return &self.canvas;
        }
        let t = self.t;
        if !self.paused {
            self.modes[self.current].render(&mut self.canvas, t);
        } else {
            self.modes[self.current].render(&mut self.canvas, self.paused_t);
        }
        if let Some((from, ft)) = &self.fade_from {
            let blended = from.blend(&self.canvas, *ft);
            self.canvas = blended;
        }

        self.render_help_bar();

        if self.locked {
            self.render_lock_overlay();
        } else if self.unlock_glow > 0.0 {
            self.render_unlock_flash();
        }

        &self.canvas
    }

    fn render_help_bar(&mut self) {
        let (w, h) = (self.canvas.width as i32, self.canvas.height as i32);
        if h < 5 || w < 30 {
            return;
        }
        let bg = crate::palette::Rgb::new(20, 22, 28);
        // Dim the bottom two rows behind the help bar so text stays readable.
        let y0 = h - 2;
        for y in y0..h {
            for x in 0..w {
                if let Some(cell) = self.canvas.get(x, y) {
                    if cell.ch == ' ' || cell.ch == '\0' {
                        self.canvas.put(x, y, ' ', bg);
                    } else {
                        self.canvas.put(x, y, cell.ch, cell.fg.scale(0.45));
                    }
                }
            }
        }

        let fg = crate::palette::Rgb::new(180, 190, 210);
        let hi = crate::palette::Rgb::new(100, 220, 255);
        let muted = fg.scale(0.6);
        let speed_label = format!("{:.1}x", self.prefs.speed);

        // Line 1: controls
        let line = "[←→n] mode [p] palette [l] like [d] dislike [-] slower [+] faster [space] pause [q] quit";
        let text_w = line.chars().count() as i32;
        let x = (w - text_w) / 2;
        let mut cx = x;
        for token in line.split_inclusive(']') {
            // Each token is either "[X] label " or trailing plain text.
            if let Some(close) = token.find(']') {
                let key = &token[1..close];
                let rest = &token[close + 1..];
                self.canvas.text(cx, y0, "[", muted);
                cx += 1;
                self.canvas.text(cx, y0, key, hi);
                cx += key.chars().count() as i32;
                self.canvas.text(cx, y0, "]", muted);
                cx += 1;
                if !rest.is_empty() {
                    self.canvas.text(cx, y0, rest, fg);
                    cx += rest.chars().count() as i32;
                }
            } else {
                self.canvas.text(cx, y0, token, fg);
                cx += token.chars().count() as i32;
            }
        }

        // Line 2: status
        let paused = self.paused;
        let mode = self.current_name();
        let status = format!(
            "mode: {}  palette: {}  speed: {}  {}",
            mode,
            self.modes[self.current].current_palette().name,
            speed_label,
            if paused { "PAUSED" } else { "running" }
        );
        let sw = status.chars().count() as i32;
        self.canvas.text((w - sw) / 2, h - 1, &status, muted);
    }

    fn render_lock_overlay(&mut self) {
        // Dim the frozen background.
        self.canvas.scale_colors(0.25);
        let (w, h) = (self.canvas.width as i32, self.canvas.height as i32);
        let box_w = 44i32;
        let box_h = 9i32;
        let bx = (w - box_w) / 2;
        let by = (h - box_h) / 2;
        let fg = crate::palette::Rgb::new(220, 220, 230);
        let error = crate::palette::Rgb::new(255, 80, 80);
        let border = if self.wrong_timer > 0.0 { error } else { fg };
        // Simple double-line box.
        for x in bx..bx + box_w {
            self.canvas.put(x, by, '═', border);
            self.canvas.put(x, by + box_h - 1, '═', border);
        }
        for y in by..by + box_h {
            self.canvas.put(bx, y, '║', border);
            self.canvas.put(bx + box_w - 1, y, '║', border);
        }
        self.canvas.put(bx, by, '╔', border);
        self.canvas.put(bx + box_w - 1, by, '╗', border);
        self.canvas.put(bx, by + box_h - 1, '╚', border);
        self.canvas.put(bx + box_w - 1, by + box_h - 1, '╝', border);

        self.canvas.text(bx + 2, by + 2, "SCREENSAVER LOCKED", fg);
        let dots: String = (0..self.entered.len()).map(|_| '●').collect();
        let prompt = format!("Password: {dots}");
        self.canvas.text(bx + 2, by + 4, &prompt, fg);
        self.canvas.text(
            bx + 2,
            by + 6,
            if self.wrong_timer > 0.0 {
                "wrong password · try again"
            } else {
                "Enter to submit · Backspace · any key exits"
            },
            if self.wrong_timer > 0.0 {
                error
            } else {
                fg.scale(0.5)
            },
        );
    }

    fn render_unlock_flash(&mut self) {
        let (w, h) = (self.canvas.width as i32, self.canvas.height as i32);
        let fg = crate::palette::Rgb::new(100, 255, 150).scale(self.unlock_glow);
        let text = "UNLOCKED";
        let x = (w - text.len() as i32) / 2;
        let y = h / 2;
        self.canvas.text(x, y, text, fg);
    }

    fn persist_prefs(&self) {
        if !self.config.learn {
            return;
        }
        if let Some(path) = &self.prefs_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = self.prefs.save(path);
        }
    }

    pub fn save_prefs(&mut self) {
        if self.locked {
            return;
        }
        let name = self.modes[self.current].name().to_string();
        let palette = self.modes[self.current].palette_name();
        self.prefs.record_dwell(&name, Some(&palette), self.dwell);
        self.dwell = 0.0;
        self.persist_prefs();
    }

    pub fn stats_report(&self) -> String {
        let names: Vec<&'static str> = self.modes.iter().map(|m| m.name()).collect();
        let weights = self.prefs.weights(&names);
        let mut lines = vec![
            "phosphor — learned preferences".to_string(),
            format!(
                "samples: {}   speed: {:.2}x",
                self.prefs.samples, self.prefs.speed
            ),
            String::new(),
            "mode          dwell      skips likes dislikes weight".to_string(),
        ];
        for (i, n) in names.iter().enumerate() {
            let s = self.prefs.modes.get(*n).cloned().unwrap_or_default();
            lines.push(format!(
                "{:<13} {:>8.1}s {:>6} {:>5} {:>8} {:>6.1}%",
                n,
                s.dwell_s,
                s.skips,
                s.likes,
                s.dislikes,
                weights.get(i).copied().unwrap_or(0.0) * 100.0
            ));
        }
        lines.push(String::new());
        lines.push("palette affinity".to_string());
        let mut pal: Vec<(&String, &f64)> = self.prefs.palettes.iter().collect();
        pal.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (name, dwell) in pal.iter().take(8) {
            lines.push(format!("{name:<13} {dwell:.1}s"));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(scripted: bool) -> App {
        App::new(Config {
            scripted,
            seed: 42,
            learn: false,
            ..Config::default()
        })
    }

    #[test]
    fn rotates_through_all_modes_when_scripted() {
        let mut app = test_app(true);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..600 {
            app.update(0.1, &[]);
            seen.insert(app.current_name().to_string());
        }
        for want in ["orbital", "rain", "plasma", "pipes", "generative"] {
            assert!(seen.contains(want), "scripted run never showed {want}");
        }
    }

    #[test]
    fn scripted_cycles_without_sticking_on_same_mode() {
        let mut app = App::new(Config {
            scripted: true,
            seed: 7,
            learn: false,
            ..Config::default()
        });
        // With 5 modes and a 3.3 s rotation, 10 s guarantees ~3 rotations.
        for _ in 0..120 {
            app.update(0.1, &[]);
        }
        // Collect modes over another full cycle window.
        let mut seen = std::collections::HashSet::new();
        let mut last = String::new();
        for _ in 0..120 {
            app.update(0.1, &[]);
            let now = app.current_name().to_string();
            if now != last {
                seen.insert(now.clone());
                last = now;
            }
        }
        for want in ["orbital", "rain", "plasma", "pipes"] {
            assert!(
                seen.contains(want),
                "scripted mode cycle should show {want}"
            );
        }
    }

    #[test]
    fn duration_triggers_exit() {
        let mut app = App::new(Config {
            duration: Some(1.0),
            learn: false,
            ..Config::default()
        });
        assert!(!app.finished());
        for _ in 0..20 {
            app.update(0.1, &[]);
        }
        assert!(app.finished());
    }

    #[test]
    fn quit_input_exits() {
        let mut app = test_app(false);
        app.update(0.016, &[Input::Quit]);
        assert!(app.finished());
    }

    #[test]
    fn compose_is_never_blank_and_follows_size() {
        let mut app = test_app(true);
        for (w, h) in [(80u16, 24u16), (50, 15), (100, 30)] {
            app.resize(w, h);
            for _ in 0..10 {
                app.update(1.0 / 30.0, &[]);
                let c = app.compose();
                assert_eq!((c.width, c.height), (w, h));
                assert!(!c.is_blank(), "blank frame at {w}x{h}");
            }
        }
    }

    #[test]
    fn likes_and_dwell_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("PHOSPHOR_STATE_DIR", dir.path());
        }
        let mut app = App::new(Config {
            mode: Some("rain".into()),
            seed: 1,
            learn: true,
            ..Config::default()
        });
        for _ in 0..20 {
            app.update(0.1, &[]);
        }
        app.update(0.1, &[Input::Like]);
        app.save_prefs();
        let p = crate::prefs::Prefs::load(&crate::prefs::prefs_path().unwrap());
        assert!(p.samples > 0, "dwell should have been recorded");
        assert!(
            p.modes.get("rain").map_or(false, |m| m.likes == 1),
            "like should have landed on rain"
        );
        unsafe {
            std::env::remove_var("PHOSPHOR_STATE_DIR");
        }
    }

    #[test]
    fn single_mode_config_sticks() {
        let mut app = App::new(Config {
            mode: Some("rain".into()),
            seed: 3,
            learn: false,
            ..Config::default()
        });
        for _ in 0..200 {
            app.update(0.1, &[]);
            assert_eq!(app.current_name(), "rain");
        }
    }

    #[test]
    fn lock_ignores_quit_and_unlocks_with_password() {
        let mut app = App::new(Config {
            locked: true,
            password: Some(Secret::new("secret")),
            seed: 5,
            learn: false,
            ..Config::default()
        });
        app.resize(80, 24);
        assert!(app.locked());
        app.update(0.1, &[Input::Quit]);
        assert!(app.locked(), "quit must not unlock");
        assert!(!app.finished());
        for c in "wrong".chars() {
            app.update(0.1, &[Input::Char(c)]);
        }
        app.update(0.1, &[Input::Submit]);
        assert!(app.locked(), "wrong password should stay locked");
        for _ in 0..10 {
            app.update(0.1, &[Input::Backspace]);
        }
        for c in "secret".chars() {
            app.update(0.1, &[Input::Char(c)]);
        }
        app.update(0.1, &[Input::Submit]);
        assert!(!app.locked(), "correct password should unlock");
        assert!(app.compose().is_blank() == false);
    }
}
