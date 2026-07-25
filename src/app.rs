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

            // Up / Ctrl+P: previous item (wrap around)
            (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.select_prev(list_len);
                KeyAction::Continue
            }

            // Down / Ctrl+N: next item (wrap around)
            (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.select_next(list_len);
                KeyAction::Continue
            }

            _ => KeyAction::Continue,
        }
    }

    /// Move selection to the previous item, wrapping to the end.
    fn select_prev(&mut self, list_len: usize) {
        if list_len == 0 {
            self.selected_index = 0;
        } else {
            self.selected_index = if self.selected_index == 0 {
                list_len - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    /// Move selection to the next item, wrapping to the start.
    fn select_next(&mut self, list_len: usize) {
        if list_len == 0 {
            self.selected_index = 0;
        } else {
            self.selected_index = if self.selected_index >= list_len - 1 {
                0
            } else {
                self.selected_index + 1
            };
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

    /// Ctrl+N walks down the list and wraps to the top
    #[test]
    fn test_ctrl_n_moves_down_and_wraps() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);

        let ctrl_n = make_key(KeyCode::Char('n'), KeyModifiers::CONTROL);
        for expected in [1, 2, 0] {
            state.handle_key(ctrl_n, 3);
            assert_eq!(state.selected_index, expected);
        }
    }

    /// Ctrl+P walks up the list, wrapping to the bottom from the top
    #[test]
    fn test_ctrl_p_moves_up_and_wraps() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);

        let ctrl_p = make_key(KeyCode::Char('p'), KeyModifiers::CONTROL);
        for expected in [2, 1, 0] {
            state.handle_key(ctrl_p, 3);
            assert_eq!(state.selected_index, expected);
        }
    }

    /// Ctrl+N / Ctrl+P on an empty list stay at 0 (no underflow panic)
    #[test]
    fn test_ctrl_n_p_empty_list() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);

        state.handle_key(make_key(KeyCode::Char('n'), KeyModifiers::CONTROL), 0);
        assert_eq!(state.selected_index, 0);
        state.handle_key(make_key(KeyCode::Char('p'), KeyModifiers::CONTROL), 0);
        assert_eq!(state.selected_index, 0);
    }

    /// Ctrl+N / Ctrl+P must not fall through to search input
    #[test]
    fn test_ctrl_n_does_not_type_into_search() {
        let nodes = mock_nodes();
        let mut state = AppState::new(nodes);

        state.handle_key(make_key(KeyCode::Char('n'), KeyModifiers::CONTROL), 3);
        state.handle_key(make_key(KeyCode::Char('p'), KeyModifiers::CONTROL), 3);
        assert!(state.search_query.is_empty());
    }
}
