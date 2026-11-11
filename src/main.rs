//! nabiz — a minimal, calm terminal monitor for per-core CPU usage, Intel
//! core temperatures, RAM, swap and disk usage.

#![forbid(unsafe_code)]

mod bar;
mod collect;
mod cpu;
mod disk;
mod error;
#[cfg(test)]
mod fixtures;
mod format;
mod mem;
mod plain;
mod sysfs;
mod temp;
mod theme;
mod topology;
mod ui;

use std::collections::VecDeque;
use std::io::{self, IsTerminal, Stdout};
use std::panic;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use collect::Collector;
use sysfs::RealFs;
use theme::{ColorMode, Theme};

/// The first frame shows "measuring…"; take the second sample soon after.
const FIRST_SAMPLE: Duration = Duration::from_millis(250);
/// Samples of overall usage kept for the header sparkline.
const HISTORY_LEN: usize = 60;

const USAGE: &str = "\
nabiz — çekirdek başına CPU kullanımı, Intel çekirdek sıcaklıkları, RAM, swap ve diskler

Kullanım: nabiz [--once]

Seçenekler:
  --once         Tek bir düz metin anlık görüntüsü yazdırıp çık
  -h, --help     Bu yardımı göster
  -V, --version  Sürümü göster
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => match run_tui() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("nabiz: {e}");
                ExitCode::FAILURE
            }
        },
        ["--once"] => {
            run_once();
            ExitCode::SUCCESS
        }
        ["-h" | "--help"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["-V" | "--version"] => {
            println!("nabiz {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("nabiz: tanınmayan argüman: {}\n\n{USAGE}", args.join(" "));
            ExitCode::from(2)
        }
    }
}

/// Two samples half a second apart, so usage is a real measurement.
fn run_once() {
    let mut collector = Collector::new(RealFs);
    collector.collect();
    thread::sleep(Duration::from_millis(500));
    print!("{}", plain::render(&collector.collect()));
}

/// Refresh intervals selectable with `+` / `-`.
const INTERVALS_MS: [u64; 9] = [250, 500, 750, 1000, 1500, 2000, 3000, 4000, 5000];
const DEFAULT_INTERVAL: usize = 3; // 1 s

/// Restores the terminal when dropped, so every exit path (normal return,
/// `?` error, unwinding panic) leaves the terminal usable.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        // Construct the guard first: if the next step fails, raw mode is
        // still undone when it drops.
        let guard = TerminalGuard;
        execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// Best effort and idempotent: safe to call more than once.
fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
}

/// Restores the terminal *before* the default hook prints the panic
/// message, so the message lands on the normal screen and stays readable.
fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Quit,
    /// Longer refresh interval.
    Slower,
    /// Shorter refresh interval.
    Faster,
    None,
}

fn key_action(key: KeyEvent) -> Action {
    if key.kind != KeyEventKind::Press {
        return Action::None;
    }
    match key.code {
        KeyCode::Char('q' | 'Q') | KeyCode::Esc => Action::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        // `=` is `+` without Shift on most layouts.
        KeyCode::Char('+' | '=') => Action::Slower,
        KeyCode::Char('-' | '_') => Action::Faster,
        _ => Action::None,
    }
}

/// Index into [`INTERVALS_MS`] after applying `action`, clamped to range.
fn step_interval(index: usize, action: Action) -> usize {
    match action {
        Action::Slower => (index + 1).min(INTERVALS_MS.len() - 1),
        Action::Faster => index.saturating_sub(1),
        Action::Quit | Action::None => index,
    }
}

fn run_tui() -> io::Result<()> {
    if !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "stdout bir terminal değil (düz metin çıktı için --once kullanın)",
        ));
    }
    let theme = Theme::new(ColorMode::detect(
        std::env::var("NO_COLOR").ok().as_deref(),
        std::env::var("COLORTERM").ok().as_deref(),
    ));
    let mut interval_idx = DEFAULT_INTERVAL;
    let interval = |idx: usize| Duration::from_millis(INTERVALS_MS[idx]);
    let mut collector = Collector::new(RealFs);
    let mut snapshot = collector.collect();
    let mut history: VecDeque<f32> = VecDeque::with_capacity(HISTORY_LEN + 1);

    install_panic_hook();
    let _guard = TerminalGuard::enter()?;
    let mut terminal: Terminal<CrosstermBackend<Stdout>> =
        Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut last_tick = Instant::now();
    let mut next_tick = last_tick + FIRST_SAMPLE.min(interval(interval_idx));
    loop {
        let view = ui::View {
            interval: interval(interval_idx),
            history: &history,
            theme,
        };
        terminal.draw(|frame| ui::draw(frame, &snapshot, &view))?;

        // Sleep in poll() until a key, a resize, or the next tick. No busy
        // loop: the process is idle between ticks.
        let timeout = next_tick.saturating_duration_since(Instant::now());
        if event::poll(timeout)? {
            // Resize and other events need nothing but the redraw above.
            if let Event::Key(key) = event::read()? {
                match key_action(key) {
                    Action::Quit => return Ok(()),
                    Action::None => {}
                    change => {
                        interval_idx = step_interval(interval_idx, change);
                        // Reschedule relative to the last sample, so a
                        // shorter interval takes effect immediately; never
                        // delay the pending first measurement.
                        let rescheduled = last_tick + interval(interval_idx);
                        next_tick = if snapshot.overall.is_none() {
                            next_tick.min(rescheduled)
                        } else {
                            rescheduled
                        };
                    }
                }
            }
        }

        let now = Instant::now();
        if now >= next_tick {
            snapshot = collector.collect();
            if let Some(u) = snapshot.overall {
                if history.len() == HISTORY_LEN {
                    history.pop_front();
                }
                history.push_back(u);
            }
            last_tick = now;
            next_tick = now + interval(interval_idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn quit_keys() {
        assert_eq!(
            key_action(key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Action::Quit
        );
        assert_eq!(
            key_action(key(KeyCode::Esc, KeyModifiers::NONE)),
            Action::Quit
        );
        assert_eq!(
            key_action(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Action::Quit
        );
        assert_eq!(
            key_action(key(KeyCode::Char('c'), KeyModifiers::NONE)),
            Action::None
        );
    }

    #[test]
    fn interval_keys_and_release_events() {
        assert_eq!(
            key_action(key(KeyCode::Char('+'), KeyModifiers::SHIFT)),
            Action::Slower
        );
        assert_eq!(
            key_action(key(KeyCode::Char('='), KeyModifiers::NONE)),
            Action::Slower
        );
        assert_eq!(
            key_action(key(KeyCode::Char('-'), KeyModifiers::NONE)),
            Action::Faster
        );
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
            ..key(KeyCode::Char('q'), KeyModifiers::NONE)
        };
        assert_eq!(key_action(release), Action::None);
    }

    #[test]
    fn interval_range_is_250ms_to_5s() {
        assert_eq!(INTERVALS_MS[DEFAULT_INTERVAL], 1000);
        assert_eq!(INTERVALS_MS[0], 250);
        assert_eq!(INTERVALS_MS[INTERVALS_MS.len() - 1], 5000);
        assert!(INTERVALS_MS.is_sorted());
        let mut i = DEFAULT_INTERVAL;
        for _ in 0..20 {
            i = step_interval(i, Action::Slower);
        }
        assert_eq!(INTERVALS_MS[i], 5000);
        for _ in 0..20 {
            i = step_interval(i, Action::Faster);
        }
        assert_eq!(INTERVALS_MS[i], 250);
        assert_eq!(step_interval(4, Action::None), 4);
    }
}
