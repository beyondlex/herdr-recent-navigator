use crate::models::{AppState, KeyAction};

impl AppState {
    pub fn new(nodes: Vec<crate::models::NavigationNode>) -> Self {
        AppState {
            nodes,
            current_category: crate::models::CategoryTab::Workspaces,
            search_query: String::new(),
            selected_index: 0,
            spinner_tick: 0,
            theme_name: None,
            cache_key: None,
            cached_displayed: std::rc::Rc::new(Vec::new()),
            cached_total: 0,
        }
    }

    /// Process a crossterm key event. `list_len` is the length of the filtered
    /// display list, used for wrapping Up/Down navigation.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent, list_len: usize) -> KeyAction {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.code, key.modifiers) {
            // Esc: clear filter text if present, otherwise dismiss
            (KeyCode::Esc, _) => {
                if self.search_query.is_empty() {
                    KeyAction::ExitDismiss
                } else {
                    self.search_query.clear();
                    self.selected_index = 0;
                    KeyAction::Continue
                }
            }

            // Ctrl+C: exit without focusing
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => KeyAction::ExitDismiss,

            // Enter: select and focus
            (KeyCode::Enter, _) => KeyAction::ExitSelect,

            // Tab: next category
            (KeyCode::Tab, _) => {
                self.current_category = self.current_category.next();
                self.selected_index = 0;
                KeyAction::Continue
            }

            // Shift+Tab: previous category
            (KeyCode::BackTab, _) => {
                self.current_category = self.current_category.previous();
                self.selected_index = 0;
                KeyAction::Continue
            }

            // Backspace: remove last char from search
            (KeyCode::Backspace, _) => {
                self.search_query.pop();
                self.selected_index = 0;
                KeyAction::Continue
            }

            // Regular character input (only if not a control combination)
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                self.search_query.push(c);
                self.selected_index = 0;
                KeyAction::Continue
            }

            // Up/Down arrows: navigate list (wrap around)
            (KeyCode::Up, _) => {
                if list_len == 0 {
                    self.selected_index = 0;
                } else {
                    self.selected_index = if self.selected_index == 0 {
                        list_len - 1
                    } else {
                        self.selected_index - 1
                    };
                }
                KeyAction::Continue
            }
            (KeyCode::Down, _) => {
                if list_len == 0 {
                    self.selected_index = 0;
                } else {
                    self.selected_index = if self.selected_index >= list_len - 1 {
                        0
                    } else {
                        self.selected_index + 1
                    };
                }
                KeyAction::Continue
            }

            _ => KeyAction::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::mock_nodes;
    use crate::models::{CategoryTab, KeyAction};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// Test E: Tab cycles categories correctly
    #[test]
    fn test_tab_cycles_categories() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);

        assert_eq!(state.current_category, CategoryTab::Workspaces);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Tabs);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Panes);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Agents);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Workspaces);
    }

    /// Shift+Tab goes backwards
    #[test]
    fn test_shift_tab_goes_backwards() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);

        state.handle_key(make_key(KeyCode::BackTab, KeyModifiers::SHIFT), 10);
        assert_eq!(state.current_category, CategoryTab::Agents);

        state.handle_key(make_key(KeyCode::BackTab, KeyModifiers::SHIFT), 10);
        assert_eq!(state.current_category, CategoryTab::Panes);
    }

    /// Number keys should append to search query (not quick-select)
    #[test]
    fn test_number_keys_append_to_search() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);

        let action = state.handle_key(make_key(KeyCode::Char('3'), KeyModifiers::NONE), 10);
        assert_eq!(action, KeyAction::Continue, "Number key should continue");
        assert_eq!(state.search_query, "3", "Key '3' should append to search");
        assert_eq!(state.selected_index, 0, "Key '3' should reset index");

        state.handle_key(make_key(KeyCode::Char('1'), KeyModifiers::NONE), 10);
        assert_eq!(state.search_query, "31", "Subsequent '1' should append");
    }

    /// Esc dismisses (no focus)
    #[test]
    fn test_esc_dismisses() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 10),
            KeyAction::ExitDismiss
        );
    }

    /// Ctrl+C dismisses (no focus)
    #[test]
    fn test_ctrl_c_dismisses() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);
        assert_eq!(
            state.handle_key(make_key(KeyCode::Char('c'), KeyModifiers::CONTROL), 10),
            KeyAction::ExitDismiss
        );
    }

    /// Enter selects and focuses
    #[test]
    fn test_enter_selects() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);
        assert_eq!(
            state.handle_key(make_key(KeyCode::Enter, KeyModifiers::NONE), 10),
            KeyAction::ExitSelect
        );
    }

    /// Backspace modifies search query
    #[test]
    fn test_backspace_modifies_search() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);
        for c in "hello".chars() {
            state.handle_key(make_key(KeyCode::Char(c), KeyModifiers::NONE), 10);
        }
        assert_eq!(state.search_query, "hello");
        state.handle_key(make_key(KeyCode::Backspace, KeyModifiers::NONE), 10);
        state.handle_key(make_key(KeyCode::Backspace, KeyModifiers::NONE), 10);
        assert_eq!(state.search_query, "hel");
    }

    /// Tab resets selected_index to 0
    #[test]
    fn test_tab_resets_selected_index() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);

        state.selected_index = 3;
        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(
            state.selected_index, 0,
            "Tab should reset selected_index to 0"
        );
    }

    /// Esc with non-empty search should clear search (not exit)
    #[test]
    fn test_esc_clears_search() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);
        state.handle_key(make_key(KeyCode::Char('x'), KeyModifiers::NONE), 10);
        assert_eq!(state.search_query, "x");
        let action = state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 10);
        assert_eq!(action, KeyAction::Continue);
        assert!(state.search_query.is_empty());
    }
}
