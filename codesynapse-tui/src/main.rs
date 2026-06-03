mod ui;

use clap::Parser;
use codesynapse_tui::{load_graph_data, TuiApp};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "codesynapse-tui", about = "Terminal UI for codesynapse graphs")]
struct Args {
    /// Path to graph.json (default: codesynapse-out/graph.json)
    #[arg(short, long)]
    input: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let path = args
        .input
        .unwrap_or_else(|| PathBuf::from("codesynapse-out/graph.json"));

    let data = load_graph_data(&path).map_err(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
    let data = data.unwrap();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(data);
    let mut filter_mode = false;

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if filter_mode {
                    match key.code {
                        KeyCode::Esc => {
                            filter_mode = false;
                            app.set_filter(String::new());
                        }
                        KeyCode::Enter => {
                            filter_mode = false;
                        }
                        KeyCode::Backspace => {
                            let mut f = app.filter.clone();
                            f.pop();
                            app.set_filter(f);
                        }
                        KeyCode::Char(c) => {
                            let mut f = app.filter.clone();
                            f.push(c);
                            app.set_filter(f);
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            break
                        }
                        KeyCode::Tab => app.next_tab(),
                        KeyCode::BackTab => app.prev_tab(),
                        KeyCode::Down | KeyCode::Char('j') => app.scroll_down(),
                        KeyCode::Up | KeyCode::Char('k') => app.scroll_up(),
                        KeyCode::Char('/') => {
                            filter_mode = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
