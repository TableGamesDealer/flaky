use crate::{ConfigState, OptionType, OptionValue, SchemaStore};

/// The high-level screen the user is on.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    /// Loading spinner while options are being fetched.
    Loading { message: String },
    /// Category list (root or a sub-category).
    CategoryList { path: Vec<String> },
    /// Option editing screen (the value editor for a specific option).
    EditOption { option_name: String },
    /// Full-text search across all options.
    Search { query: String },
    /// Confirmation dialog (save, apply, discard).
    Confirm(ConfirmKind),
    /// Output log (nixos-rebuild output).
    Log { lines: Vec<String>, title: String },
    /// Error display.
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmKind {
    Save,
    Apply,
    Discard,
    Quit,
}

/// Editing mode within an option.
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// Normal navigation — keys move the cursor.
    Navigate,
    /// Typing into a text/number field.
    Editing,
    /// Confirmation dialog active.
    Confirming,
}

/// Top-level application state, shared between the input handler and renderer.
pub struct App {
    pub schema: SchemaStore,
    pub state: ConfigState,

    /// Current screen stack (back = pop).
    pub screen_stack: Vec<Screen>,

    /// Currently selected row index in list screens.
    pub cursor: usize,

    /// Input buffer for text/number editing.
    pub input_buf: String,
    pub input_cursor: usize,

    pub mode: AppMode,

    /// Status bar message (clears after next keypress).
    pub status_msg: Option<String>,

    /// True once we should exit the event loop.
    pub should_quit: bool,

    /// Output lines from the last nixos-rebuild run.
    pub build_log: Vec<String>,
}

