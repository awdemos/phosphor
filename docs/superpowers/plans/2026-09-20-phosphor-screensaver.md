# phosphor screensaver implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `phosphor`, a truecolor multi-mode terminal screensaver in Rust (ratatui) with an LLM-driven generative mode and on-device preference learning, then ship it as `github.com/awdemos/phosphor` with a 10s asciinema demo.

**Architecture:** Single binary crate. Every mode renders into a backend-agnostic `Canvas` (testable without a terminal); `App` owns scheduling, crossfade, input, and dwell tracking; `main.rs` wires the crossterm terminal lifecycle. Spec: `docs/superpowers/specs/2026-09-20-phosphor-screensaver-design.md`.

**Tech Stack:** Rust 2024 edition, ratatui 0.29, crossterm 0.28, clap 4 (derive), serde/serde_json 1, rand 0.9 + rand_chacha 0.9, dirs 6, ureq 2, tempfile 3 (dev).

**Repo layout:**

```
phosphor/
├── Cargo.toml
├── LICENSE                  (MIT, copyright awdemos)
├── README.md
├── demo.cast / demo.gif / demo.svg   (Task 18)
├── docs/superpowers/...     (spec, this plan)
├── src/
│   ├── main.rs              CLI + terminal lifecycle
│   ├── app.rs               event core: scheduler, crossfade, input, dwell
│   ├── engine.rs            Canvas cell buffer + crossfade blending
│   ├── palette.rs           Rgb, Palette gradients
│   ├── glyph.rs             ramps + glyph sets
│   ├── clock.rs             3x5 block-digit clock
│   ├── prefs.rs             learning store (JSON, atomic writes)
│   ├── generative.rs        SceneSpec, offline synthesizer, JSON extract
│   ├── llm.rs               OpenAI-compatible client + autodetect
│   └── modes/
│       ├── mod.rs           Mode trait + registry
│       ├── orbital.rs       starfield + rings + nebula + clock + ticker
│       ├── rain.rs          glyph rain
│       ├── plasma.rs        sine-field plasma
│       ├── pipes.rs         pipe walkers over plasma floor
│       └── generative.rs    SceneSpec-driven wrapper mode
└── tests/headless.rs        end-to-end headless tests
```

**Shared APIs (defined in Tasks 2–6, used everywhere — signatures are normative):**

- `Canvas::new(w,h) / clear / resize / put(x,y,ch,fg) / text(x,y,&str,fg) / get(x,y)->Option<Cell> / blend(&other,t)->Canvas / paint(&mut ratatui::Frame) / is_blank()`
- `Rgb::new(r,g,b) / lerp(other,t) / scale(f) / hue_shift(deg) / to_color()`
- `Palette { name: &'static str, stops: Vec<Rgb> }` with `sample(t) / all() / by_name(&str) / random(&mut ChaCha8Rng)`
- `ramp_char(level: f64) -> char`, `GlyphSet::{Katakana,Braille,Ascii,Blocks,Mix}` with `from_name / random`
- `draw_clock(&mut Canvas, cx, cy, t_secs, fg, show_seconds)`
- `Prefs::{load,save,record_dwell,record_skip,record_vote,adjust_speed,weights}`

**Conventions:** edition 2024; `use rand::{Rng, SeedableRng}; use rand::seq::IndexedRandom;` for all RNG (`rng.random::<f64>()`, `slice.choose(&mut rng)`); no `unwrap()` outside tests; clippy pedantic-clean (`cargo clippy -- -D warnings`); commits carry the trailer `Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>`.

---

### Task 1: Manifest + dependencies

**Files:**
- Modify: `Cargo.toml`
- Create: `.gitignore`

- [ ] **Step 1: Replace `Cargo.toml`**

```toml
[package]
name = "phosphor"
version = "0.1.0"
edition = "2024"
description = "A truecolor, self-learning terminal screensaver"
license = "MIT"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rand = "0.9"
rand_chacha = "0.9"
dirs = "6"
ureq = { version = "2", features = ["json"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: `.gitignore`**

```
/target
demo.cast
```

- [ ] **Step 3: Fetch + build**

Run: `cargo build`
Expected: compiles (only `fn main() {}` so far); deps resolve from local registry cache.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore
git commit -m "chore: manifest and dependencies

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 2: `engine.rs` — Canvas

**Files:**
- Create: `src/engine.rs`
- Modify: `src/main.rs` (add `mod engine;`)

- [ ] **Step 1: Failing tests** — append to `src/engine.rs`:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: crate::palette::Rgb,
    pub bg: Option<crate::palette::Rgb>,
}

pub struct Canvas {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_get_roundtrip() {
        let mut c = Canvas::new(10, 4);
        let fg = crate::palette::Rgb::new(255, 0, 0);
        c.put(3, 2, '#', fg);
        assert_eq!(c.get(3, 2).map(|c| c.ch), Some('#'));
        assert_eq!(c.get(3, 2).map(|c| c.fg), Some(fg));
        assert_eq!(c.get(99, 99), None, "out of bounds reads return None");
    }

    #[test]
    fn text_writes_run() {
        let mut c = Canvas::new(20, 2);
        c.text(5, 1, "hi", crate::palette::Rgb::new(0, 255, 0));
        assert_eq!(c.get(5, 1).map(|c| c.ch), Some('h'));
        assert_eq!(c.get(6, 1).map(|c| c.ch), Some('i'));
        assert_eq!(c.get(7, 1).map(|c| c.ch), Some(' '));
    }

    #[test]
    fn blank_canvas_is_blank() {
        assert!(Canvas::new(8, 3).is_blank());
        let mut c = Canvas::new(8, 3);
        c.put(0, 0, 'x', crate::palette::Rgb::new(1, 2, 3));
        assert!(!c.is_blank());
    }

    #[test]
    fn blend_crossfades_cells() {
        let red = crate::palette::Rgb::new(255, 0, 0);
        let blue = crate::palette::Rgb::new(0, 0, 255);
        let mut a = Canvas::new(2, 1);
        a.put(0, 0, '@', red);
        let mut b = Canvas::new(2, 1);
        b.put(0, 0, ' ', blue);
        let half = a.blend(&b, 0.5);
        let cell = half.get(0, 0).unwrap();
        assert_eq!(cell.fg, crate::palette::Rgb::new(128, 0, 128));
        let full = a.blend(&b, 1.0);
        assert_eq!(full.get(0, 0).map(|c| c.ch), Some(' '));
    }

    #[test]
    fn resize_keeps_size_consistent() {
        let mut c = Canvas::new(4, 4);
        c.resize(6, 2);
        assert_eq!((c.width, c.height), (6, 2));
        assert!(c.is_blank());
    }
}
```

- [ ] **Step 2: Run, verify failure**

Run: `cargo test engine`
Expected: FAIL — `palette` module missing / Canvas methods undefined.

- [ ] **Step 3: Implement** — same file, above the tests:

```rust
impl Canvas {
    pub fn new(width: u16, height: u16) -> Self {
        Self { width, height, cells: vec![Cell::default(); width as usize * height as usize] }
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.cells.clear();
        self.cells.resize(width as usize * height as usize, Cell::default());
    }

    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        Some(y as usize * self.width as usize + x as usize)
    }

    pub fn put(&mut self, x: i32, y: i32, ch: char, fg: crate::palette::Rgb) {
        if let Some(i) = self.idx(x, y) {
            self.cells[i] = Cell { ch, fg, bg: None };
        }
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, fg: crate::palette::Rgb) {
        for (i, ch) in s.chars().enumerate() {
            self.put(x + i as i32, y, ch, fg);
        }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Cell> {
        self.idx(x, y).map(|i| self.cells[i])
    }

    pub fn is_blank(&self) -> bool {
        self.cells.iter().all(|c| c.ch == ' ' || c.ch == '\0')
    }

    /// Crossfade with `other` (t=0 → self, t=1 → other) by blending
    /// per-cell luminance through the glyph ramp.
    pub fn blend(&self, other: &Canvas, t: f64) -> Canvas {
        let t = t.clamp(0.0, 1.0);
        let (w, h) = (self.width.min(other.width), self.height.min(other.height));
        let mut out = Canvas::new(self.width, self.height);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let a = self.get(x, y).unwrap_or_default();
                let b = other.get(x, y).unwrap_or_default();
                let la = crate::glyph::ramp_level(a.ch);
                let lb = crate::glyph::ramp_level(b.ch);
                let level = la + (lb - la) * t;
                let fg = a.fg.lerp(b.fg, t);
                let ch = if level <= 0.0 && a.ch == ' ' && b.ch == ' ' { ' ' } else { crate::glyph::ramp_char(level) };
                out.put(x, y, ch, fg);
            }
        }
        out
    }

    pub fn paint(&self, frame: &mut ratatui::Frame) {
        let buf = frame.buffer_mut();
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                let Some(cell) = self.get(x, y) else { continue };
                if cell.ch == ' ' || cell.ch == '\0' {
                    continue;
                }
                if let Some(c) = buf.cell_mut((x as u16, y as u16)) {
                    c.set_char(cell.ch).set_fg(cell.fg.to_color());
                    if let Some(bg) = cell.bg {
                        c.set_bg(bg.to_color());
                    }
                }
            }
        }
    }
}
```

Note: depends on `palette::Rgb` (Task 3) and `glyph::{ramp_char, ramp_level}` (Task 4). Implement Tasks 3 and 4 next; until then tests stay red — that is expected. Run the full suite green at the end of Task 4.

- [ ] **Step 4: Commit (after Task 4 green gate)**

