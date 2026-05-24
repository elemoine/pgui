use crate::event::{AppEvent, Event, EventHandler};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::ListState;
use ratatui::DefaultTerminal;
use sqlx::postgres::{PgConnection, PgRow};
use sqlx::Connection as _;

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

/// Application.
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// Event handler.
    pub events: EventHandler,
    /// Current focus pane
    pub focus: Focus,
    /// SQL editor content
    pub editor: String,
    /// Cursor position in editor
    pub cursor: usize,
    /// Query results
    pub results: Option<Vec<PgRow>>,
    /// Scroll position in results
    pub results_scroll: u16,
    /// Available tables
    pub tables: Vec<MockTable>,
    /// Table list selection state
    pub table_list_state: ListState,
    /// Right pane view mode
    pub right_view: RightView,
}

impl Default for App {
    fn default() -> Self {
        let tables = mock_tables();
        let mut table_list_state = ListState::default();
        if !tables.is_empty() {
            table_list_state.select(Some(0));
        }
        let editor = String::from("SELECT id, email, created_at\nFROM users\nWHERE id = 1;");
        let cursor = editor.len();
        Self {
            running: true,
            events: EventHandler::new(),
            focus: Focus::Editor,
            editor,
            cursor,
            results: None,
            results_scroll: 0,
            tables,
            table_list_state,
            right_view: RightView::List,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        while self.running {
            terminal.draw(|frame| {
                crate::ui::render(frame, &mut self);
            })?;
            match self.events.next().await? {
                Event::Tick => self.tick(),
                Event::Crossterm(event) => match event {
                    crossterm::event::Event::Key(key_event)
                        if key_event.kind == KeyEventKind::Press =>
                    {
                        self.handle_key_events(key_event)?
                    }
                    _ => {}
                },
                Event::App(app_event) => match app_event {
                    AppEvent::ExecuteQuery => self.execute_query().await?,
                    AppEvent::Quit => self.quit(),
                },
            }
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    pub fn handle_key_events(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        if key_event.kind != KeyEventKind::Press {
            return Ok(());
        }

        let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
        match key_event.code {
            KeyCode::Char('q') if ctrl => {
                self.events.send(AppEvent::Quit);
                return Ok(());
            }
            KeyCode::Char('c') if ctrl => {
                self.events.send(AppEvent::Quit);
                return Ok(());
            }
            KeyCode::Char('r') if ctrl => {
                self.events.send(AppEvent::ExecuteQuery);
                return Ok(());
            }
            KeyCode::F(5) => {
                self.events.send(AppEvent::ExecuteQuery);
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

    async fn execute_query(&mut self) -> color_eyre::Result<()> {
        let mut conn =
            PgConnection::connect("postgres://alma:almaalma@localhost:5432/alma_db").await?;
        let q = sqlx::query(&self.editor);
        self.results = Some(q.fetch_all(&mut conn).await?);
        self.results_scroll = 0;
        Ok(())
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
                    let mut prev = self.cursor - 1;
                    while !self.editor.is_char_boundary(prev) {
                        prev -= 1;
                    }
                    self.editor.replace_range(prev..self.cursor, "");
                    self.cursor = prev;
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    let mut prev = self.cursor - 1;
                    while !self.editor.is_char_boundary(prev) {
                        prev -= 1;
                    }
                    self.cursor = prev;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.editor.len() {
                    let mut next = self.cursor + 1;
                    while next < self.editor.len() && !self.editor.is_char_boundary(next) {
                        next += 1;
                    }
                    self.cursor = next;
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
