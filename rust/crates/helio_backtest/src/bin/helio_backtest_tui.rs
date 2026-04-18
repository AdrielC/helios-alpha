//! Native terminal UI for the backtest harness (`--features tui`).
//! Run inside **tmux** for scrollback and detach: `tmux new -s backtest ./scripts/helio-backtest-tmux.sh`

use std::io::{self, stdout, Stdout};
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use helio_backtest::{BacktestHarness, BacktestRunSpec, FixedClock};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Terminal;

type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

fn main() -> io::Result<()> {
    let mut terminal = init_terminal()?;
    let res = run_app(&mut terminal);
    restore_terminal(&mut terminal)?;
    res
}

fn init_terminal() -> io::Result<AppTerminal> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut AppTerminal) -> io::Result<()> {
    terminal.show_cursor()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

fn run_app(terminal: &mut AppTerminal) -> io::Result<()> {
    let mut use_wall = false;
    let mut fixed_anchor = 1_700_000_000i64;
    let mut log: Vec<String> = vec![
        "helio-backtest-tui".into(),
        "r = run harness".into(),
        "w = toggle WallClock vs FixedClock".into(),
        "f = bump fixed anchor +1 day (when not wall)".into(),
        "q = quit".into(),
        "Tip: run in tmux for persistent sessions.".into(),
    ];

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(8),
                    Constraint::Length(12),
                ])
                .split(f.area());

            let title = if use_wall {
                " helio_backtest | WallClock ".to_string()
            } else {
                format!(" helio_backtest | FixedClock({fixed_anchor}) ")
            };
            let header = Paragraph::new(title).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
            f.render_widget(
                header.block(Block::default().borders(Borders::BOTTOM)),
                chunks[0],
            );

            let items: Vec<ListItem> = log
                .iter()
                .rev()
                .take(chunks[1].height as usize)
                .rev()
                .map(|s| ListItem::new(s.as_str()))
                .collect();
            let list = List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" log "),
            );
            f.render_widget(list, chunks[1]);

            let help = Paragraph::new(
                "Keys: r run | w toggle wall/fixed | f bump fixed anchor | q quit | wasm: helio_backtest_wasm + trunk",
            )
            .style(Style::default().fg(Color::DarkGray));
            f.render_widget(
                help.block(Block::default().borders(Borders::ALL).title(" help ")),
                chunks[2],
            );
        })?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('r') => {
                        let spec = demo_spec();
                        let report = if use_wall {
                            BacktestHarness::wall().run(&spec)
                        } else {
                            BacktestHarness::new(FixedClock(fixed_anchor)).run(&spec)
                        };
                        match report {
                            Ok(r) => {
                                log.push(format!(
                                    "fingerprint={}.. range=[{}, {}] bars={} pnl={:.6}",
                                    &r.fingerprint_hex[..12.min(r.fingerprint_hex.len())],
                                    r.range.start_epoch_sec,
                                    r.range.end_epoch_sec,
                                    r.bars_processed,
                                    r.pnl_simple
                                ));
                            }
                            Err(e) => log.push(format!("error: {e}")),
                        }
                    }
                    KeyCode::Char('w') => {
                        use_wall = !use_wall;
                        log.push(if use_wall {
                            "clock: WallClock".into()
                        } else {
                            format!("clock: FixedClock({fixed_anchor})")
                        });
                    }
                    KeyCode::Char('f') => {
                        if !use_wall {
                            fixed_anchor += 86_400;
                            log.push(format!("fixed anchor -> {fixed_anchor}"));
                        } else {
                            log.push("disable Wall (w) before bumping fixed anchor".into());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn demo_spec() -> BacktestRunSpec {
    helio_backtest::demo_run_spec()
}