```bash
git add src/engine.rs src/palette.rs src/glyph.rs src/main.rs
git commit -m "feat: canvas cell buffer, palettes, glyphs

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 3: `palette.rs` — Rgb + Palette

**Files:**
- Create: `src/palette.rs`
- Modify: `src/main.rs` (add `mod palette;`)

- [ ] **Step 1: Failing tests** — append:

```rust
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
        let p = Palette { name: "t", stops: vec![Rgb::new(0, 0, 0), Rgb::new(255, 255, 255)] };
        assert_eq!(p.sample(0.0), Rgb::new(0, 0, 0));
        assert_eq!(p.sample(0.5), Rgb::new(128, 128, 128));
        assert_eq!(p.sample(1.0), Rgb::new(255, 255, 255));
        assert_eq!(p.sample(-0.25), p.sample(0.75));
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
        let r = Rgb::new(255, 0, 0);
        let shifted = r.hue_shift(120.0);
        assert!(shifted.g > 200 && shifted.r < 50);
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test palette` → FAIL (types undefined).

- [ ] **Step 3: Implement** — above tests:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
        let t = t.rem_euclid(1.0);
        let pos = t * n as f64;
        let i = pos as usize % n;
        let j = (i + 1) % n;
        self.stops[i].lerp(self.stops[j], pos - pos.floor())
    }

    pub fn all() -> Vec<Palette> {
        vec![
            Palette { name: "ember", stops: vec![Rgb::new(255, 60, 0), Rgb::new(255, 180, 40), Rgb::new(120, 10, 60), Rgb::new(20, 4, 30)] },
            Palette { name: "lagoon", stops: vec![Rgb::new(0, 220, 190), Rgb::new(0, 120, 255), Rgb::new(60, 0, 120), Rgb::new(0, 20, 40)] },
            Palette { name: "orchard", stops: vec![Rgb::new(250, 90, 160), Rgb::new(140, 60, 220), Rgb::new(40, 200, 180), Rgb::new(250, 220, 90)] },
            Palette { name: "polaris", stops: vec![Rgb::new(200, 220, 255), Rgb::new(90, 140, 255), Rgb::new(20, 40, 120), Rgb::new(4, 8, 30)] },
            Palette { name: "rainforest", stops: vec![Rgb::new(40, 220, 100), Rgb::new(0, 140, 90), Rgb::new(200, 230, 60), Rgb::new(10, 40, 20)] },
            Palette { name: "monolith", stops: vec![Rgb::new(240, 240, 245), Rgb::new(150, 150, 160), Rgb::new(60, 60, 70), Rgb::new(12, 12, 16)] },
        ]
    }

    pub fn by_name(name: &str) -> Option<Palette> {
        Palette::all().into_iter().find(|p| p.name == name)
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
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
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
    let f = |n: f64| {
        let k = (n + h * 12.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * k.max(-1.0).min(1.0).max(k - 3.0f64.min(9.0 - k).min(1.0))
    };
    Rgb::new((f(0.0) * 255.0) as u8, (f(8.0) * 255.0) as u8, (f(4.0) * 255.0) as u8)
}
```

- [ ] **Step 4: Run** — `cargo test palette` → still fails to link until Task 4 (`glyph` missing); proceed to Task 4, then run `cargo test` (all green).

---

### Task 4: `glyph.rs` — ramps + glyph sets

**Files:**
- Create: `src/glyph.rs`
- Modify: `src/main.rs` (add `mod glyph;`)

- [ ] **Step 1: Failing tests** — append:

```rust
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
        assert_eq!(GlyphSet::Katakana.pick(&mut r1), GlyphSet::Katakana.pick(&mut r2));
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test glyph` → FAIL.

- [ ] **Step 3: Implement** — above tests:

```rust
use rand::Rng;
use rand::seq::IndexedRandom;
use rand_chacha::ChaCha8Rng;

/// Luminance ramp, dimmest to brightest. Used for density-mapped rendering
/// and as the shared alphabet for crossfade blending.
pub const RAMP: &[char] = &[' ', '·', ':', ';', 't', '+', 'n', 'N', 'M', '@'];

pub const KATAKANA: &[char] = &['ｱ', 'ｶ', 'ｻ', 'ﾀ', 'ﾅ', 'ﾊ', 'ﾏ', 'ﾔ', 'ﾗ', 'ﾜ', 'ｦ', 'ﾝ', 'ｼ', 'ｷ', 'ｸ'];
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
    RAMP.iter().position(|&c| c == ch).map_or(0.0, |i| i as f64 / (RAMP.len() - 1) as f64)
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
            GlyphSet::Mix => [KATAKANA, BRAILLE, ASCII_SET, BLOCKS].choose(rng).copied().unwrap_or(ASCII_SET),
        };
        set.choose(rng).copied().unwrap_or('·')
    }
}
```

- [ ] **Step 4: Run full suite green + commit**

Run: `cargo test`
Expected: PASS (engine, palette, glyph all green).

```bash
git add src/engine.rs src/palette.rs src/glyph.rs src/main.rs
git commit -m "feat: canvas cell buffer, palettes, glyphs

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 5: `clock.rs` — block-digit clock

**Files:**
- Create: `src/clock.rs`
- Modify: `src/main.rs` (add `mod clock;`)

- [ ] **Step 1: Failing test** — append:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_hh_mm_with_colon() {
        let mut c = crate::engine::Canvas::new(40, 7);
        draw_clock(&mut c, 1, 1, 9.0 * 3600.0 + 5.0 * 60.0, crate::palette::Rgb::new(255, 255, 255), false);
        // "09:05" at 4 chars wide + colon = 4*4+2 = 18 cols; must be non-blank in that band
        let mut lit = 0;
        for y in 0..7 {
            for x in 0..40 {
                if c.get(x, y).map(|c| c.ch).unwrap_or(' ') != ' ' {
                    lit += 1;
                }
            }
        }
        assert!(lit > 40, "clock should light up a bunch of cells, got {lit}");
    }

    #[test]
    fn every_digit_glyph_is_three_wide_five_tall() {
        for g in GLYPHS {
            assert_eq!(g.len(), 5);
            for row in g {
                assert_eq!(row.chars().count(), 3, "row {row:?} not 3 wide");
            }
        }
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test clock` → FAIL.

- [ ] **Step 3: Implement** — above tests:

```rust
use crate::engine::Canvas;
use crate::palette::Rgb;

/// 3x5 pixel font: index 0..=9 digits, 10 = ':'.
pub const GLYPHS: [[&str; 5]; 11] = [
    ["###", "# #", "# #", "# #", "###"], // 0
    ["  #", "  #", "  #", "  #", "  #"], // 1
    ["###", "  #", "###", "#  ", "###"], // 2
    ["###", "  #", "###", "  #", "###"], // 3
    ["# #", "# #", "###", "  #", "  #"], // 4
    ["###", "#  ", "###", "  #", "###"], // 5
    ["###", "#  ", "###", "# #", "###"], // 6
    ["###", "  #", "  #", "  #", "  #"], // 7
    ["###", "# #", "###", "# #", "###"], // 8
    ["###", "# #", "###", "  #", "###"], // 9
    ["   ", " # ", "   ", " # ", "   "], // :
];

/// Draw HH:MM (or HH:MM:SS) with top-left at (cx, cy), doubled horizontally
/// for terminal aspect ratio.
pub fn draw_clock(canvas: &mut Canvas, cx: i32, cy: i32, t_secs: f64, fg: Rgb, show_seconds: bool) {
    let h = (t_secs / 3600.0) as u32 % 24;
    let m = (t_secs / 60.0) as u32 % 60;
    let s = t_secs as u32 % 60;
    let digits: Vec<usize> = if show_seconds {
        vec![(h / 10) as usize, (h % 10) as usize, 10, (m / 10) as usize, (m % 10) as usize, 10, (s / 10) as usize, (s % 10) as usize]
    } else {
        vec![(h / 10) as usize, (h % 10) as usize, 10, (m / 10) as usize, (m % 10) as usize]
    };
    let mut x = cx;
    for d in digits {
        let g = &GLYPHS[d];
        for (row, line) in g.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                if ch == '#' {
                    canvas.put(x + col as i32, cy + row as i32, '█', fg);
                    canvas.put(x + col as i32 + 1, cy + row as i32, '█', fg);
                }
            }
        }
        x += 8;
    }
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test clock` → PASS.

