use std::io;

use crossterm::{
    clipboard::CopyToClipboard,
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{self as crossterm_terminal, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{DefaultTerminal, Terminal, backend::CrosstermBackend};
use sqlx::PgPool;

use crate::{
    config::{KeyRemaps, TuiKeybindings},
    errors::AppResult,
    render::ResultGrid,
};

mod ansi;
mod edit;
mod preview;
mod render;
mod state;
mod update;

#[cfg(test)]
mod tests;

use render::render_result_grid;
use state::{GridViewState, TuiRequest};
use update::update_selected_row;

pub async fn show_result_grid(
    grid: &mut ResultGrid,
    keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
) -> AppResult<()> {
    show_result_grid_with_updates(grid, keybindings, key_remaps, None).await
}

pub async fn show_result_grid_with_updates(
    grid: &mut ResultGrid,
    keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
    pool: Option<&PgPool>,
) -> AppResult<()> {
    let mut session = TerminalSession::start()?;
    let result = run_result_grid(&mut session.terminal, grid, keybindings, key_remaps, pool).await;
    let restore_result = session.restore();

    match (result, restore_result) {
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err.into()),
        (Ok(()), Ok(())) => Ok(()),
    }
}

struct TerminalSession {
    terminal: DefaultTerminal,
    restored: bool,
}

impl TerminalSession {
    fn start() -> io::Result<Self> {
        crossterm_terminal::enable_raw_mode()?;
        if let Err(err) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = crossterm_terminal::disable_raw_mode();
            return Err(err);
        }

        let backend = CrosstermBackend::new(io::stdout());
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(err) => {
                let _ = restore_terminal();
                return Err(err);
            }
        };

        Ok(Self {
            terminal,
            restored: false,
        })
    }

    fn restore(&mut self) -> io::Result<()> {
        self.restored = true;
        restore_terminal()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if !self.restored {
            let _ = restore_terminal();
        }
    }
}

fn restore_terminal() -> io::Result<()> {
    let raw_result = crossterm_terminal::disable_raw_mode();
    let screen_result = execute!(io::stdout(), LeaveAlternateScreen);

    match (raw_result, screen_result) {
        (Err(err), _) => Err(err),
        (Ok(()), Err(err)) => Err(err),
        (Ok(()), Ok(())) => Ok(()),
    }
}

async fn run_result_grid(
    terminal: &mut DefaultTerminal,
    grid: &mut ResultGrid,
    keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
    pool: Option<&PgPool>,
) -> AppResult<()> {
    let mut state = GridViewState::new();
    state.clamp_to_grid(grid);

    loop {
        terminal.draw(|frame| render_result_grid(frame, grid, &mut state, keybindings))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        let key = if state.is_editing() {
            key_remaps.remap_text_input_key_event(key)
        } else {
            key_remaps.remap_key_event(key)
        };

        match state.handle_key(key, grid, keybindings) {
            TuiRequest::Continue => {}
            TuiRequest::Quit => break,
            TuiRequest::YankCell(value) => {
                let message = match yank_cell_to_clipboard(&value) {
                    Ok(()) => "yanked cell",
                    Err(_) => "yank failed",
                };
                state.set_toast(message);
            }
            TuiRequest::UpdateSelectedRow => {
                state.set_toast("updating row...");
                terminal.draw(|frame| render_result_grid(frame, grid, &mut state, keybindings))?;
                let result =
                    update_selected_row(grid, state.selected_row, &mut state.staged, pool).await;
                if result.updated {
                    state.invalidate_preview();
                }
                state.set_toast(result.message);
            }
        }
    }

    Ok(())
}

fn yank_cell_to_clipboard(value: &str) -> io::Result<()> {
    execute!(io::stdout(), CopyToClipboard::to_clipboard_from(value))
}
