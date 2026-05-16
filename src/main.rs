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
    // Bootstrap with sample data so we can see your render immediately
    let schema = SchemaStore::from_options(vec![
        NixOption {
            name: "boot.loader.grub.enable".into(),
            description: "Whether to enable the GRUB boot loader.".into(),
            option_type: OptionType::Bool,
            default: Some(OptionValue::Bool(true)),
            example: None,
            declared: true,
            declared_in: Some("/nixos/modules/boot/loader/grub.nix".into()),
        },
        NixOption {
            name: "networking.hostName".into(),
            description: "The hostname of the machine.".into(),
            option_type: OptionType::Str,
            default: Some(OptionValue::Str("flaky".into())),
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
            description: "The time zone used when displaying dates and times.".into(),
            option_type: OptionType::Str,
            default: Some(OptionValue::Str("America/Chicago".into())),
            example: None,
            declared: true,
            declared_in: None,
        },
    ]);

    let mut state = ConfigState::new();
    let mut app = App::new(schema, state);

    // TUI boilerplate
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| flaky::render::draw(f, &app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
                        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
                        KeyCode::Enter => app.enter_selected(),
                        KeyCode::Char(' ') => {
                            if let Some(flaky::ListItem::Option(name)) = app.selected_item() {
                                app.toggle_bool(&name);
                            }
                        }
                        KeyCode::Char('u') => { /* undo stub */ }
                        KeyCode::Char('s') => app.set_status("💾 Save stub — writer coming next"),
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

    Ok(())
}