```bash
git add src/clock.rs src/main.rs
git commit -m "feat: block digit clock

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 6: `prefs.rs` — learning store

**Files:**
- Create: `src/prefs.rs`
- Modify: `src/main.rs` (add `mod prefs;`)

- [ ] **Step 1: Failing tests** — append:

```rust
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
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_is_atomic_no_leftover_tmp() {
        let dir = tmp();
        let path = dir.path().join("p.json");
        Prefs::default().save(&path).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains(".tmp")).collect();
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
        assert!(w.iter().all(|x| *x > 0.0), "smoothing keeps every mode reachable");
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
        for _ in 0..100 { p.adjust_speed(1.5); }
        assert_eq!(p.speed, 4.0);
        for _ in 0..100 { p.adjust_speed(0.5); }
        assert_eq!(p.speed, 0.25);
    }

    #[test]
    fn state_dir_env_override() {
        std::env::set_var("PHOSPHOR_STATE_DIR", "/tmp/phosphor-test-state");
        assert_eq!(state_dir(), Some(std::path::PathBuf::from("/tmp/phosphor-test-state")));
        std::env::remove_var("PHOSPHOR_STATE_DIR");
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test prefs` → FAIL.

- [ ] **Step 3: Implement** — above tests:

```rust
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
        Self { modes: HashMap::new(), palettes: HashMap::new(), speed: 1.0, samples: 0 }
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
        let text = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
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
        // Renormalize over observed palettes is not needed; modes only here.
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
    if let Ok(d) = std::env::var("PHOSPHOR_STATE_DIR") {
        if !d.is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    dirs::data_dir().map(|d| d.join("phosphor"))
}

pub fn prefs_path() -> Option<PathBuf> {
    state_dir().map(|d| d.join("prefs.json"))
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test prefs` → PASS.

```bash
git add src/prefs.rs src/main.rs
git commit -m "feat: preference learning store

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 7: `modes/mod.rs` + `orbital.rs`

**Files:**
- Create: `src/modes/mod.rs`, `src/modes/orbital.rs`
- Modify: `src/main.rs` (add `mod modes;`)

- [ ] **Step 1: Failing test** — append to `src/modes/mod.rs`:

```rust
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
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test modes` → FAIL.

- [ ] **Step 3: Implement `src/modes/mod.rs`** — above tests:

```rust
use crate::engine::Canvas;
use crate::palette::Palette;
use rand_chacha::ChaCha8Rng;

pub mod orbital;
pub mod plasma;
pub mod rain;
pub mod pipes;
pub mod generative;

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
```

- [ ] **Step 4: Implement `src/modes/orbital.rs`**:

```rust
use super::Mode;
use crate::clock::draw_clock;
use crate::engine::Canvas;
use crate::glyph::ramp_char;
use crate::palette::{Palette, Rgb};
use rand::Rng;
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
    sats: Vec<(f64, f64)>, // (phase, angular speed)
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
                .map(|_| (rng.random::<f64>() * std::f64::consts::TAU, 0.15 + rng.random::<f64>() * 0.5))
                .collect();
            rings.push(Ring {
                rx: 0.16 + 0.11 * i as f64,
                ry: 0.07 + 0.05 * i as f64,
                angle: rng.random::<f64>() * std::f64::consts::TAU,
                spin: (0.05 + rng.random::<f64>() * 0.1) * if i % 2 == 0 { 1.0 } else { -1.0 },
                sats,
            });
        }
        Self {
            palette: Palette::by_name("polaris").unwrap_or_else(|| Palette::all().remove(0)),
            stars,
            rings,
            ticker: TICKER_BASE.to_string(),
            ticker_scroll: 0.0,
            t: 0.0,
        }
    }

    fn refresh_ticker(&mut self, rng: &mut ChaCha8Rng) {
        let mut s = TICKER_BASE.to_string();
        let tok = 500.0 + rng.random::<f64>() * 700.0;
        let gpu = 58.0 + rng.random::<f64>() * 24.0;
        let p99 = 40.0 + rng.random::<f64>() * 120.0;
        let agents = rng.random_range(3..24);
        let ent = rng.random::<f64>();
        let mood: &str = *["luminous", "restless", "serene", "electric", "cryptic"].choose(rng).unwrap_or(&"luminous");
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
        if self.ticker_scroll > self.ticker.chars().count() as f64 + 40.0 {
            self.ticker_scroll = 0.0;
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
                let n = (u.sin() * (v * 0.7 + t * 0.11).cos() + (u * 0.6 - t * 0.07).sin() * v.sin()) * 0.25 + 0.5;
                if n > 0.72 {
                    let level = ((n - 0.72) / 0.28) * 0.35;
                    canvas.put(x, y, ramp_char(level), self.palette.sample(n * 0.6).scale(0.35));
                }
            }
        }
        // Stars.
        for star in &self.stars {
            let x = (star.x * w as f64) as i32;
            let y = (star.y * (h - 1) as f64) as i32;
            let tw = (t * (1.0 + star.layer as f64) + star.twinkle).sin() * 0.5 + 0.5;
            let ch = if star.layer == 2 { '*' } else { '.' };
            canvas.put(x, y, ch, self.palette.sample(0.15 + 0.1 * star.layer as f64 + tw * 0.2).scale(0.4 + tw * 0.6));
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
                canvas.put(x + 1, y, '»', self.palette.sample(0.75).scale(0.5));
            }
        }
        // Clock, top center.
        let hhmm = 4 * 8 - 2; // "HH:MM" = 4 digits * 8 + colon gap
        let clock_x = (w - hhmm) / 2;
        let local = t % 86400.0;
        draw_clock(canvas, clock_x.max(0), 1, local, Rgb::new(220, 230, 255), false);
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
```

- [ ] **Step 5: Run + commit**

Run: `cargo test modes` → PASS (rain/plasma/pipes don't exist yet — temporarily comment those `pub mod` lines and registry arms? No: implement stubs? No — build() only references them once they exist. Until Tasks 8–10, keep `mod` declarations for orbital only and extend registry per task. To keep every commit green: in this task declare only `pub mod orbital;` and have `build` match only "orbital" plus `_ => None`; extend in Tasks 8–10.)

Adjusted interim `src/modes/mod.rs` for this commit: declare `pub mod orbital;` only; registry arms for the other three arrive with their tasks. (The final code shown above is the end state after Task 10.)

```bash
git add src/modes
git commit -m "feat: mode trait + orbital mode

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 8: `rain.rs`

**Files:**
- Create: `src/modes/rain.rs`
- Modify: `src/modes/mod.rs` (add `pub mod rain;` + registry arm)

- [ ] **Step 1: Failing test** — append to `src/modes/mod.rs` tests:

```rust
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
```

- [ ] **Step 2: Run, verify failure** — `cargo test modes::tests::rain` → FAIL.

- [ ] **Step 3: Implement `src/modes/rain.rs`**:

```rust
use super::Mode;
use crate::engine::Canvas;
use crate::glyph::{BRAILLE, KATAKANA, ramp_char};
use crate::palette::Palette;
use rand::Rng;
use rand_chacha::ChaCha8Rng;

struct Column {
    head: f64,
    speed: f64,
    len: f64,
}

pub struct Rain {
    palette: Palette,
    columns: Vec<Column>,
    hue: f64,
    glitch: f64,
}

impl Rain {
    pub fn new(seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut columns = Vec::new();
        for _ in 0..256 {
            columns.push(Column {
                head: rng.random::<f64>() * -40.0,
                speed: 2.0 + rng.random::<f64>() * 6.0,
                len: 6.0 + rng.random::<f64>() * 14.0,
            });
        }
        Self {
            palette: Palette::by_name("rainforest").unwrap_or_else(|| Palette::all().remove(0)),
            columns,
            hue: 0.0,
            glitch: 0.0,
        }
    }

    /// Deterministic glyph for a cell so rain shimmers without extra state.
    fn cell_glyph(&self, x: i32, y: i32, t: f64) -> char {
        let bucket = ((t * 6.0) as i64 ^ (x as i64 * 31) ^ (y as i64 * 17)).unsigned_abs() as usize;
        let set = if bucket % 3 == 0 { KATAKANA } else { BRAILLE };
        set[bucket % set.len()]
    }
}

impl Mode for Rain {
    fn name(&self) -> &'static str {
        "rain"
    }

    fn update(&mut self, dt: f64, size: (u16, u16), rng: &mut ChaCha8Rng, speed: f64) {
        let dt = dt * speed;
        self.hue = (self.hue + dt * 0.015) % 1.0;
        self.glitch = (self.glitch - dt).max(0.0);
        if rng.random::<f64>() < dt * 0.06 {
            self.glitch = 0.35;
        }
        let h = size.1 as f64;
        for col in &mut self.columns {
            col.head += col.speed * dt * (h / 24.0);
            if col.head - col.len > h {
                col.head = -rng.random::<f64>() * 30.0;
                col.speed = 2.0 + rng.random::<f64>() * 6.0;
                col.len = 6.0 + rng.random::<f64>() * 14.0;
            }
        }
    }

    fn render(&self, canvas: &mut Canvas, t: f64) {
        let (w, h) = (canvas.width as i32, canvas.height as i32);
        for x in 0..w {
            let col = &self.columns[x as usize % self.columns.len()];
            let head = col.head as i32;
            for i in 0..col.len as i32 {
                let y = head - i;
                if y < 0 || y >= h {
                    continue;
                }
                let fade = 1.0 - i as f64 / col.len;
                let boosted = if self.glitch > 0.0 {
                    (fade + self.glitch * ((x ^ y) % 3) as f64).min(1.0)
                } else {
                    fade
                };
                let ch = if i == 0 {
                    crate::glyph::ramp_char(1.0)
                } else {
                    self.cell_glyph(x, y, t)
                };
                let color = self
                    .palette
                    .sample(self.hue + x as f64 / w.max(1) as f64 * 0.25)
                    .hue_shift(self.hue * 360.0 + t * 4.0)
                    .scale(0.15 + boosted * 0.85);
                canvas.put(x, y, ch, color);
            }
        }
        let _ = ramp_char(0.0); // keep ramp import used regardless of cfg churn
    }

    fn palette_name(&self) -> String {
        self.palette.name.to_string()
    }

    fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test modes` → PASS.

```bash
git add src/modes
git commit -m "feat: rain mode

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 9: `plasma.rs`

**Files:**
- Create: `src/modes/plasma.rs`
- Modify: `src/modes/mod.rs`

- [ ] **Step 1: Failing test** — append:

```rust
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
```

- [ ] **Step 2: Run, verify failure** — FAIL.

- [ ] **Step 3: Implement `src/modes/plasma.rs`**:

```rust
use super::Mode;
use crate::engine::Canvas;
use crate::glyph::ramp_char;
use crate::palette::Palette;
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
                    canvas.put(x, y, ch, self.palette.sample(level * 0.5 + self.hue).hue_shift(self.hue * 360.0));
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
```

- [ ] **Step 4: Run + commit**

Run: `cargo test modes` → PASS.

```bash
git add src/modes
git commit -m "feat: plasma mode

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 10: `pipes.rs`

**Files:**
- Create: `src/modes/pipes.rs`
- Modify: `src/modes/mod.rs`

- [ ] **Step 1: Failing test** — append:

```rust
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
```

- [ ] **Step 2: Run, verify failure** — FAIL.

- [ ] **Step 3: Implement `src/modes/pipes.rs`**:

```rust
use super::Mode;
use crate::engine::Canvas;
use crate::palette::Palette;
use rand::Rng;
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
    grid: HashMap<(i32, i32), (char, f64)>, // cell → (glyph, phase)
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
                    walker.dir = Dir { x: -old.y * turn, y: old.x * turn };
                }
                let nx = walker.x + walker.dir.x;
                let ny = walker.y + walker.dir.y;
                if walker.life <= 0.0 || nx < 1 || ny < 1 || nx >= w - 1 || ny >= h - 1 {
                    *walker = Self::spawn(rng, w, h);
                    break;
                }
                walker.x = nx;
                walker.y = ny;
                if walker.dir != old {
                    self.grid.insert((walker.x - old.x, walker.y - old.y), (Self::corner(old, walker.dir), walker.phase));
                }
                let body = if walker.dir.x != 0 { '─' } else { '│' };
                self.grid.insert((walker.x, walker.y), (body, walker.phase));
            }
        }
        // Fade the oldest trails so the floor doesn't fill solid.
        if self.grid.len() > 4000 {
            self.grid.retain(|_, (_, ph)| *ph > (self.hue % 1.0) - 0.5 && self.grid.len() <= 4000);
        }
    }

    fn render(&self, canvas: &mut Canvas, t: f64) {
        let (w, h) = (canvas.width as f64, canvas.height as f64);
        let (cx, cy) = (w / 2.0, h / 2.0);
        // Dim plasma floor.
        for y in 0..canvas.height as i32 {
            for x in 0..canvas.width as i32 {
                let d = (((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt()) * 0.14;
                let level = ((x as f64 * 0.09 + t * 0.5).sin() + (y as f64 * 0.12 - t).sin() + (d - t * 0.7).sin()) / 6.0 + 0.32;
                if level > 0.4 {
                    canvas.put(x, y, crate::glyph::ramp_char((level - 0.4) * 0.8), self.palette.sample(self.hue + level * 0.4).scale(0.25));
                }
            }
        }
        // Pipe trails.
        for (&(x, y), &(ch, phase)) in &self.grid {
            canvas.put(x, y, ch, self.palette.sample(phase * 0.5 + self.hue).hue_shift(phase * 60.0));
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
```

- [ ] **Step 4: Run + commit**

Run: `cargo test modes` → PASS.

```bash
git add src/modes
git commit -m "feat: pipes mode

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 11: `generative.rs` — SceneSpec + offline synthesizer

**Files:**
- Create: `src/generative.rs`
- Modify: `src/main.rs` (add `mod generative;`)

- [ ] **Step 1: Failing tests** — append:

```rust
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
        let c = SceneSpec::from_prompt("other words");
        assert_ne!(a.palette, "" );
        let _ = c;
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
        assert!(extract_spec("{\"mode\": \"rain\"}").is_some(), "missing fields are backfilled by sanitize");
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test generative` → FAIL.

- [ ] **Step 3: Implement** — above tests:

```rust
use crate::glyph::GlyphSet;
use crate::palette::Palette;
use serde::{Deserialize, Serialize};

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
        let _ = GlyphSet::from_name(&self.glyphs); // unknown → Mix, resolved at render
        self
    }

    /// Offline synthesizer: derive a valid scene from prompt text alone via
    /// seeded hashing. Same prompt → same scene.
    pub fn from_prompt(prompt: &str) -> SceneSpec {
        use rand::{Rng, SeedableRng};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        prompt.hash(&mut hash);
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(hash.finish());
        let palettes = Palette::all();
        let mode = ["orbital", "rain", "plasma", "pipes"][rng.random_range(0..4)];
        let glyphs = ["katakana", "braille", "ascii", "blocks"][rng.random_range(0..4)];
        SceneSpec {
            palette: palettes[rng.random_range(0..palettes.len())].name.to_string(),
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
        palette: value.get("palette").and_then(|v| v.as_str()).unwrap_or("polaris").to_string(),
        mode: value.get("mode").and_then(|v| v.as_str()).unwrap_or("plasma").to_string(),
        density: value.get("density").and_then(|v| v.as_f64()).unwrap_or(0.5),
        speed: value.get("speed").and_then(|v| v.as_f64()).unwrap_or(0.5),
        glyphs: value.get("glyphs").and_then(|v| v.as_str()).unwrap_or("mix").to_string(),
        hue_drift: value.get("hue_drift").and_then(|v| v.as_f64()).unwrap_or(0.3),
    };
    Some(spec.sanitize())
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test generative` → PASS.

```bash
git add src/generative.rs src/main.rs
git commit -m "feat: scene spec, offline synthesizer, json extraction

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 12: `llm.rs` — OpenAI-compatible client

**Files:**
- Create: `src/llm.rs`
- Modify: `src/main.rs` (add `mod llm;`)

- [ ] **Step 1: Failing tests** — append:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn from_env_requires_url_and_model() {
        std::env::set_var("PHOSPHOR_LLM_URL", "http://x:1/v1");
        std::env::set_var("PHOSPHOR_LLM_MODEL", "m");
        assert!(LlmConfig::from_env().is_some());
        std::env::remove_var("PHOSPHOR_LLM_MODEL");
        assert!(LlmConfig::from_env().is_none());
        std::env::remove_var("PHOSPHOR_LLM_URL");
    }

    #[test]
    fn parse_chat_content_reads_openai_shape() {
        let body = r#"{"choices":[{"message":{"content":"{\"palette\":\"ember\"}"}}]}"#;
        let c = parse_chat_content(body).unwrap();
        assert!(c.contains("ember"));
        assert!(parse_chat_content("{}").is_none());
        assert!(parse_chat_content("not json").is_none());
    }

    #[test]
    fn fetch_spec_against_mock_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).unwrap();
            let _req = String::from_utf8_lossy(&buf[..n]);
            let body = r#"{"choices":[{"message":{"content":"{\"palette\":\"lagoon\",\"mode\":\"plasma\",\"density\":0.8,\"speed\":0.4,\"glyphs\":\"braille\",\"hue_drift\":0.1}"}}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            use std::io::Write;
            stream.write_all(resp.as_bytes()).unwrap();
        });
        let cfg = LlmConfig { base: format!("http://{addr}/v1"), model: "mock".into(), api_key: None };
        let spec = fetch_spec(&cfg, "deep ocean").unwrap();
        assert_eq!(spec.palette, "lagoon");
        assert_eq!(spec.mode, "plasma");
        server.join().unwrap();
    }

    #[test]
    fn fetch_spec_fails_gracefully_on_dead_port() {
        let cfg = LlmConfig { base: "http://127.0.0.1:1/v1".into(), model: "x".into(), api_key: None };
        assert!(fetch_spec(&cfg, "anything").is_none());
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test llm` → FAIL.

- [ ] **Step 3: Implement** — above tests:

```rust
use crate::generative::{SceneSpec, extract_spec};

#[derive(Clone, Debug)]
pub struct LlmConfig {
    /// e.g. http://localhost:11434/v1 (Ollama) or http://localhost:8080/v1 (llama.cpp)
    pub base: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl LlmConfig {
    pub fn from_env() -> Option<LlmConfig> {
        let base = std::env::var("PHOSPHOR_LLM_URL").ok()?;
        let model = std::env::var("PHOSPHOR_LLM_MODEL").ok()?;
        if base.is_empty() || model.is_empty() {
            return None;
        }
        Some(LlmConfig { base: base.trim_end_matches('/').to_string(), model, api_key: std::env::var("PHOSPHOR_LLM_API_KEY").ok() })
    }

    /// Probe common local serving ports for an OpenAI-compatible endpoint.
    pub fn autodetect() -> Option<LlmConfig> {
        for base in ["http://localhost:11434/v1", "http://localhost:8080/v1"] {
            let url = base.replace("/v1", "") + "/health";
            if ureq::get(&url).timeout(std::time::Duration::from_millis(400)).call().is_ok() {
                let model = probe_model(base).unwrap_or_else(|| "local".to_string());
                return Some(LlmConfig { base: base.to_string(), model, api_key: None });
            }
        }
        None
    }

    pub fn resolve(offline: bool) -> Option<LlmConfig> {
        if offline {
            None
        } else {
            LlmConfig::from_env().or_else(LlmConfig::autodetect)
        }
    }
}

fn probe_model(base: &str) -> Option<String> {
    let v: serde_json::Value = ureq::get(&format!("{base}/models"))
        .timeout(std::time::Duration::from_millis(600))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    v["data"][0]["id"].as_str().map(str::to_string)
}

pub fn parse_chat_content(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v["choices"][0]["message"]["content"].as_str().map(str::to_string)
}

const SYSTEM_PROMPT: &str = "You design terminal screensaver scenes. Reply with ONLY a JSON object: {\"palette\": one of ember,lagoon,orchard,polaris,rainforest,monolith, \"mode\": one of orbital,rain,plasma,pipes, \"density\": 0..1, \"speed\": 0..1, \"glyphs\": one of katakana,braille,ascii,blocks, \"hue_drift\": 0..1}. Invent a fresh variation on the given theme.";

/// Ask the LLM for a scene; None on any failure (caller falls back to the
/// offline synthesizer).
pub fn fetch_spec(config: &LlmConfig, prompt: &str) -> Option<SceneSpec> {
    let body = serde_json::json!({
        "model": config.model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt},
        ],
        "temperature": 0.9,
        "max_tokens": 300,
    });
    let mut req = ureq::post(&format!("{}/chat/completions", config.base))
        .timeout(std::time::Duration::from_secs(5))
        .set("Content-Type", "application/json");
    if let Some(key) = &config.api_key {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    let resp = req.send_json(body).ok()?;
    let text = resp.into_string().ok()?;
    let content = parse_chat_content(&text)?;
    extract_spec(&content)
}
```

- [ ] **Step 4: Run + commit**

Run: `cargo test llm` → PASS (mock-server test included).

```bash
git add src/llm.rs src/main.rs
git commit -m "feat: openai-compatible llm client with local autodetect

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 13: `modes/generative.rs` — wrapper mode

**Files:**
- Create: `src/modes/generative.rs`
- Modify: `src/modes/mod.rs` (already declares `pub mod generative;` — add `Generative` export; `build()` still returns None for "generative" since it needs a prompt/llm config)

- [ ] **Step 1: Failing test** — append to `src/modes/mod.rs` tests:

```rust
    #[test]
    fn generative_mode_rethemes_offline() {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(8);
        let mut g = generative::Generative::new(42, Some("coral reef at night".into()), None, true);
        assert_eq!(g.name(), "generative");
        for _ in 0..10 {
            g.update(5.0, (60, 20), &mut rng, 1.0); // 50s > epoch: forces retheme
        }
        let mut c = Canvas::new(60, 20);
        g.render(&mut c, 3.0);
        assert!(!c.is_blank());
        assert_eq!(g.spec().palette, g.palette_name());
    }
```

- [ ] **Step 2: Run, verify failure** — FAIL.

- [ ] **Step 3: Implement `src/modes/generative.rs`**:

```rust
use super::Mode;
use super::{orbital::Orbital, pipes::Pipes, plasma::Plasma, rain::Rain};
use crate::engine::Canvas;
use crate::generative::SceneSpec;
use crate::llm::{self, LlmConfig};
use crate::palette::Palette;
use rand::SeedableRng;
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
    pub fn new(seed: u64, prompt: Option<String>, llm: Option<LlmConfig>, offline: bool) -> Self {
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

    fn retheme(&mut self, t: f64, seed: u64) {
        let variation = format!("{} (variation {}, t={t:.0})", self.prompt, self.spec.hue_drift);
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
            let seed = (self.synth_rng.next_u64()).wrapping_add((dt * 1000.0) as u64);
            self.retheme(0.0, seed);
        }
        let _ = rng;
        self.inner.update(dt, size, &mut self.synth_rng, speed * self.spec.speed.max(0.05));
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
```

- [ ] **Step 4: Run + commit**

Run: `cargo test modes` → PASS.

```bash
git add src/modes
git commit -m "feat: generative wrapper mode with periodic retheming

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 14: `app.rs` — scheduler, crossfade, input, dwell

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs` (add `mod app;`)

- [ ] **Step 1: Failing tests** — append:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(scripted: bool) -> App {
        App::new(Config { scripted, seed: 42, learn: false, ..Config::default() })
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
    fn never_repeats_mode_back_to_back() {
        let mut app = App::new(Config { scripted: true, seed: 7, learn: false, ..Config::default() });
        let mut prev = app.current_name().to_string();
        for _ in 0..400 {
            app.update(0.1, &[]);
            let now = app.current_name().to_string();
            assert_ne!(prev, now, "repeated {prev} twice in a row");
            prev = now;
        }
    }

    #[test]
    fn duration_triggers_exit() {
        let mut app = App::new(Config { duration: Some(1.0), learn: false, ..Config::default() });
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
        std::env::set_var("PHOSPHOR_STATE_DIR", dir.path());
        let mut app = App::new(Config { scripted: true, seed: 1, learn: true, ..Config::default() });
        for _ in 0..50 {
            app.update(0.1, &[]);
        }
        app.update(0.1, &[Input::Like]);
        app.save_prefs();
        let p = crate::prefs::Prefs::load(&crate::prefs::prefs_path().unwrap());
        assert!(p.samples > 0, "dwell should have been recorded");
        assert!(p.modes.values().any(|m| m.likes == 1), "like should have landed on current mode");
        std::env::remove_var("PHOSPHOR_STATE_DIR");
    }

    #[test]
    fn single_mode_config_sticks() {
        let mut app = App::new(Config { mode: Some("rain".into()), seed: 3, learn: false, ..Config::default() });
        for _ in 0..200 {
            app.update(0.1, &[]);
            assert_eq!(app.current_name(), "rain");
        }
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test app` → FAIL.

- [ ] **Step 3: Implement `src/app.rs`** — above tests:

```rust
use crate::engine::Canvas;
use crate::modes::{self, Mode, MODE_NAMES};
use crate::prefs::{self, Prefs};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

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
    prefs_path: Option<std::path::PathBuf>,
    t: f64,
    since_rotate: f64,
    rotate_every: f64,
    dwell: f64,
    size: (u16, u16),
    scripted_idx: usize,
    quit: bool,
}

const FADE_SECS: f64 = 1.2;
const MIN_W: u16 = 20;
const MIN_H: u16 = 8;

impl App {
    pub fn new(config: Config) -> Self {
        let rng = ChaCha8Rng::seed_from_u64(config.seed);
        let mode_list: Vec<Box<dyn Mode>> = match &config.mode {
            Some(name) => modes::build(name, config.seed)
                .into_iter()
                .collect(),
            None => {
                let mut v: Vec<Box<dyn Mode>> = ["orbital", "rain", "plasma", "pipes"]
                    .iter()
                    .filter_map(|n| modes::build(n, config.seed))
                    .collect();
                v.push(Box::new(modes::generative::Generative::new(
                    config.seed,
                    config.prompt.clone(),
                    crate::llm::LlmConfig::resolve(config.offline),
                    config.offline,
                )));
                v
            }
        };
        let prefs_path = prefs::prefs_path();
        let prefs = prefs_path.as_ref().map(|p| Prefs::load(p)).unwrap_or_default();
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
            scripted_idx: 0,
            quit: false,
        };
        if app.modes.is_empty() {
            app.modes.push(modes::build("plasma", app.config.seed).expect("plasma always builds"));
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

    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    pub fn update(&mut self, dt: f64, inputs: &[Input]) {
        let dt = dt.clamp(0.0, 0.25); // tab-away jumps must not explode the sim
        self.t += dt;
        self.dwell += dt;
        self.since_rotate += dt;

        for input in inputs {
            match input {
                Input::Quit => self.quit = true,
                Input::Next => self.rotate(true, 1),
                Input::Prev => self.rotate(true, -1),
                Input::Like => {
                    let name = self.current_name().to_string();
                    self.prefs.record_vote(&name, true);
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
            }
        }

        if let Some(d) = self.config.duration {
            if self.t >= d {
                self.quit = true;
            }
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
    }

    fn rotate(&mut self, manual: bool, _dir: i32) {
        if self.modes.len() < 2 {
            return;
        }
        let old = self.current;
        // Account dwell/skip for the outgoing mode before switching.
        let old_name = self.modes[old].name().to_string();
        let old_palette = self.modes[old].palette_name();
        self.prefs.record_dwell(&old_name, Some(&old_palette), self.dwell);
        if manual {
            self.prefs.record_skip(&old_name);
        }
        self.persist_prefs();

        let next = if self.config.scripted {
            self.scripted_idx = (self.scripted_idx + 1) % self.modes.len();
            self.scripted_idx
        } else {
            self.weighted_pick()
        };
        if next == old {
            return;
        }
        // Snapshot the outgoing frame for the crossfade.
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
        let roll: f64 = self.rng.random::<f64>() * weights.iter().sum::<f64>();
        let mut acc = 0.0;
        for (i, w) in weights.iter().enumerate() {
            acc += w;
            if roll <= acc {
                return i;
            }
        }
        self.current
    }

    /// Render the current frame (with any active crossfade) into the canvas.
    pub fn compose(&mut self) -> &Canvas {
        self.canvas.resize(self.size.0.max(1), self.size.1.max(1));
        self.canvas.clear();
        let (w, h) = self.size;
        if w < MIN_W || h < MIN_H {
            let fg = crate::palette::Rgb::new(200, 200, 210);
            self.canvas.text(1, 1, "phosphor", fg);
            self.canvas.text(1, 2, "terminal too small", fg.scale(0.6));
            self.canvas.text(1, 3, &format!("need {MIN_W}x{MIN_H}, got {w}x{h}"), fg.scale(0.6));
            return &self.canvas;
        }
        let t = self.t;
        self.modes[self.current].render(&mut self.canvas, t);
        if let Some((from, ft)) = &self.fade_from {
            let blended = from.blend(&self.canvas, *ft);
            self.canvas = blended;
        }
        &self.canvas
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

    /// Final dwell accounting + flush. Called once at exit.
    pub fn save_prefs(&mut self) {
        let name = self.modes[self.current].name().to_string();
        let palette = self.modes[self.current].palette_name();
        self.prefs.record_dwell(&name, Some(&palette), self.dwell);
        self.dwell = 0.0;
        self.persist_prefs();
    }

    /// Human-readable learning report for `phosphor stats`.
    pub fn stats_report(&self) -> String {
        let names: Vec<&'static str> = self.modes.iter().map(|m| m.name()).collect();
        let weights = self.prefs.weights(&names);
        let mut lines = vec![
            "phosphor — learned preferences".to_string(),
            format!("samples: {}   speed: {:.2}x", self.prefs.samples, self.prefs.speed),
            String::new(),
            "mode          dwell      skips likes dislikes weight".to_string(),
        ];
        for (i, n) in names.iter().enumerate() {
            let s = self.prefs.modes.get(*n).cloned().unwrap_or_default();
            lines.push(format!(
                "{:<13} {:>8.1}s {:>6} {:>5} {:>8} {:>6.1}%",
                n, s.dwell_s, s.skips, s.likes, s.dislikes, weights.get(i).copied().unwrap_or(0.0) * 100.0
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
```

- [ ] **Step 4: Run + commit**

Run: `cargo test app` → PASS.

```bash
git add src/app.rs src/main.rs
git commit -m "feat: scheduler, crossfade, input, dwell learning

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 15: `main.rs` — CLI + terminal lifecycle

**Files:**
- Modify: `src/main.rs` (full rewrite)

- [ ] **Step 1: Failing test** — keep in `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_run() {
        let cli = Cli::try_parse_from(["phosphor"]).unwrap();
        assert!(matches!(cli.command, None));
        assert!(cli.learn);
    }

    #[test]
    fn parses_mode_and_flags() {
        let cli = Cli::try_parse_from(["phosphor", "--mode", "rain", "--seed", "9", "--no-learn", "--offline", "--scripted", "--duration", "5"]).unwrap();
        assert_eq!(cli.mode.as_deref(), Some("rain"));
        assert!(!cli.learn);
        assert!(cli.offline);
        assert!(cli.scripted);
        assert_eq!(cli.duration, Some(5.0));
        assert_eq!(cli.seed, 9);
    }

    #[test]
    fn parses_stats_and_forget() {
        assert!(matches!(Cli::try_parse_from(["phosphor", "stats"]).unwrap().command, Some(Commands::Stats)));
        assert!(matches!(Cli::try_parse_from(["phosphor", "forget"]).unwrap().command, Some(Commands::Forget)));
    }

    #[test]
    fn key_mapping() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        let k = |code: KeyCode| KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: crossterm::event::KeyEventState::NONE };
        assert_eq!(map_key(k(KeyCode::Right)), app::Input::Next);
        assert_eq!(map_key(k(KeyCode::Char('n'))), app::Input::Next);
        assert_eq!(map_key(k(KeyCode::Left)), app::Input::Prev);
        assert_eq!(map_key(k(KeyCode::Char('l'))), app::Input::Like);
        assert_eq!(map_key(k(KeyCode::Char('d'))), app::Input::Dislike);
        assert_eq!(map_key(k(KeyCode::Char('+'))), app::Input::Faster);
        assert_eq!(map_key(k(KeyCode::Char('-'))), app::Input::Slower);
        assert_eq!(map_key(k(KeyCode::Char('p'))), app::Input::NextPalette);
        assert_eq!(map_key(k(KeyCode::Esc)), app::Input::Quit);
        assert_eq!(map_key(k(KeyCode::Char('x'))), app::Input::Quit);
    }
}
```

- [ ] **Step 2: Run, verify failure** — `cargo test main_tests` → FAIL.

- [ ] **Step 3: Implement `src/main.rs`** (modules + CLI + lifecycle; tests appended from Step 1):

```rust
mod app;
mod engine;
mod generative;
mod glyph;
mod llm;
mod modes;
mod palette;
mod prefs;
mod clock;

use app::{App, Config, Input};
use clap::{Parser, Subcommand};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};

#[derive(Parser, Debug)]
#[command(name = "phosphor", about = "a truecolor, self-learning terminal screensaver", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Force a single mode (orbital, rain, plasma, pipes, generative).
    #[arg(long)]
    mode: Option<String>,
    /// Theme prompt for the generative mode.
    #[arg(long)]
    prompt: Option<String>,
    /// RNG seed (reproducible runs; used for the demo recording).
    #[arg(long, default_value_t = 0x5eed)]
    seed: u64,
    /// Frame rate cap.
    #[arg(long, default_value_t = 30.0)]
    fps: f64,
    /// Auto-exit after N seconds.
    #[arg(long)]
    duration: Option<f64>,
    /// Disable preference learning (still reads prefs).
    #[arg(long)]
    no_learn: bool,
    /// Never call an LLM (offline generative mode).
    #[arg(long)]
    offline: bool,
    /// Deterministic fast-rotation showcase (for recordings).
    #[arg(long)]
    scripted: bool,
    #[arg(long, default_value_t = true)]
    learn: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Print learned preference report.
    Stats,
    /// Erase learned preferences.
    Forget,
}

