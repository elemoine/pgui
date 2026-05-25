use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, Event as CrosstermEvent, EventStream};
use futures::StreamExt;
use ratatui::widgets::ListState;
use ratatui::DefaultTerminal;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};
use std::time::Duration;
use tokio::time::interval;

use crate::ui::{Focus, RightView, MockTable, MockColumn, MockIndex};
use crate::db;

const DATABASE_URL: &str = "postgres://alma:almaalma@localhost:5432/alma_db";
const TICK_INTERVAL_MS: u64 = 33;

/// Application.
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Current focus pane
    pub focus: Focus,
    /// SQL editor content
    pub editor: String,
    /// Cursor position in editor
    pub cursor: usize,
    /// Query results
    pub results: Option<Vec<PgRow>>,
    /// Vertical scroll position in results
    pub results_scroll: u16,
    /// Horizontal scroll position in results
    pub results_scroll_x: u16,
    /// Available tables
    pub tables: Vec<MockTable>,
    /// Table list selection state
    pub table_list_state: ListState,
    /// Right pane view mode
    pub right_view: RightView,
    /// Database connection pool
    pub db_pool: Option<PgPool>,
    /// Running query task
    query_task: Option<tokio::task::JoinHandle<color_eyre::Result<Vec<PgRow>>>>,
}

impl Default for App {
    fn default() -> Self {
        let tables = mock_tables();
        let mut table_list_state = ListState::default();
        if !tables.is_empty() {
            table_list_state.select(Some(0));
        }
        let editor = String::new();
        let cursor = 0;
        Self {
            running: true,
            focus: Focus::Editor,
            editor,
            cursor,
            results: None,
            results_scroll: 0,
            results_scroll_x: 0,
            tables,
            table_list_state,
            right_view: RightView::List,
            db_pool: None,
            query_task: None,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize the app with a database pool.
    pub async fn with_db(mut self) -> color_eyre::Result<Self> {
        match PgPool::connect(DATABASE_URL).await {
            Ok(pool) => {
                eprintln!("✓ Connecté à la base de données");
                self.db_pool = Some(pool);
            }
            Err(e) => {
                eprintln!("✗ Impossible de se connecter à la base de données: {}", e);
            }
        }
        Ok(self)
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        let mut event_stream = EventStream::new();
        let mut tick_interval = interval(Duration::from_millis(TICK_INTERVAL_MS));

        loop {
            tokio::select! {
                _ = tick_interval.tick() => {
                    self.tick();
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
                            self.results_scroll = 0;
                            self.results_scroll_x = 0;
                        }
                        Ok(Err(e)) => {
                            eprintln!("✗ Erreur requête: {}", e);
                        }
                        Err(e) => {
                            eprintln!("✗ Erreur tâche: {}", e);
                        }
                    }
                    self.query_task = None;
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
            KeyCode::F(5) => {
                self.spawn_query();
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
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let prev = prev_char_boundary(&self.editor, self.cursor);
                    self.editor.replace_range(prev..self.cursor, "");
                    self.cursor = prev;
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = prev_char_boundary(&self.editor, self.cursor);
                }
            }
            KeyCode::Right => {
                if self.cursor < self.editor.len() {
                    self.cursor = next_char_boundary(&self.editor, self.cursor);
                }
            }
            KeyCode::Home => self.cursor = line_start(&self.editor, self.cursor),
            KeyCode::End => self.cursor = line_end(&self.editor, self.cursor),
            _ => {}
        }
    }

    fn handle_results_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.results_scroll = self.results_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.results_scroll = self.results_scroll.saturating_add(1);
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
                self.results_scroll = self.results_scroll.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.results_scroll = self.results_scroll.saturating_add(5);
            }
            _ => {}
        }
    }

    fn handle_right_key(&mut self, key: KeyEvent) {
        match self.right_view {
            RightView::List => match key.code {
                KeyCode::Down | KeyCode::Char('j') => self.select_next_table(),
                KeyCode::Up | KeyCode::Char('k') => self.select_prev_table(),
                KeyCode::Enter => {
                    if let Some(i) = self.table_list_state.selected() {
                        self.right_view = RightView::Details(i);
                    }
                }
                _ => {}
            },
            RightView::Details(_) => match key.code {
                KeyCode::Esc | KeyCode::Backspace => self.right_view = RightView::List,
                _ => {}
            },
        }
    }

    fn select_next_table(&mut self) {
        if self.tables.is_empty() {
            return;
        }
        let i = match self.table_list_state.selected() {
            Some(i) => (i + 1) % self.tables.len(),
            None => 0,
        };
        self.table_list_state.select(Some(i));
    }

    fn select_prev_table(&mut self) {
        if self.tables.is_empty() {
            return;
        }
        let i = match self.table_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tables.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_list_state.select(Some(i));
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&self) {}

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    fn spawn_query(&mut self) {
        if let Some(pool) = &self.db_pool {
            let editor = self.editor.clone();
            let pool = pool.clone();
            eprintln!("→ Exécution de la requête...");
            self.query_task = Some(tokio::spawn(async move {
                db::execute_query(&editor, pool).await
            }));
        } else {
            eprintln!("✗ Base de données non connectée");
        }
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

fn mock_tables() -> Vec<MockTable> {
    vec![
        MockTable {
            name: "users".into(),
            columns: vec![
                MockColumn {
                    name: "id".into(),
                    data_type: "bigint".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "email".into(),
                    data_type: "text".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "created_at".into(),
                    data_type: "timestamptz".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "deleted_at".into(),
                    data_type: "timestamptz".into(),
                    nullable: true,
                },
            ],
            indexes: vec![
                MockIndex {
                    name: "users_pkey".into(),
                    columns: vec!["id".into()],
                    unique: true,
                },
                MockIndex {
                    name: "users_email_idx".into(),
                    columns: vec!["email".into()],
                    unique: true,
                },
            ],
        },
        MockTable {
            name: "orders".into(),
            columns: vec![
                MockColumn {
                    name: "id".into(),
                    data_type: "bigint".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "user_id".into(),
                    data_type: "bigint".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "amount_cents".into(),
                    data_type: "integer".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "status".into(),
                    data_type: "text".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "created_at".into(),
                    data_type: "timestamptz".into(),
                    nullable: false,
                },
            ],
            indexes: vec![
                MockIndex {
                    name: "orders_pkey".into(),
                    columns: vec!["id".into()],
                    unique: true,
                },
                MockIndex {
                    name: "orders_user_id_idx".into(),
                    columns: vec!["user_id".into()],
                    unique: false,
                },
                MockIndex {
                    name: "orders_status_idx".into(),
                    columns: vec!["status".into()],
                    unique: false,
                },
            ],
        },
        MockTable {
            name: "products".into(),
            columns: vec![
                MockColumn {
                    name: "id".into(),
                    data_type: "bigint".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "sku".into(),
                    data_type: "text".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "name".into(),
                    data_type: "text".into(),
                    nullable: false,
                },
                MockColumn {
                    name: "price_cents".into(),
                    data_type: "integer".into(),
                    nullable: false,
                },
            ],
            indexes: vec![
                MockIndex {
                    name: "products_pkey".into(),
                    columns: vec!["id".into()],
                    unique: true,
                },
                MockIndex {
                    name: "products_sku_key".into(),
                    columns: vec!["sku".into()],
                    unique: true,
                },
            ],
        },
    ]
}

