use crossterm::event::{
    Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use futures::StreamExt;
use ratatui::widgets::ListState;
use ratatui::DefaultTerminal;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use tokio::time::{self, Duration};

use crate::db;
use crate::ui::{Focus, RightView};

const DATABASE_URL: &str = "postgres://alma:almaalma@localhost:5432/alma_db";

/// Application.
pub struct App {
    /// Is the application running?
    running: bool,
    /// Current focus pane
    pub focus: Focus,
    /// SQL editor content
    pub editor: String,
    /// Cursor position in editor
    pub cursor: usize,
    /// Query results
    pub results: Option<Vec<PgRow>>,
    /// Vertical scroll position in results
    pub results_scroll_y: u16,
    /// Horizontal scroll position in results
    pub results_scroll_x: u16,
    /// Available tables
    pub tables: Vec<String>,
    /// Filter text applied to the table list
    pub table_filter: String,
    /// Table list selection state
    pub table_list_state: ListState,
    /// Right pane view mode
    pub right_view: RightView,
    /// Columns of the currently inspected table
    pub columns: Option<Vec<db::ColumnInfo>>,
    /// Vertical scroll position in the columns view
    pub columns_scroll: u16,
    /// Database connection pool
    db_pool: Option<PgPool>,
    /// Running query task
    query_task: Option<tokio::task::JoinHandle<color_eyre::Result<Vec<PgRow>>>>,
    /// Running refresh tables task
    refresh_tables_task: Option<tokio::task::JoinHandle<color_eyre::Result<Vec<String>>>>,
    /// Running list columns task
    list_columns_task: Option<tokio::task::JoinHandle<color_eyre::Result<Vec<db::ColumnInfo>>>>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        let tables = Vec::new();
        let table_list_state = ListState::default();
        // if !tables.is_empty() {
        //     table_list_state.select(Some(0));
        // }
        let editor = String::new();
        let cursor = 0;
        Self {
            running: true,
            focus: Focus::Editor,
            editor,
            cursor,
            results: None,
            results_scroll_y: 0,
            results_scroll_x: 0,
            tables,
            table_filter: String::new(),
            table_list_state,
            right_view: RightView::List,
            columns: None,
            columns_scroll: 0,
            db_pool: None,
            query_task: None,
            refresh_tables_task: None,
            list_columns_task: None,
        }
    }

    /// Initialize the app with a database pool.
    pub async fn with_db(mut self) -> color_eyre::Result<Self> {
        match PgPool::connect(DATABASE_URL).await {
            Ok(pool) => {
                self.db_pool = Some(pool);
            }
            Err(e) => {
                eprintln!("✗ Failed to connect to database: {}", e);
            }
        }
        Ok(self)
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        let mut event_stream = EventStream::new();

        terminal.draw(|frame| {
            crate::ui::render(frame, &mut self);
        })?;

        let mut interval = time::interval(Duration::from_millis(500));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.spawn_refresh_tables();
                    terminal.draw(|frame| {
                        crate::ui::render(frame, &mut self);
                    })?;
                }
                Some(Ok(event)) = event_stream.next() => {
                    match event {
                        CrosstermEvent::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                            self.handle_key_events(key_event)?;
                            terminal.draw(|frame| {
                                crate::ui::render(frame, &mut self);
                            })?;
                        }
                        _ => {}
                    }
                }
                query_result = async {
                    if let Some(task) = &mut self.query_task {
                        task.await
                    } else {
                        std::future::pending().await
                    }
                }, if self.query_task.is_some() => {
                    match query_result {
                        Ok(Ok(rows)) => {
                            self.results = Some(rows);
                            self.results_scroll_y = 0;
                            self.results_scroll_x = 0;
                        }
                        Ok(Err(e)) => {
                            eprintln!("✗ Query error: {}", e);
                        }
                        Err(e) => {
                            eprintln!("✗ Task error: {}", e);
                        }
                    }
                    self.query_task = None;
                    terminal.draw(|frame| {
                        crate::ui::render(frame, &mut self);
                    })?;
                }
                refresh_table_result = async {
                    if let Some(task) = &mut self.refresh_tables_task {
                        task.await
                    } else {
                        std::future::pending().await
                    }
                }, if self.refresh_tables_task.is_some() => {
                    match refresh_table_result {
                        Ok(Ok(tables)) => {
                            self.tables = tables;
                            self.clamp_table_selection();
                        }
                        Ok(Err(e)) => {
                            eprintln!("✗ Refresh tables error: {}", e);
                        }
                        Err(e) => {
                            eprintln!("✗ Task error: {}", e);
                        }
                    }
                    self.refresh_tables_task = None;
                    terminal.draw(|frame| {
                        crate::ui::render(frame, &mut self);
                    })?;
                }
                list_columns_result = async {
                    if let Some(task) = &mut self.list_columns_task {
                        task.await
                    } else {
                        std::future::pending().await
                    }
                }, if self.list_columns_task.is_some() => {
                    match list_columns_result {
                        Ok(Ok(columns)) => {
                            self.columns = Some(columns);
                        }
                        Ok(Err(e)) => {
                            eprintln!("✗ List columns error: {}", e);
                        }
                        Err(e) => {
                            eprintln!("✗ Task error: {}", e);
                        }
                    }
                    self.list_columns_task = None;
                    terminal.draw(|frame| {
                        crate::ui::render(frame, &mut self);
                    })?;
                }
            }

            if !self.running {
                break;
            }
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    pub fn handle_key_events(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
        match key_event.code {
            KeyCode::Char('q') if ctrl => {
                self.quit();
                return Ok(());
            }
            KeyCode::Char('c') if ctrl => {
                self.quit();
                return Ok(());
            }
            KeyCode::Char('r') if ctrl => {
                self.spawn_query();
                return Ok(());
            }
            KeyCode::Char('w') if ctrl => {
                self.clear_editor();
                return Ok(());
            }
            KeyCode::Tab => {
                self.cycle_focus_forward();
                return Ok(());
            }
            KeyCode::BackTab => {
                self.cycle_focus_backward();
                return Ok(());
            }
            _ => {}
        }

        match self.focus {
            Focus::Editor => self.handle_editor_key(key_event),
            Focus::Results => self.handle_results_key(key_event),
            Focus::Right => self.handle_right_key(key_event),
        }

        Ok(())
    }

    pub fn cycle_focus_forward(&mut self) {
        self.focus = match self.focus {
            Focus::Editor => Focus::Results,
            Focus::Results => Focus::Right,
            Focus::Right => Focus::Editor,
        };
    }

    pub fn cycle_focus_backward(&mut self) {
        self.focus = match self.focus {
            Focus::Editor => Focus::Right,
            Focus::Results => Focus::Editor,
            Focus::Right => Focus::Results,
        };
    }

    fn handle_editor_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.editor.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            KeyCode::Enter => {
                self.editor.insert(self.cursor, '\n');
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let prev = prev_char_boundary(&self.editor, self.cursor);
                self.editor.replace_range(prev..self.cursor, "");
                self.cursor = prev;
            }
            KeyCode::Left if self.cursor > 0 => {
                self.cursor = prev_char_boundary(&self.editor, self.cursor);
            }
            KeyCode::Right if self.cursor < self.editor.len() => {
                self.cursor = next_char_boundary(&self.editor, self.cursor);
            }
            KeyCode::Home => self.cursor = line_start(&self.editor, self.cursor),
            KeyCode::End => self.cursor = line_end(&self.editor, self.cursor),
            _ => {}
        }
    }

    fn handle_results_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.results_scroll_y = self.results_scroll_y.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.results_scroll_y = self.results_scroll_y.saturating_add(1);
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.results_scroll_x = self.results_scroll_x.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(result) = &self.results {
                    if !result.is_empty() {
                        let num_cols = result[0].columns().len();
                        let max_offset = num_cols.saturating_sub(1);
                        if self.results_scroll_x < max_offset as u16 {
                            self.results_scroll_x = self.results_scroll_x.saturating_add(1);
                        }
                    }
                }
            }
            KeyCode::PageUp => {
                self.results_scroll_y = self.results_scroll_y.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.results_scroll_y = self.results_scroll_y.saturating_add(5);
            }
            _ => {}
        }
    }

    fn handle_right_key(&mut self, key: KeyEvent) {
        match self.right_view {
            RightView::List => match key.code {
                KeyCode::Down => self.select_next_table(),
                KeyCode::Up => self.select_prev_table(),
                KeyCode::Enter => {
                    let selected = self
                        .table_list_state
                        .selected()
                        .and_then(|i| self.filtered_tables().get(i).map(|t| (*t).clone()));
                    if let Some(table) = selected {
                        self.right_view = RightView::Details(table.clone());
                        self.columns = None;
                        self.columns_scroll = 0;
                        self.spawn_list_columns(table);
                    }
                }
                KeyCode::Esc if !self.table_filter.is_empty() => {
                    self.table_filter.clear();
                    self.reset_table_selection();
                }
                KeyCode::Backspace => {
                    self.table_filter.pop();
                    self.reset_table_selection();
                }
                KeyCode::Char(c) => {
                    self.table_filter.push(c);
                    self.reset_table_selection();
                }
                _ => {}
            },
            RightView::Details(_) => match key.code {
                KeyCode::Esc | KeyCode::Backspace => {
                    self.right_view = RightView::List;
                    self.columns = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.columns_scroll = self.columns_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.columns_scroll = self.columns_scroll.saturating_add(1);
                }
                KeyCode::PageUp => {
                    self.columns_scroll = self.columns_scroll.saturating_sub(5);
                }
                KeyCode::PageDown => {
                    self.columns_scroll = self.columns_scroll.saturating_add(5);
                }
                _ => {}
            },
        }
    }

    /// Tables matching the current filter (case-insensitive substring).
    pub fn filtered_tables(&self) -> Vec<&String> {
        if self.table_filter.is_empty() {
            return self.tables.iter().collect();
        }
        let needle = self.table_filter.to_lowercase();
        self.tables
            .iter()
            .filter(|t| t.to_lowercase().contains(&needle))
            .collect()
    }

    /// Reset the selection to the first filtered table (called when the filter changes).
    fn reset_table_selection(&mut self) {
        if self.filtered_tables().is_empty() {
            self.table_list_state.select(None);
        } else {
            self.table_list_state.select(Some(0));
        }
    }

    /// Keep the selection within the bounds of the filtered list.
    fn clamp_table_selection(&mut self) {
        let len = self.filtered_tables().len();
        match self.table_list_state.selected() {
            _ if len == 0 => self.table_list_state.select(None),
            None => self.table_list_state.select(Some(0)),
            Some(i) if i >= len => self.table_list_state.select(Some(len - 1)),
            Some(_) => {}
        }
    }

    fn select_next_table(&mut self) {
        let len = self.filtered_tables().len();
        if len == 0 {
            return;
        }
        let i = match self.table_list_state.selected() {
            Some(i) => (i + 1) % len,
            None => 0,
        };
        self.table_list_state.select(Some(i));
    }

    fn select_prev_table(&mut self) {
        let len = self.filtered_tables().len();
        if len == 0 {
            return;
        }
        let i = match self.table_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_list_state.select(Some(i));
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    fn spawn_query(&mut self) {
        if let Some(pool) = &self.db_pool {
            let editor = self.editor.clone();
            let pool = pool.clone();
            self.query_task = Some(tokio::spawn(async move {
                db::execute_query(&editor, pool).await
            }));
        } else {
            eprintln!("✗ Database not connected");
        }
    }

    fn spawn_refresh_tables(&mut self) {
        if let Some(pool) = &self.db_pool {
            let pool = pool.clone();
            self.refresh_tables_task =
                Some(tokio::spawn(async move { db::list_tables(pool).await }));
        } else {
            eprintln!("✗ Database not connected");
        }
    }

    fn spawn_list_columns(&mut self, table: String) {
        if let Some(pool) = &self.db_pool {
            let pool = pool.clone();
            self.list_columns_task =
                Some(tokio::spawn(
                    async move { db::list_columns(&table, pool).await },
                ));
        } else {
            eprintln!("✗ Database not connected");
        }
    }

    fn clear_editor(&mut self) {
        self.editor.clear();
        self.cursor = 0;
    }
}

fn prev_char_boundary(text: &str, pos: usize) -> usize {
    let mut p = pos.saturating_sub(1);
    while p > 0 && !text.is_char_boundary(p) {
        p -= 1;
    }
    p
}

fn next_char_boundary(text: &str, pos: usize) -> usize {
    let mut p = pos + 1;
    while p < text.len() && !text.is_char_boundary(p) {
        p += 1;
    }
    p
}

fn line_start(text: &str, cursor: usize) -> usize {
    text[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0)
}

fn line_end(text: &str, cursor: usize) -> usize {
    match text[cursor..].find('\n') {
        Some(off) => cursor + off,
        None => text.len(),
    }
}
