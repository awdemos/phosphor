# phosphor — terminal screensaver design

Date: 2026-09-20. Status: approved by user, implemented in this repo.

## Summary

phosphor is a truecolor, multi-mode terminal screensaver written in Rust with
ratatui (crossterm backend). It is local-first: all state lives on disk, the only
network touch is an optional OpenAI-compatible LLM endpoint used by the generative
mode, with a deterministic offline fallback. It observes how long the user dwells
on each mode/palette and re-weights its auto-rotation accordingly — self-learning
preference adaptation with no telemetry.

Full-terminal mode: alternate screen + hidden cursor fills the terminal window;
OS-level fullscreen (e.g. F11) composes on top for literal fullscreen.

## Architecture

Single binary crate, modules with one clear responsibility each:

- `main.rs` — clap CLI: `run` (default), `stats`, `forget`; flags `--mode`,
  `--prompt`, `--seed`, `--fps`, `--duration`, `--no-learn`, `--offline`,
  `--scripted`.
- `app.rs` — owns the event loop: fixed-timestep updates, mode scheduler,
  crossfades, key input, terminal restore on exit/panic.
- `engine.rs` — frame clock (delta time), FPS cap, canvas helpers
  (lerp, blend, glyph ramps, sub-cell brightness).
- `palette.rs` — named truecolor palettes, linear interpolation, hue rotation.
- `glyph.rs` — glyph sets (katakana, braille, ascii, blocks) and intensity ramps.
- `clock.rs` — 3x5 block-digit font, renders HH:MM(SS).
- `prefs.rs` — learned-preference store: JSON at
  `$XDG_DATA_HOME/phosphor/prefs.json` (dirs crate), atomic write (tmp+rename),
  dwell/skips/likes per mode, palette affinity, speed multiplier. `--no-learn`
  disables writes. `stats`/`forget` subcommands read/reset it.
- `generative.rs` — `SceneSpec` (palette, density, speed, motion, glyph set),
  strict serde parse + range sanitize, hash-seeded offline synthesizer.
- `llm.rs` — OpenAI-compatible chat client; config from `PHOSPHOR_LLM_URL`,
  `PHOSPHOR_LLM_MODEL`, `PHOSPHOR_LLM_API_KEY`; autodetect Ollama (:11434) and
  llama.cpp server (:8080); 5s timeout, single retry, JSON extraction from
  fenced blocks, sanitize into range. Any failure → offline synthesizer.
- `modes/` — `Mode` trait (`update(dt, size, ctx)` / `render(frame, ctx)`),
  registry, and the five renderers:
  1. `orbital` — 3-layer parallax starfield, orbital rings + satellites,
     layered-sine nebula, block-digit clock, scrolling telemetry ticker.
  2. `rain` — glyph rain (katakana/braille/ascii), bright heads, fading tails,
     hue cycling, glitch bursts, corner clock.
  3. `plasma` — classic sine-field plasma, palette-morphing over time.
  4. `pipes` — pipe network growing over faint plasma floor.
  5. `generative` — renders any of the above parameter-driven by a SceneSpec;
     re-themes periodically (LLM if reachable, else re-seeded).

## Data flow

CLI args → App config → Scheduler picks mode (softmax over learned weights;
fixed choice with `--mode`) → Mode updates state from delta time → renders into
ratatui `Frame` → dwell time accrues to prefs on exit/rotation → prefs reweight
future rotations. `--scripted` forces a fast rotation (showcases all modes in a
short recording); `--duration` auto-exits.

## Learning model

- Signals: dwell seconds per mode (primary), palette dwell, explicit `l`/`d`
  keys, skip counts (switching away early), speed adjustments.
- Weight: `(dwell + α) / (total + n·α)` per mode with α smoothing, multiplied by
  `like^+1` / `dislike^-1` factors; palette affinity mirrors mode math.
- Scheduler samples ∝ weights, never repeating the same mode back-to-back,
  rotating on a jittered ~45s interval (`--scripted`: ~3.3s), crossfade ~1.2s
  by blending luminance of the two modes' cells through a glyph ramp.

## Keys

`←/→/n` switch mode (skip signal), `l`/`d` like/dislike, `+/-` speed, `p` next
palette, `q`/Esc/any other key exits. Mouse movement exits when mouse capture is
enabled.

## Error handling

Terminal restored via panic hook + Drop guard; SIGINT/SIGTERM handled; resize
clamps to a minimum 20x8; all persistence is atomic; LLM failures degrade to
offline synthesis; no `unwrap()` on user-facing paths.

## Testing

Unit tests per module (palette interpolation, digit font, prefs weight math and
atomic round-trip via tempdir, SceneSpec sanitize, synthesizer determinism,
scheduler no-repeat property, LLM JSON extraction). Integration tests drive the
real app on ratatui `TestBackend`: fixed-size frames, mid-run resize, min-size
guard, `--duration` auto-exit, non-blank output. Gate:
`cargo fmt --check && cargo clippy -- -D warnings && cargo test`.

## Pre-ship bug sweep

Build the `adversarial` CLI from the sibling repo `../adversarial.sh`
(`cargo build --release -p adversarial-cli`), then run
`adversarial scan --agent opencode` plus `--simulate` against this repo.
Fix all confirmed findings and re-run until clean before pushing.

## Repo + demo

Public `github.com/awdemos/phosphor`, MIT. README in the awdemos house style
(hand-written, terminal-inspired). Demo: `asciinema rec` (110x34) of
`phosphor --seed 42 --duration 10s --scripted`, rendered with `agg` to
`demo.gif` (+ `demo.svg`, `demo.cast` committed). Created and pushed with `gh`.
