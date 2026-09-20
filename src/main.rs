mod app;
mod clock;
mod engine;
mod generative;
mod glyph;
mod llm;
mod lock;
mod modes;
mod palette;
mod prefs;

use app::{App, Config, Input};
use clap::{Parser, Subcommand};
#[cfg(test)]
use crossterm::event::KeyEventState;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};

#[derive(Parser, Debug)]
#[command(
    name = "phosphor",
    about = "a truecolor, self-learning terminal screensaver",
    version
)]
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

    /// Password for lock mode. Prefer env PHOSPHOR_PASSWORD instead.
    #[arg(long)]
    password: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Print learned preference report.
    Stats,
    /// Erase learned preferences.
    Forget,
    /// Run locked from the start.
    Lock,
}

fn map_key_unlocked(key: KeyEvent) -> Input {
    match key.code {
        KeyCode::Right | KeyCode::Char('n') | KeyCode::Char('N') => Input::Next,
        KeyCode::Left => Input::Prev,
        KeyCode::Char('l') | KeyCode::Char('L') => Input::Like,
        KeyCode::Char('d') | KeyCode::Char('D') => Input::Dislike,
        KeyCode::Char('+') | KeyCode::Char('=') => Input::Faster,
        KeyCode::Char('-') => Input::Slower,
        KeyCode::Char('p') | KeyCode::Char('P') => Input::NextPalette,
        KeyCode::Char(' ') => Input::Pause,
        _ => Input::Quit,
    }
}

fn map_key_locked(key: KeyEvent) -> Input {
    match key.code {
        KeyCode::Char(c) => Input::Char(c),
        KeyCode::Backspace => Input::Backspace,
        KeyCode::Enter => Input::Submit,
        _ => Input::Char('\0'),
    }
}

fn config_from(cli: &Cli) -> Config {
    let locked = matches!(cli.command, Some(Commands::Lock)) || cli.password.is_some();
    let password = cli
        .password
        .clone()
        .or_else(|| {
            std::env::var("PHOSPHOR_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .map(lock::Secret::new);
    Config {
        mode: cli.mode.clone(),
        prompt: cli.prompt.clone(),
        seed: cli.seed,
        fps: cli.fps,
        duration: cli.duration,
        learn: !cli.no_learn,
        offline: cli.offline,
        scripted: cli.scripted,
        locked,
        password,
    }
}

fn restore_terminal(stdout: &mut Stdout) {
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(
        stdout,
        LeaveAlternateScreen,
        crossterm::cursor::Show,
        crossterm::event::DisableMouseCapture
    );
}

fn run(config: Config) -> io::Result<()> {
    if config.locked && config.password.is_none() {
        eprintln!(
            "phosphor lock: no password configured. Set PHOSPHOR_PASSWORD or use --password."
        );
        std::process::exit(1);
    }

    let mut app = App::new(config.clone());
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        &mut stdout,
        EnterAlternateScreen,
        crossterm::cursor::Hide,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut stdout = io::stdout();
        restore_terminal(&mut stdout);
        default_hook(info);
    }));

    let frame_budget =
        std::time::Duration::from_secs_f64((1.0 / config.fps.max(1.0)).clamp(1.0 / 240.0, 1.0));
    let mut last = std::time::Instant::now();
    let result = loop {
        let now = std::time::Instant::now();
        let dt = now.duration_since(last).as_secs_f64();
        last = now;

        let mut inputs = Vec::new();
        while event::poll(std::time::Duration::from_millis(0))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    inputs.push(if app.locked() {
                        map_key_locked(key)
                    } else {
                        map_key_unlocked(key)
                    });
                }
                Event::Resize(w, h) => app.resize(w, h),
                Event::Mouse(_) if !app.locked() => {
                    inputs.push(Input::Quit);
                }
                _ => {}
            }
        }
        if let Ok((w, h)) = crossterm::terminal::size() {
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

    let mut stdout_end = io::stdout();
    restore_terminal(&mut stdout_end);
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
            let app = App::new(Config {
                learn: false,
                ..Config::default()
            });
            if app.prefs().samples == 0 {
                println!("no learned preferences yet — run phosphor for a while");
            } else {
                println!("{}", app.stats_report());
            }
            Ok(())
        }
        _ => run(config_from(&cli)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_run() {
        let cli = Cli::try_parse_from(["phosphor"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.no_learn);
    }

    #[test]
    fn parses_mode_and_flags() {
        let cli = Cli::try_parse_from([
            "phosphor",
            "--mode",
            "rain",
            "--seed",
            "9",
            "--no-learn",
            "--offline",
            "--scripted",
            "--duration",
            "5",
        ])
        .unwrap();
        assert_eq!(cli.mode.as_deref(), Some("rain"));
        assert!(cli.no_learn);
        assert!(cli.offline);
        assert!(cli.scripted);
        assert_eq!(cli.duration, Some(5.0));
        assert_eq!(cli.seed, 9);
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            Cli::try_parse_from(["phosphor", "stats"]).unwrap().command,
            Some(Commands::Stats)
        ));
        assert!(matches!(
            Cli::try_parse_from(["phosphor", "forget"]).unwrap().command,
            Some(Commands::Forget)
        ));
        assert!(matches!(
            Cli::try_parse_from(["phosphor", "lock"]).unwrap().command,
            Some(Commands::Lock)
        ));
    }

    #[test]
    fn key_mapping_unlocked() {
        let k = |code: KeyCode| KeyEvent {
            code,
            modifiers: crossterm::event::KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(map_key_unlocked(k(KeyCode::Right)), Input::Next);
        assert_eq!(map_key_unlocked(k(KeyCode::Char('n'))), Input::Next);
        assert_eq!(map_key_unlocked(k(KeyCode::Left)), Input::Prev);
        assert_eq!(map_key_unlocked(k(KeyCode::Char('l'))), Input::Like);
        assert_eq!(map_key_unlocked(k(KeyCode::Char('d'))), Input::Dislike);
        assert_eq!(map_key_unlocked(k(KeyCode::Char('+'))), Input::Faster);
        assert_eq!(map_key_unlocked(k(KeyCode::Char('-'))), Input::Slower);
        assert_eq!(map_key_unlocked(k(KeyCode::Char('p'))), Input::NextPalette);
        assert_eq!(map_key_unlocked(k(KeyCode::Esc)), Input::Quit);
        assert_eq!(map_key_unlocked(k(KeyCode::Char('x'))), Input::Quit);
    }

    #[test]
    fn key_mapping_locked() {
        let k = |code: KeyCode| KeyEvent {
            code,
            modifiers: crossterm::event::KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(map_key_locked(k(KeyCode::Char('a'))), Input::Char('a'));
        assert_eq!(map_key_locked(k(KeyCode::Backspace)), Input::Backspace);
        assert_eq!(map_key_locked(k(KeyCode::Enter)), Input::Submit);
        assert_eq!(map_key_locked(k(KeyCode::Esc)), Input::Char('\0'));
    }

    #[test]
    fn config_reads_env_password() {
        unsafe {
            std::env::set_var("PHOSPHOR_PASSWORD", "hunter2");
        }
        let cli = Cli::try_parse_from(["phosphor", "lock"]).unwrap();
        let cfg = config_from(&cli);
        assert!(cfg.locked);
        assert!(cfg.password.is_some());
        unsafe {
            std::env::remove_var("PHOSPHOR_PASSWORD");
        }
    }
}
