use nu_ansi_term::{Color, Style as AnsiStyle};

use crate::{
    config::{KeyRemaps, TuiKeybindings},
    errors::AppResult,
    meta,
    render::{
        DisplayMode, ResultGrid, RowsDisplay, grid_display, render_grid, render_grid_blank,
        render_row_count,
    },
    tui::{show_result_grid, show_result_grid_with_updates},
};
use sqlx::PgPool;

pub(super) async fn render_meta_output(
    output: meta::MetaOutput,
    tui_keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
    display_mode: DisplayMode,
) -> AppResult<()> {
    let mut sections = output.sections;
    if sections.len() == 1
        && let Some(section) = sections.pop()
    {
        return render_meta_section(section, tui_keybindings, key_remaps, display_mode).await;
    }

    render_meta_sections_inline(&sections);
    Ok(())
}

pub(super) async fn print_rows_display(
    display: RowsDisplay,
    tui_keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
    update_pool: Option<&PgPool>,
) -> AppResult<()> {
    match display {
        RowsDisplay::Inline(output) => println!("{output}"),
        RowsDisplay::Tui(mut grid) => {
            match update_pool {
                Some(pool) => {
                    show_result_grid_with_updates(
                        &mut grid,
                        tui_keybindings,
                        key_remaps,
                        Some(pool),
                    )
                    .await?;
                }
                None => show_result_grid(&mut grid, tui_keybindings, key_remaps).await?,
            }
            println!("{}", render_row_count(grid.row_count()));
        }
    }

    Ok(())
}

fn render_meta_sections_inline(sections: &[meta::MetaSection]) {
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("{}", render_section_title(&section.title));
        println!("{}", render_grid(&section.grid));
    }
}

fn render_section_title(title: &str) -> String {
    AnsiStyle::new()
        .bold()
        .fg(Color::Cyan)
        .paint(title)
        .to_string()
}

async fn render_meta_section(
    section: meta::MetaSection,
    tui_keybindings: &TuiKeybindings,
    key_remaps: &KeyRemaps,
    display_mode: DisplayMode,
) -> AppResult<()> {
    print_rows_display(
        grid_rows_display(section.grid, display_mode),
        tui_keybindings,
        key_remaps,
        None,
    )
    .await
}

fn grid_rows_display(grid: ResultGrid, display_mode: DisplayMode) -> RowsDisplay {
    match display_mode {
        DisplayMode::Auto => grid_display(grid),
        DisplayMode::Inline => RowsDisplay::Inline(render_grid(&grid)),
        DisplayMode::InlineBlank => RowsDisplay::Inline(render_grid_blank(&grid)),
        DisplayMode::Tui if grid.row_count() == 0 => RowsDisplay::Inline(render_row_count(0)),
        DisplayMode::Tui => RowsDisplay::Tui(grid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_title_is_rendered_as_header() {
        // Given
        let title = "Columns";

        // When
        let rendered = render_section_title(title);

        // Then
        assert_eq!(rendered, "\u{1b}[1;36mColumns\u{1b}[0m");
    }
}
