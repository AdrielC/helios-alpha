//! WebAssembly TUI for [`helio_backtest`] using [Ratzilla](https://github.com/ratatui/ratzilla).
//!
//! From `rust/crates/helio_backtest_wasm/`: `trunk serve` (needs `wasm32-unknown-unknown` + [trunk](https://github.com/trunk-rs/trunk)).

use std::{cell::RefCell, io, rc::Rc};

use helio_backtest::{BacktestHarness, FixedClock};
use ratzilla::ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};
use ratzilla::{event::KeyCode, DomBackend, WebRenderer};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    if let Err(e) = run_app() {
        ratzilla::web_sys::console::error_1(&format!("helio_backtest_wasm: {e}").into());
    }
}

fn run_app() -> io::Result<()> {
    let use_wall = Rc::new(RefCell::new(false));
    let fixed_anchor = Rc::new(RefCell::new(1_700_000_000i64));
    let log = Rc::new(RefCell::new(vec![
        "helio_backtest wasm".to_string(),
        "Space = run harness".to_string(),
        "w = toggle WallClock".to_string(),
        "f = bump fixed anchor".to_string(),
    ]));

    let backend = DomBackend::new()?;
    let terminal = Terminal::new(backend)?;

    let use_wall_k = use_wall.clone();
    let fixed_k = fixed_anchor.clone();
    let log_k = log.clone();
    terminal.on_key_event(move |key_event| {
        match key_event.code {
            KeyCode::Char(' ') => {
                let spec = helio_backtest::demo_run_spec();
                let report = if *use_wall_k.borrow() {
                    BacktestHarness::wall().run(&spec)
                } else {
                    BacktestHarness::new(FixedClock(*fixed_k.borrow())).run(&spec)
                };
                let mut l = log_k.borrow_mut();
                match report {
                    Ok(r) => l.push(format!(
                        "fp={}.. bars={} pnl={:.4}",
                        &r.fingerprint_hex[..12.min(r.fingerprint_hex.len())],
                        r.bars_processed,
                        r.pnl_simple
                    )),
                    Err(e) => l.push(format!("err: {e}")),
                }
            }
            KeyCode::Char('w') => {
                let mut u = use_wall_k.borrow_mut();
                *u = !*u;
                log_k.borrow_mut().push(if *u {
                    "clock: wall".into()
                } else {
                    "clock: fixed".into()
                });
            }
            KeyCode::Char('f') => {
                if !*use_wall_k.borrow() {
                    *fixed_k.borrow_mut() += 86_400;
                    log_k
                        .borrow_mut()
                        .push(format!("anchor {}", *fixed_k.borrow()));
                }
            }
            _ => {}
        }
    });

    let use_wall_d = use_wall.clone();
    let log_d = log.clone();

    terminal.draw_web(move |frame| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(6),
                Constraint::Length(8),
            ])
            .split(frame.area());

        let title = if *use_wall_d.borrow() {
            " helio_backtest | wasm | WallClock "
        } else {
            " helio_backtest | wasm | FixedClock "
        };
        frame.render_widget(
            Paragraph::new(title)
                .alignment(Alignment::Center)
                .style(
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                )
                .block(Block::default().borders(Borders::BOTTOM)),
            chunks[0],
        );

        let items: Vec<ListItem> = {
            let log_b = log_d.borrow();
            log_b
                .iter()
                .rev()
                .take(chunks[1].height as usize)
                .rev()
                .map(|s| ListItem::new(s.clone()))
                .collect()
        };
        frame.render_widget(
            List::new(items).block(Block::default().borders(Borders::ALL).title(" log ")),
            chunks[1],
        );

        frame.render_widget(
            Paragraph::new("Space run | w wall | f bump | Ratzilla + helio_backtest")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL).title(" help ")),
            chunks[2],
        );
    });

    Ok(())
}