fn map_key(key: KeyEvent) -> Input {
    match key.code {
        KeyCode::Right | KeyCode::Char('n') => Input::Next,
        KeyCode::Left => Input::Prev,
        KeyCode::Char('l') | KeyCode::Char('L') => Input::Like,
        KeyCode::Char('d') | KeyCode::Char('D') => Input::Dislike,
        KeyCode::Char('+') | KeyCode::Char('=') => Input::Faster,
        KeyCode::Char('-') => Input::Slower,
        KeyCode::Char('p') | KeyCode::Char('P') => Input::NextPalette,
        _ => Input::Quit,
    }
}

fn config_from(cli: &Cli) -> Config {
    Config {
        mode: cli.mode.clone(),
        prompt: cli.prompt.clone(),
        seed: cli.seed,
        fps: cli.fps,
        duration: cli.duration,
        learn: cli.learn && !cli.no_learn,
        offline: cli.offline,
        scripted: cli.scripted,
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
}

fn run(config: Config) -> io::Result<()> {
    let mut app = App::new(config.clone());
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Best-effort terminal restore on panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    let frame_budget = std::time::Duration::from_secs_f64((1.0 / config.fps.max(1.0)).clamp(1.0 / 240.0, 1.0));
    let mut last = std::time::Instant::now();
    let result = loop {
        let now = std::time::Instant::now();
        let dt = now.duration_since(last).as_secs_f64();
        last = now;

        let mut inputs = Vec::new();
        while event::poll(std::time::Duration::from_millis(0))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => inputs.push(map_key(key)),
                Event::Resize(w, h) => app.resize(w, h),
                Event::Mouse(_) => inputs.push(Input::Quit),
                _ => {}
            }
        }
        if let Event::Resize(w, h) = crossterm::terminal::size().map(|(w, h)| Event::Resize(w, h)).unwrap_or(Event::Resize(80, 24)) {
            app.resize(w, h);
        }

        app.update(dt, &inputs);
        terminal.draw(|f| {
            let canvas = app.compose();
            canvas.paint(f);
        })?;
        if app.finished() {
            break Ok(());
        }
        let elapsed = now.elapsed();
        if elapsed < frame_budget {
            std::thread::sleep(frame_budget - elapsed);
        }
    };

    let _ = terminal.show_cursor();
    restore_terminal();
    app.save_prefs();
    result
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Forget) => {
            if let Some(path) = prefs::prefs_path() {
                let _ = std::fs::remove_file(&path);
                println!("forgot preferences at {}", path.display());
            } else {
                println!("no state dir found; nothing to forget");
            }
            Ok(())
        }
        Some(Commands::Stats) => {
            let app = App::new(Config { learn: false, ..Config::default() });
            if app.prefs().samples == 0 {
                println!("no learned preferences yet — run phosphor for a while");
            } else {
                println!("{}", app.stats_report());
            }
            Ok(())
        }
        None => run(config_from(&cli)),
    }
}
```

Note: `Event::Resize` handling via `crossterm::terminal::size()` each loop keeps the canvas in sync even when the resize event races the draw; harmless double-call.

- [ ] **Step 4: Run + commit**

Run: `cargo test` → PASS (whole suite).

```bash
git add src/main.rs
git commit -m "feat: cli and terminal lifecycle

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 16: headless integration tests

