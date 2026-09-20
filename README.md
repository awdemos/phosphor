# phosphor

A truecolor, self-learning terminal screensaver with an optional password-protected lock mode. Rust + ratatui, local-first, LLM-optional.

![phosphor demo](demo.gif)

## Modes

| mode | what it does |
|---|---|
| `orbital` | parallax starfield, orbital rings, nebula, block clock, telemetry ticker |
| `rain` | katakana/braille glyph rain with glitch bursts |
| `plasma` | demoscene plasma through morphing palettes |
| `pipes` | pipe walkers over a plasma floor |
| `generative` | re-themes itself from an LLM prompt (or offline hash-synthesis) |

`phosphor` (no args) runs `wander`: it rotates through modes on a learned schedule — it watches how long you linger, which palettes you keep, what you skip, and quietly re-weights what it shows you. All learning stays in `~/.local/share/phosphor/prefs.json`. `phosphor stats` shows what it learned; `phosphor forget` wipes it.

## Install

    cargo install --git https://github.com/awdemos/phosphor

Or build from source: `cargo build --release` → `target/release/phosphor`. Run it fullscreen with your terminal's fullscreen key (e.g. F11) for the full effect. Any keypress or mouse movement exits — it's a screensaver.

## Keys

    ← → n   switch mode (counts as a skip)      p   next palette
    l d     like / dislike (feeds the learner)  +/- playback speed (learned)
    q esc   exit (any other key exits too)

When locked, only Backspace and Enter work; everything else is ignored.

## Options

    --mode MODE        orbital|rain|plasma|pipes|generative (default: wander)
    --prompt "TEXT"    theme for the generative mode
    --seed N           reproducible run (used for the demo recording)
    --fps N            frame cap (default 30)
    --duration SECS    auto-exit (used for recordings)
    --no-learn         don't write preference updates
    --offline          never call an LLM
    --scripted         deterministic fast-rotation showcase

## Lock mode

    PHOSPHOR_PASSWORD=s3cret phosphor lock
    # or (less secure — visible in ps):
    phosphor lock --password s3cret

This is a terminal novelty lock, not real encryption. While locked the screensaver runs behind an overlay; exit keys and mouse are ignored, Backspace edits the password, Enter submits, and the correct password removes the overlay. Without a configured password `phosphor lock` exits with an error.

## LLM hookup (optional)

The generative mode can ask a local LLM for fresh scene specs. It speaks OpenAI-compatible chat completions and auto-detects Ollama (:11434) and llama.cpp server (:8080), or use any endpoint:

    export PHOSPHOR_LLM_URL=http://localhost:11434/v1
    export PHOSPHOR_LLM_MODEL=llama3.2
    phosphor --mode generative --prompt "deep ocean phosphorescence"

With no endpoint reachable it falls back to a deterministic synthesizer seeded from your prompt — same prompt, same scene.

## How the learning works

Every mode rotation records dwell seconds (and palette dwell, skips, likes, dislikes) to a JSON file. Selection weights are smoothed dwell with exponential vote factors, so liked modes surface more and skipped modes fade — while every mode stays reachable. Nothing leaves your machine.

## Demo

`demo.cast` is the raw asciinema recording (10s, seeded); `demo.svg` the same thing in vector form.