impl App {
    pub fn new(schema: SchemaStore, state: ConfigState) -> Self {
        Self {
            schema,
            state,
            screen_stack: vec![Screen::CategoryList { path: vec![] }],
            cursor: 0,
            input_buf: String::new(),
            input_cursor: 0,
            mode: AppMode::Navigate,
            status_msg: None,
            should_quit: false,
            build_log: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Screen helpers
    // -----------------------------------------------------------------------

    pub fn current_screen(&self) -> &Screen {
        self.screen_stack.last().expect("screen stack empty")
    }

    pub fn push_screen(&mut self, screen: Screen) {
        self.cursor = 0;
        self.screen_stack.push(screen);
    }

    pub fn pop_screen(&mut self) {
        if self.screen_stack.len() > 1 {
            self.screen_stack.pop();
            // Restore cursor to 0 on back (could save per-level in future).
            self.cursor = 0;
        }
    }

    /// Replace the topmost screen without growing the stack.
    pub fn replace_screen(&mut self, screen: Screen) {
        if let Some(last) = self.screen_stack.last_mut() {
            *last = screen;
        }
        self.cursor = 0;
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    /// Items visible on the current list screen.
    pub fn current_list_items(&self) -> Vec<ListItem> {
        match self.current_screen() {
            Screen::CategoryList { path } => self.items_for_path(path),
            Screen::Search { query } => self
                .schema
                .search(query)
                .into_iter()
                .map(|o| ListItem::Option(o.name.clone()))
                .collect(),
            _ => vec![],
        }
    }

    fn items_for_path(&self, path: &[String]) -> Vec<ListItem> {
        let prefix = path.join(".");
        let mut items: Vec<ListItem> = Vec::new();
        let mut seen_prefixes = std::collections::HashSet::new();

        for opt_name in self.schema.options.keys() {
            if !prefix.is_empty() && !opt_name.starts_with(&format!("{prefix}.")) {
                continue;
            }

            let rest = if prefix.is_empty() {
                opt_name.as_str()
            } else {
                &opt_name[prefix.len() + 1..]
            };

            let next_seg = rest.split('.').next().unwrap_or(rest);
            let is_leaf = !rest.contains('.');
            let child_path = if prefix.is_empty() {
                next_seg.to_string()
            } else {
                format!("{prefix}.{next_seg}")
            };

            if is_leaf {
                // Direct option leaf.
                if seen_prefixes.insert(opt_name.clone()) {
                    items.push(ListItem::Option(opt_name.clone()));
                }
            } else {
                // Intermediate namespace → show as category.
                if seen_prefixes.insert(child_path.clone()) {
                    items.push(ListItem::Category(child_path));
                }
            }
        }

        items
    }

    pub fn move_up(&mut self) {
        let len = self.current_list_items().len();
        if len == 0 {
            return;
        }
        if self.cursor == 0 {
            self.cursor = len - 1;
        } else {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let len = self.current_list_items().len();
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor + 1) % len;
    }

    pub fn selected_item(&self) -> Option<ListItem> {
        let items = self.current_list_items();
        items.into_iter().nth(self.cursor)
    }

    /// Enter the selected item.
    pub fn enter_selected(&mut self) {
        match self.selected_item() {
            Some(ListItem::Category(path)) => {
                let parts: Vec<String> = path.split('.').map(|s| s.to_string()).collect();
                self.push_screen(Screen::CategoryList { path: parts });
            }
            Some(ListItem::Option(name)) => {
                self.push_screen(Screen::EditOption { option_name: name });
            }
            None => {}
        }
    }

    // -----------------------------------------------------------------------
    // Editing
    // -----------------------------------------------------------------------

    /// Toggle a bool option in-place.
    pub fn toggle_bool(&mut self, name: &str) {
        let current = self.state.get(name).cloned();
        if let Some(opt) = self.schema.options.get(name) {
            let new_val = match current {
                Some(OptionValue::Bool(b)) => OptionValue::Bool(!b),
                _ => OptionValue::Bool(true),
            };
            if let Err(e) = self.state.set(opt, Some(new_val)) {
                self.status_msg = Some(format!("error: {e}"));
            }
        }
    }

    /// Cycle an enum option to the next value.
    pub fn cycle_enum(&mut self, name: &str, forward: bool) {
        use OptionType;
        let Some(opt) = self.schema.options.get(name) else {
            return;
        };
        let OptionType::Enum { values } = &opt.option_type else {
            return;
        };
        let values = values.clone();
        let current_idx = match self.state.get(name) {
            Some(OptionValue::Str(s)) => values.iter().position(|v| v == s).unwrap_or(0),
            _ => 0,
        };
        let next_idx = if forward {
            (current_idx + 1) % values.len()
        } else {
            if current_idx == 0 {
                values.len() - 1
            } else {
                current_idx - 1
            }
        };
        let new_val = OptionValue::Str(values[next_idx].clone());
        if let Err(e) = self.state.set(opt, Some(new_val)) {
            self.status_msg = Some(format!("error: {e}"));
        }
    }

    pub fn commit_input(&mut self, option_name: &str) {
        let raw = self.input_buf.clone();
        let Some(opt) = self.schema.options.get(option_name) else {
            return;
        };
        let parsed = match &opt.option_type {
            OptionType::Str | OptionType::Path => Some(OptionValue::Str(raw)),
            OptionType::Int { .. } => {
                if let Ok(n) = raw.parse::<i64>() {
                    Some(OptionValue::Int(n))
                } else {
                    self.status_msg = Some(format!("'{}' is not a valid integer", raw));
                    return;
                }
            }
            OptionType::Float => {
                if let Ok(f) = raw.parse::<f64>() {
                    Some(OptionValue::Float(f))
                } else {
                    self.status_msg = Some(format!("'{}' is not a valid number", raw));
                    return;
                }
            }
            _ => None,
        };

        if let Some(val) = parsed {
            if let Err(e) = self.state.set(opt, Some(val)) {
                self.status_msg = Some(format!("error: {e}"));
                return;
            }
        }

        self.mode = AppMode::Navigate;
        self.input_buf.clear();
        self.input_cursor = 0;
    }

    // -----------------------------------------------------------------------
    // Status
    // -----------------------------------------------------------------------

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = Some(msg.into());
    }

    pub fn clear_status(&mut self) {
        self.status_msg = None;
    }
}

/// An item in a navigation list.
#[derive(Debug, Clone, PartialEq)]
pub enum ListItem {
    /// A namespace with sub-items.
    Category(String),
    /// A leaf option.
    Option(String),
}

impl ListItem {
    pub fn display_name(&self) -> &str {
        match self {
            ListItem::Category(p) | ListItem::Option(p) => {
                p.rsplit('.').next().unwrap_or(p.as_str())
            }
        }
    }

    pub fn full_path(&self) -> &str {
        match self {
            ListItem::Category(p) | ListItem::Option(p) => p,
        }
    }
}