**Files:**
- Create: `tests/headless.rs`

- [ ] **Step 1: Write the test file:**

```rust
use phosphor::app::{App, Config, Input};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn headless_terminal(w: u16, h: u16) -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(w, h)).expect("test backend")
}

#[test]
fn app_runs_60_frames_headlessly() {
    let mut app = App::new(Config { scripted: true, seed: 42, learn: false, ..Config::default() });
    app.resize(90, 26);
    let mut term = headless_terminal(90, 26);
    for _ in 0..60 {
        app.update(1.0 / 30.0, &[]);
        term.draw(|f| app.compose().paint(f)).expect("draw");
    }
    let buf = term.backend().buffer();
    let lit = buf.content().iter().filter(|c| c.symbol() != " " && c.symbol() != "").count();
    assert!(lit > 100, "expected a rich frame, got {lit} lit cells");
}

#[test]
fn resize_mid_run_stays_consistent() {
    let mut app = App::new(Config { seed: 5, learn: false, ..Config::default() });
    let mut term = headless_terminal(60, 18);
    for (w, h) in [(60u16, 18u16), (100, 30), (24, 10), (60, 18)] {
        app.resize(w, h);
        app.update(0.05, &[]);
        term.resize(ratatui::prelude::Size { width: w, height: h }).expect("resize");
        term.draw(|f| app.compose().paint(f)).expect("draw");
    }
}

#[test]
fn tiny_terminal_shows_notice_not_panic() {
    let mut app = App::new(Config { seed: 5, learn: false, ..Config::default() });
    app.resize(10, 4);
    app.update(0.05, &[]);
    let canvas = app.compose();
    assert!(!canvas.is_blank(), "small-size notice should render");
}

#[test]
fn keyboard_like_does_not_panic_and_quit_works() {
    let mut app = App::new(Config { seed: 6, learn: false, ..Config::default() });
    app.resize(80, 24);
    for inputs in [
        &[Input::Like][..],
        &[Input::Faster][..],
        &[Input::Slower][..],
        &[Input::NextPalette][..],
        &[Input::Next][..],
        &[Input::Prev][..],
        &[Input::Dislike][..],
        &[Input::Quit][..],
    ] {
        app.update(0.033, inputs);
    }
    assert!(app.finished());
}

#[test]
fn duration_exits_within_budget() {
    let mut app = App::new(Config { duration: Some(2.0), seed: 1, learn: false, ..Config::default() });
    app.resize(80, 24);
    let mut frames = 0;
    while !app.finished() && frames < 500 {
        app.update(1.0 / 60.0, &[]);
        frames += 1;
    }
    assert!(app.finished(), "never exited despite duration");
    assert!(frames <= 130, "exited too late: {frames} frames");
}
```

