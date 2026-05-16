use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, prelude::*};
use std::{
    io,
    time::{Duration, Instant},
};

use flaky::{App, ConfigState, NixOption, OptionType, OptionValue, SchemaStore};

fn main() -> Result<()> {
    // === Bootstrap with fake schema for immediate demo ===
    let mut schema = SchemaStore::from_options(vec![
        NixOption {
            name: "boot.loader.grub.enable".into(),
            description: "Whether to enable the GRUB boot loader.".into(),
            option_type: OptionType::Bool,
            default: Some(OptionValue::Bool(true)),
            example: None,
            declared: true,
            declared_in: Some("/nix/store/.../grub.nix".into()),
        },
        NixOption {
            name: "networking.hostName".into(),
            description: "The hostname of the machine.".into(),
            option_type: OptionType::Str,
            default: Some(OptionValue::Str("nixos".into())),
            example: None,
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "services.openssh.enable".into(),
            description: "Enable the OpenSSH daemon.".into(),
            option_type: OptionType::Bool,
            default: Some(OptionValue::Bool(false)),
            example: None,
            declared: true,
            declared_in: None,
        },
        NixOption {
            name: "time.timeZone".into(),
            description: "The time zone to use.".into(),
            option_type: OptionType::Str,
            default: Some(OptionValue::Str("America/Chicago".into())),
            example: None,
            declared: true,
            declared_in: None,
        },
    ]);

    let mut state = ConfigState::new();
    // Pre-load some values
    let mut initial = std::collections::HashMap::new();
    initial.insert("boot.loader.grub.enable".into(), OptionValue::Bool(true));
    initial.insert(
        "networking.hostName".into(),
        OptionValue::Str("flaky-box".into()),
    );
    state.load_current(initial);

    let mut app = App::new(schema, state);

    // === TUI Setup ===
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| flaky::render::draw(f, &app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if app.mode == flaky::AppMode::Confirming {
                                app.mode = flaky::AppMode::Navigate;
                            } else {
                                break;
                            }
                        }
                        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
                        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
                        KeyCode::Enter => {
                            if let Some(item) = app.selected_item() {
                                match item {
                                    flaky::ListItem::Category(_) => app.enter_selected(),
                                    flaky::ListItem::Option(name) => {
                                        if let Some(opt) = app.schema.options.get(&name) {
                                            if matches!(opt.option_type, OptionType::Bool) {
                                                app.toggle_bool(&name);
                                            } else {
                                                app.enter_selected();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char(' ') => {
                            if let Some(flaky::ListItem::Option(name)) = app.selected_item() {
                                app.toggle_bool(&name);
                            }
                        }
                        KeyCode::Char('u') => {
                            if let Some(name) = app.state.undo() {
                                app.set_status(format!("Undid change to {}", name));
                            }
                        }
                        KeyCode::Char('s') => {
                            // TODO: save via writer
                            app.set_status("💾 Saved to flake.nix (stub)");
                            // app.state.take_pending();
                        }
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    println!(
        "👋 Flaky session ended. Pending changes: {}",
        app.state.pending_count()
    );
    Ok(())
}
