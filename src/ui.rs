use ratatui::{
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Cell, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame,
};
use sqlx::{Column, Row as _};

use crate::app::App;
use crate::db;

const MAX_VISIBLE_COLS: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Editor,
    Results,
    Right,
}

#[derive(Clone, Copy)]
pub enum RightView {
    List,
    Details(usize),
}

#[derive(Clone)]
pub struct MockColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Clone)]
pub struct MockIndex {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[derive(Clone)]
pub struct MockTable {
    pub name: String,
    pub columns: Vec<MockColumn>,
    pub indexes: Vec<MockIndex>,
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let [body, help] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

    let [left, right] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(body);

    let [editor_area, results_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(left);

    render_editor(frame, &*app, editor_area);
    render_results(frame, &*app, results_area);
    render_right(frame, app, right);
    render_help(frame, help);
}

fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let style = if focused {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::DarkGray)
    };
    Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(style)
        .title(title)
}

fn render_editor(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Editor;
    let block = pane_block("SQL Editor — Ctrl-R / F5 to execute", focused);
    let inner = block.inner(area);

    let paragraph = Paragraph::new(app.editor.as_str()).block(block);
    frame.render_widget(paragraph, area);

    if focused {
        let (col, row) = cursor_to_row_col(&app.editor, app.cursor);
        let x = inner.x.saturating_add(col);
        let y = inner.y.saturating_add(row);
        if x < inner.right() && y < inner.bottom() {
            frame.set_cursor_position(Position::new(x, y));
        }
    }
}

fn cursor_to_row_col(text: &str, cursor: usize) -> (u16, u16) {
    let prefix = &text[..cursor.min(text.len())];
    let mut row: u16 = 0;
    let mut col: u16 = 0;
    for c in prefix.chars() {
        if c == '\n' {
            row = row.saturating_add(1);
            col = 0;
        } else {
            col = col.saturating_add(1);
        }
    }
    (col, row)
}

fn render_results(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Results;
    let block = pane_block("Query Results", focused);

    match &app.results {
        None => {
            let p = Paragraph::new(Text::from(vec![
                Line::from(""),
                Line::styled(
                    "  (no results yet — press Ctrl-R or F5 to run the query)",
                    Style::new().fg(Color::DarkGray),
                ),
            ]))
            .block(block);
            frame.render_widget(p, area);
        }
        Some(result) => {
            if result.is_empty() {
                let p = Paragraph::new(Text::from(vec![
                    Line::from(""),
                    Line::styled(
                        "  (query executed successfully, but returned no rows)",
                        Style::new().fg(Color::DarkGray),
                    ),
                ]))
                .block(block);
                frame.render_widget(p, area);
            } else {
                let cols = result[0].columns();
                let num_cols = cols.len();
                let max_rows = area.height.saturating_sub(3) as usize;
                let col_offset = app.results_scroll_x as usize;

                let header = Row::new(cols.iter().skip(col_offset).take(MAX_VISIBLE_COLS).map(
                    |c| {
                        Cell::from(c.name().to_string())
                            .style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                    },
                ));

                let rows: Vec<Row> = result
                    .iter()
                    .skip(app.results_scroll_y as usize)
                    .take(max_rows)
                    .map(|row| {
                        Row::new(
                            (0..num_cols)
                                .skip(col_offset)
                                .take(MAX_VISIBLE_COLS)
                                .map(|i| Cell::from(db::cell_to_string(row, i))),
                        )
                    })
                    .collect();

                let n = num_cols
                    .saturating_sub(col_offset)
                    .min(MAX_VISIBLE_COLS)
                    .max(1);
                let widths: Vec<Constraint> = (0..n).map(|_| Constraint::Fill(1)).collect();
                let table = Table::new(rows, widths)
                    .header(header)
                    .block(block)
                    .highlight_symbol(" ▶ ");
                frame.render_widget(table, area);
            }
        }
    }
}

fn render_right(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Right;
    match app.right_view {
        RightView::List => {
            let items: Vec<ListItem> = app
                .tables
                .iter()
                .map(|t| ListItem::new(t.name.as_str()))
                .collect();
            let list = List::new(items)
                .block(pane_block("Tables — Press Enter to inspect", focused))
                .highlight_symbol(" > ")
                .highlight_style(
                    Style::new()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                );
            frame.render_stateful_widget(list, area, &mut app.table_list_state);
        }
        RightView::Details(i) => {
            let table = &app.tables[i];
            let title = format!("Table: {} — Press Esc to go back", table.name);
            let mut lines: Vec<Line> = Vec::new();

            lines.push(Line::styled(
                "Columns",
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
            for col in &table.columns {
                let nullable = if col.nullable { "NULL" } else { "NOT NULL" };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(col.name.as_str(), Style::new().fg(Color::Cyan)),
                    Span::raw("  "),
                    Span::styled(col.data_type.as_str(), Style::new().fg(Color::Green)),
                    Span::raw("  "),
                    Span::styled(nullable, Style::new().fg(Color::DarkGray)),
                ]));
            }
            lines.push(Line::from(""));

            lines.push(Line::styled(
                "Indexes",
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
            for idx in &table.indexes {
                let unique = if idx.unique { "UNIQUE" } else { "      " };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(idx.name.as_str(), Style::new().fg(Color::Cyan)),
                    Span::raw("  "),
                    Span::styled(unique, Style::new().fg(Color::Magenta)),
                    Span::raw("  ("),
                    Span::raw(idx.columns.join(", ")),
                    Span::raw(")"),
                ]));
            }

            let p = Paragraph::new(Text::from(lines))
                .block(pane_block(title.as_str(), focused))
                .wrap(Wrap { trim: false });
            frame.render_widget(p, area);
        }
    }
}

fn render_help(frame: &mut Frame, area: Rect) {
    let help = Paragraph::new(
        " Tab/Shift-Tab: switch pane │ Ctrl-R or F5: run │ Enter: open table │ Esc: back │ Ctrl-Q: quit",
    )
    .style(Style::new().fg(Color::DarkGray));
    frame.render_widget(help, area);
}