- [ ] **Step 2: Run** — `cargo test --test headless` → PASS. (Requires `lib.rs`: add `src/lib.rs` re-exporting modules so integration tests can import:

```rust
pub mod app;
pub mod engine;
pub mod generative;
pub mod glyph;
pub mod llm;
pub mod modes;
pub mod palette;
pub mod prefs;
pub mod clock;
```

and `src/main.rs` keeps its own `mod` declarations — binary builds the same code. Cargo builds both lib and bin from the same sources without conflict.)

- [ ] **Step 3: Commit**

```bash
git add src/lib.rs src/main.rs tests/headless.rs
git commit -m "test: headless end-to-end tests

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 17: green gate + adversarial.sh bug sweep

**Files:**
- Possibly any src file (fixes).

- [ ] **Step 1: Gate**

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

Fix every warning/error until all three are green.

- [ ] **Step 2: Build the adversarial CLI from the sibling repo**

```bash
cd /var/home/a/code/adversarial.sh
cargo build --release -p adversarial-cli
ls -la target/release/adversarial
```

Expected: release binary exists. (This repo's target-dir config may redirect the output — if so, use the printed path.)

- [ ] **Step 3: Run the deep scan on phosphor**

```bash
cd /var/home/a/code/phosphor
/var/home/a/code/adversarial.sh/target/release/adversarial scan . --agent opencode
```

(If the binary path differs, use the actual one. `--agent opencode` uses the locally installed, already-authenticated opencode CLI; no API keys.)

Expected: exit code 0 (clean), 10 (triage-only), or 11 (confirmed findings).

- [ ] **Step 4: Fix + re-run until clean**

For every confirmed finding: fix in code, add/adjust a regression test, re-run Step 1 gate, then re-run Step 3. Loop until the scan reports no confirmed findings.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "fix: adversarial scan findings

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

### Task 18: demo recording, README, LICENSE, ship

**Files:**
- Create: `README.md`, `LICENSE`, `demo.gif`, `demo.svg`, `scripts/record_demo.py`

- [ ] **Step 1: Build release + smoke-test 3 seconds in a real pty**

```bash
cd /var/home/a/code/phosphor
cargo build --release
timeout 5 script -qec "PHOSPHOR_STATE_DIR=/tmp/phosphor-demo ./target/release/phosphor --seed 42 --scripted --offline --duration 3 --no-learn" /dev/null || true
```

Expected: runs ~3s and exits; no panic output. (Headless CI-style pty; final visual check happens in the recording.)

- [ ] **Step 2: Recording helper (forces a 110x34 pty regardless of caller size)**

Create `scripts/record_demo.py`:

```python
#!/usr/bin/env python3
"""Record the phosphor demo in a fixed-size pty via asciinema."""
import os
import pty
import fcntl
import struct
import subprocess
import sys

COLS, ROWS = 110, 34
CMD = [
    "asciinema", "rec",
    "--overwrite",
    "-c", "./target/release/phosphor --seed 42 --scripted --offline --no-learn --duration 10",
    "demo.cast",
]

def main() -> int:
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        os.environ.setdefault("PHOSPHOR_STATE_DIR", "/tmp/phosphor-demo")
        os.execvp(CMD[0], CMD)
    fcntl.ioctl(fd, termios_TIOCSWINSZ := 0x5414, struct.pack("HHHH", ROWS, COLS, 0, 0))
    try:
        _, status = os.waitpid(pid, 0)
    except KeyboardInterrupt:
        os.kill(pid, 9)
        return 130
    return os.waitstatus_to_exitcode(status)

if __name__ == "__main__":
    sys.exit(main())
```

Run: `python3 scripts/record_demo.py && asciinema --version && head -c 200 demo.cast`
Expected: exit 0; `demo.cast` exists and starts with `{"version": 2`.

- [ ] **Step 3: Render the gif + svg**

```bash
agg demo.cast demo.gif
agg demo.cast demo.svg
ls -la demo.gif demo.svg
```

Expected: both files exist and are non-trivial in size (>200 KiB gif).

- [ ] **Step 4: Visual verification**

Open `demo.gif` (ReadMediaFile) and confirm: multiple modes visible, clock/ticker legible, no obvious garbage frames. Re-record with a different `--seed` if a mode looks broken.

- [ ] **Step 5: LICENSE (MIT)**

```
MIT License

Copyright (c) 2026 awdemos

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 6: README.md**

```markdown
# phosphor

A truecolor, self-learning terminal screensaver. Rust + ratatui, local-first,
LLM-optional.

![phosphor demo](demo.gif)

## Modes

| mode | what it does |
|---|---|
| `orbital` | parallax starfield, orbital rings, nebula, block clock, telemetry ticker |
| `rain` | katakana/braille glyph rain with glitch bursts |
| `plasma` | demoscene plasma through morphing palettes |
| `pipes` | pipe walkers over a plasma floor |
| `generative` | re-themes itself from an LLM prompt (or offline hash-synthesis) |

`phosphor` (no args) runs `wander`: it rotates through modes on a learned
schedule — it watches how long you linger, which palettes you keep, what you
skip, and quietly re-weights what it shows you. All learning stays in
`~/.local/share/phosphor/prefs.json`. `phosphor stats` shows what it learned;
`phosphor forget` wipes it.

## Install

    cargo install --git https://github.com/awdemos/phosphor

Or build from source: `cargo build --release` → `target/release/phosphor`.
Run it fullscreen with your terminal's fullscreen key (e.g. F11) for the full
effect. Any keypress or mouse movement exits — it's a screensaver.

## Keys

    ← → n   switch mode (counts as a skip)      p   next palette
    l d     like / dislike (feeds the learner)  +/- playback speed (learned)
    q esc   exit (any other key exits too)

## Options

    --mode MODE        orbital|rain|plasma|pipes|generative (default: wander)
    --prompt "TEXT"    theme for the generative mode
    --seed N           reproducible run (used for the demo recording)
    --fps N            frame cap (default 30)
    --duration SECS    auto-exit (used for recordings)
    --no-learn         don't write preference updates
    --offline          never call an LLM
    --scripted         deterministic fast-rotation showcase

## LLM hookup (optional)

The generative mode can ask a local LLM for fresh scene specs. It speaks
OpenAI-compatible chat completions and auto-detects Ollama (:11434) and
llama.cpp server (:8080), or use any endpoint:

    export PHOSPHOR_LLM_URL=http://localhost:11434/v1
    export PHOSPHOR_LLM_MODEL=llama3.2
    phosphor --mode generative --prompt "deep ocean phosphorescence"

With no endpoint reachable it falls back to a deterministic synthesizer seeded
from your prompt — same prompt, same scene.

## How the learning works

Every mode rotation records dwell seconds (and palette dwell, skips, likes,
dislikes) to a JSON file. Selection weights are smoothed dwell with exponential
vote factors, so liked modes surface more and skipped modes fade — while every
mode stays reachable. Nothing leaves your machine.

## Demo

`demo.cast` is the raw asciinema recording (10s, seeded); `demo.svg` the same
thing in vector form.
```

- [ ] **Step 7: Create the GitHub repo and push**

```bash
git add -A
git commit -m "docs: readme, license, demo recording

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
gh repo create awdemos/phosphor --public \
  --description "A truecolor, self-learning terminal screensaver (Rust + ratatui)" \
  --source . --remote origin --push
```

Expected: `https://github.com/awdemos/phosphor` created with all commits pushed.

- [ ] **Step 8: Verify**

```bash
gh repo view awdemos/phosphor --json url,isPrivate,defaultBranchRef -q '{url: .url, private: .isPrivate, branch: .defaultBranchRef.name}'
```

Expected: url `https://github.com/awdemos/phosphor`, `private: false`.

---

## Self-review notes (coverage map)

- [ ] **Step 8: Verify**

```bash
gh repo view awdemos/phosphor --json url,isPrivate,defaultBranchRef -q '{url: .url, private: .isPrivate, branch: .defaultBranchRef.name}'
```

Expected: url `https://github.com/awdemos/phosphor`, `private: false`.

---

### Task 19: Password-protected lock overlay

**Files:**
- Create: `src/lock.rs`
- Modify: `src/app.rs`, `src/main.rs`, `src/engine.rs`

**Shared API additions:**
- `Canvas::dim(&mut self, factor: f64)` — scale all foreground colors by `factor`.
- `Input::{Char(char), Backspace, Submit}` — password typing.
- `Config::{locked: bool, password: Option<Secret>}`.

- [ ] **Step 1: Add `Canvas::dim` to `src/engine.rs`:**

```rust
    pub fn dim(&mut self, factor: f64) {
        let f = factor.clamp(0.0, 1.0);
        for cell in &mut self.cells {
            if cell.ch != ' ' && cell.ch != '\0' {
                cell.fg = cell.fg.scale(f);
            }
        }
    }
```

Add a test: write a colored cell, dim it, assert color scaled.

- [ ] **Step 2: Implement `src/lock.rs`:**

```rust
/// In-memory password storage with best-effort zeroing and a constant-time
/// equality check. This is a casual screensaver lock, not a vault.
#[derive(Clone, Debug)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn empty() -> Self {
        Self(String::new())
    }

    pub fn push(&mut self, c: char) {
        self.0.push(c);
    }

    pub fn pop(&mut self) {
        self.0.pop();
    }

    pub fn clear(&mut self) {
        // Best-effort memory scrub before reallocation/clear.
        unsafe {
            self.0.as_bytes_mut().fill(0);
        }
        self.0.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn constant_time_eq(&self, other: &Self) -> bool {
        constant_time_bytes_eq(self.0.as_bytes(), other.0.as_bytes())
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}

fn constant_time_bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_basic() {
        assert!(Secret::new("foo").constant_time_eq(&Secret::new("foo")));
        assert!(!Secret::new("foo").constant_time_eq(&Secret::new("bar")));
        assert!(!Secret::new("foo").constant_time_eq(&Secret::new("fooo")));
        assert!(Secret::empty().constant_time_eq(&Secret::empty()));
    }

    #[test]
    fn push_pop_clear() {
        let mut s = Secret::empty();
        s.push('a');
        s.push('b');
        assert!(!s.is_empty());
        s.pop();
        assert!(s.constant_time_eq(&Secret::new("a")));
        s.clear();
        assert!(s.is_empty());
    }
}
```

- [ ] **Step 3: Update `src/app.rs`** — add to `Config`:

```rust
    pub locked: bool,
    pub password: Option<crate::lock::Secret>,
```

Add `Input::{Char(char), Backspace, Submit}` to the `Input` enum.

Add fields to `App`:

```rust
    locked: bool,
    password: Option<crate::lock::Secret>,
    entered: crate::lock::Secret,
    wrong_timer: f64,
    unlock_glow: f64,
```

In `App::new`, initialize `locked: config.locked && config.password.is_some()`, `password: config.password.clone()`, `entered: Secret::empty()`, `wrong_timer: 0.0`, `unlock_glow: 0.0`.

Modify `App::update` to short-circuit when `self.locked`:

```rust
        if self.locked {
            for input in inputs {
                match input {
                    Input::Char(c) => self.entered.push(*c),
                    Input::Backspace => self.entered.pop(),
                    Input::Submit => {
                        if self.password.as_ref().is_some_and(|p| p.constant_time_eq(&self.entered)) {
                            self.locked = false;
                            self.entered.clear();
                            self.unlock_glow = 1.0;
                        } else {
                            self.wrong_timer = 0.6;
                            self.entered.clear();
                        }
                    }
                    _ => {}
                }
            }
            self.wrong_timer = (self.wrong_timer - dt).max(0.0);
            self.unlock_glow = (self.unlock_glow - dt).max(0.0);
            return;
        }
```

Place this **before** the normal input handling. Dwell and mode updates do not run while locked, so learning pauses.

In `App::compose`, after rendering the mode and before returning, if locked draw the lock overlay; if `unlock_glow > 0` draw a brief green flash.

```rust
        if self.locked {
            self.canvas.dim(0.35);
            self.draw_lock_overlay();
        } else if self.unlock_glow > 0.0 {
            self.draw_unlock_flash();
        }
```

Add helper methods to `App`:

```rust
    fn draw_lock_overlay(&mut self) {
        let (w, h) = (self.canvas.width as i32, self.canvas.height as i32);
        let box_w = (w - 8).clamp(36, 60);
        let box_h = 9;
        let x0 = (w - box_w) / 2;
        let y0 = (h - box_h) / 2;
        let fg = crate::palette::Rgb::new(220, 225, 235);
        let red = crate::palette::Rgb::new(255, 60, 60);
        let border = if self.wrong_timer > 0.0 { red } else { fg };
        // Box outline.
        for x in x0..x0 + box_w {
            self.canvas.put(x, y0, '─', border);
            self.canvas.put(x, y0 + box_h - 1, '─', border);
        }
        for y in y0..y0 + box_h {
            self.canvas.put(x0, y, '│', border);
            self.canvas.put(x0 + box_w - 1, y, '│', border);
        }
        self.canvas.put(x0, y0, '┌', border);
        self.canvas.put(x0 + box_w - 1, y0, '┐', border);
        self.canvas.put(x0, y0 + box_h - 1, '└', border);
        self.canvas.put(x0 + box_w - 1, y0 + box_h - 1, '┘', border);

        let title = "SCREENSAVER LOCKED";
        let tx = x0 + (box_w - title.len() as i32) / 2;
        self.canvas.text(tx, y0 + 2, title, fg);

        let prompt = "password:";
        let px = x0 + 4;
        self.canvas.text(px, y0 + 4, prompt, fg.scale(0.8));
        let stars: String = std::iter::repeat('*').take(self.entered.0.chars().count()).collect();
        self.canvas.text(px + prompt.len() as i32 + 1, y0 + 4, &stars, fg);

        let hint = "Enter to submit · Backspace to delete";
        let hx = x0 + (box_w - hint.len() as i32) / 2;
        self.canvas.text(hx, y0 + box_h - 2, hint, fg.scale(0.55));
    }

    fn draw_unlock_flash(&mut self) {
        let (w, h) = (self.canvas.width as i32, self.canvas.height as i32);
        let msg = "UNLOCKED";
        let x = (w - msg.len() as i32) / 2;
        let y = h / 2;
        let g = crate::palette::Rgb::new(80, 220, 120).scale(self.unlock_glow.min(1.0));
        self.canvas.text(x, y, msg, g);
    }
```

Note: `entered.0` is private; add a public method `Secret::len(&self) -> usize` returning `self.0.chars().count()` or `self.0.len()` (chars count for display). Add `Secret::masked(&self) -> String` returning asterisks.

Add tests to `src/app.rs` tests:

```rust
    #[test]
    fn locked_mode_requires_password() {
        let mut app = App::new(Config {
            scripted: true,
            seed: 1,
            learn: false,
            locked: true,
            password: Some(crate::lock::Secret::new("secret")),
            ..Config::default()
        });
        app.resize(80, 24);
        // Typing the wrong password and submitting keeps it locked.
        app.update(0.1, &[Input::Char('w'), Input::Char('r'), Input::Char('o'), Input::Char('n'), Input::Submit]);
        assert!(app.locked, "should still be locked after wrong password");
        // Typing the right password unlocks.
        app.update(0.1, &[Input::Char('s'), Input::Char('e'), Input::Char('c'), Input::Char('r'), Input::Char('e'), Input::Char('t'), Input::Submit]);
        assert!(!app.locked, "should unlock");
    }

    #[test]
    fn locked_mode_ignores_quit() {
        let mut app = App::new(Config {
            seed: 2,
            learn: false,
            locked: true,
            password: Some(crate::lock::Secret::new("x")),
            ..Config::default()
        });
        app.resize(60, 20);
        app.update(0.1, &[Input::Quit, Input::Char('x'), Input::Submit]);
        assert!(!app.locked);
    }

    #[test]
    fn locked_mode_does_not_advance_dwell() {
        let mut app = App::new(Config {
            scripted: true,
            seed: 3,
            learn: false,
            locked: true,
            password: Some(crate::lock::Secret::new("ok")),
            ..Config::default()
        });
        app.resize(60, 20);
        let name0 = app.current_name().to_string();
        for _ in 0..100 {
            app.update(0.1, &[]);
        }
        assert_eq!(app.current_name(), name0, "mode should not rotate while locked");
    }
```

- [ ] **Step 4: Update `src/main.rs`** — add `mod lock;`, add `Commands::Lock { password: Option<String> }`, add `--password` top-level flag, and two key mapping functions:

```rust
#[derive(Parser, Debug)]
#[command(name = "phosphor", about = "...", version)]
struct Cli {
    ...
    #[arg(long)]
    password: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Stats,
    Forget,
    Lock {
        #[arg(long)]
        password: Option<String>,
    },
}

fn map_key(key: KeyEvent) -> Input {
    match key.code {
        KeyCode::Right | KeyCode::Char('n') => Input::Next,
        KeyCode::Left => Input::Prev,
        KeyCode::Char('l') | KeyCode::Char('L') => Input::Like,
        KeyCode::Char('d') | KeyCode::Char('D') => Input::Dislike,
        KeyCode::Char('+') | KeyCode::Char('=') => Input::Faster,
        KeyCode::Char('-') => Input::Slower,
        KeyCode::Char('p') | KeyCode::Char('P') => Input::NextPalette,
        _ => Input::Quit,
    }
}

fn map_locked_key(key: KeyEvent) -> Input {
    match key.code {
        KeyCode::Char(c) => Input::Char(c),
        KeyCode::Backspace => Input::Backspace,
        KeyCode::Enter => Input::Submit,
        _ => Input::Char('\0'), // ignored by locked handler
    }
}
```

In `config_from`, set password:

```rust
    let password_text = cli
        .password
        .clone()
        .or_else(|| std::env::var("PHOSPHOR_PASSWORD").ok())
        .or_else(|| match &cli.command {
            Some(Commands::Lock { password }) => password.clone(),
            _ => None,
        });
    let password = password_text.map(|p| {
        if p.is_empty() {
            None
        } else {
            Some(crate::lock::Secret::new(p))
        }
    }).flatten();

    Config {
        ...
        locked: matches!(cli.command, Some(Commands::Lock { .. })) || cli.password.is_some(),
        password,
    }
```

If `locked` requested but password is None, print an error and exit:

```rust
fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let locked_command = matches!(cli.command, Some(Commands::Lock { .. }));
    let password_text = ...
    if locked_command && password_text.is_none() {
        eprintln!("phosphor lock: set PHOSPHOR_PASSWORD or pass --password");
        std::process::exit(2);
    }
    ...
}
```

In the event loop, choose mapper based on `app.locked()` (add `pub fn is_locked(&self) -> bool` to App):

```rust
let inputs: Vec<Input> = events
    .iter()
    .filter_map(|e| match e {
        Event::Key(k) if k.kind == KeyEventKind::Press => {
            Some(if app.is_locked() { map_locked_key(*k) } else { map_key(*k) })
        }
        _ => None,
    })
    .collect();
```

Ignore mouse while locked:

```rust
Event::Mouse(_) => {
    if !app.is_locked() {
        inputs.push(Input::Quit);
    }
}
```

- [ ] **Step 5: Run full gate** — `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.

- [ ] **Step 6: Update README with lock section** — add after the modes table:

```markdown
## Lock mode

`phosphor lock --password "s3cr3t"` (or `PHOSPHOR_PASSWORD=s3cr3t phosphor lock`)
starts the screensaver behind a password overlay. While locked the animation
freezes and key input is ignored except the password prompt. Unlock resumes the
show. This is a casual deterrent with constant-time password comparison, not a
cryptographic vault.
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: password-protected lock overlay

Co-Authored-By: Adversarial.sh <noreply@adversarial.sh>"
```

---

## Self-review notes (coverage map)

- Spec "Render engine" → Tasks 2, 14, 15, 19. Modes → Tasks 7–10, 13. SceneSpec/LLM → Tasks 11–12. Learning → Tasks 6, 14. Keys/CLI → 15, 19. Tests → all TDD tasks + 16, 19. Green gate → 17. adversarial.sh sweep → 17. Demo/README/repo → 18, 19. Fullscreen semantics → README + 15 (alternate screen).
