use crate::models::{Action, AppState, CategoryTab, InputMode, KeyAction, Keybindings};

impl AppState {
    pub fn new(
        nodes: Vec<crate::models::NavigationNode>,
        keybindings: Keybindings,
        tabs: Vec<CategoryTab>,
    ) -> Self {
        AppState {
            nodes,
            keybindings,
            // Start on the first configured tab (run_inner re-applies the
            // persisted / `--view` category afterwards).
            current_category: tabs.first().copied().unwrap_or(CategoryTab::Workspaces),
            tabs,
            search_query: String::new(),
            input_mode: InputMode::Navigation,
            selected_index: 0,
            spinner_tick: 0,
            theme_name: None,
            theme_overrides: crate::theme::ThemeOverrides::default(),
            cache_key: None,
            cached_displayed: std::rc::Rc::new(Vec::new()),
            cached_total: 0,
            others: std::collections::HashMap::new(),
            contents: std::collections::HashMap::new(),
        }
    }

    /// Step to the next (`step` = 1) or previous (`step` = -1) configured tab,
    /// wrapping. Hidden tabs are skipped; a current tab outside the configured
    /// list (e.g. persisted from an older config) falls back to the first.
    fn cycle_category(&mut self, step: isize) {
        let len = self.tabs.len();
        if len == 0 {
            return;
        }
        let idx = self
            .tabs
            .iter()
            .position(|t| *t == self.current_category)
            .unwrap_or(0);
        self.current_category = self.tabs[(idx as isize + step).rem_euclid(len as isize) as usize];
    }

    /// Process a crossterm key event. `list_len` is the length of the filtered
    /// display list, used for wrapping Up/Down navigation.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent, list_len: usize) -> KeyAction {
        use crossterm::event::{KeyCode, KeyModifiers};

        let action = self.keybindings.action_for(&key);

        // Printable input wins over configurable printable movement bindings
        // while filtering; otherwise keys such as j/k could not be searched.
        if self.input_mode == InputMode::Filtering
            && let KeyCode::Char(c) = key.code
            && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            self.search_query.push(c);
            self.selected_index = 0;
            return KeyAction::Continue;
        }

        match action {
            Some(Action::Dismiss) if self.input_mode == InputMode::Filtering => {
                self.input_mode = InputMode::Navigation;
                KeyAction::Continue
            }
            Some(Action::Dismiss) => KeyAction::ExitDismiss,
            None if key.code == KeyCode::Esc && self.input_mode == InputMode::Filtering => {
                self.input_mode = InputMode::Navigation;
                KeyAction::Continue
            }
            None if key.code == KeyCode::Esc => KeyAction::ExitDismiss,
            Some(Action::ForceQuit) => KeyAction::ExitDismiss,
            Some(Action::Select) => KeyAction::ExitSelect,
            Some(Action::NextCategory) => {
                self.cycle_category(1);
                self.selected_index = 0;
                KeyAction::Continue
            }
            Some(Action::PreviousCategory) => {
                self.cycle_category(-1);
                self.selected_index = 0;
                KeyAction::Continue
            }
            Some(Action::Backspace) if self.input_mode == InputMode::Filtering => {
                self.search_query.pop();
                self.selected_index = 0;
                KeyAction::Continue
            }
            Some(Action::Backspace) => KeyAction::Continue,
            Some(Action::MoveUp) => {
                self.select_prev(list_len);
                KeyAction::Continue
            }
            Some(Action::MoveDown) => {
                self.select_next(list_len);
                KeyAction::Continue
            }
            None => {
                match (self.input_mode, key.code, key.modifiers) {
                    (InputMode::Navigation, KeyCode::Char('/'), KeyModifiers::NONE) => {
                        self.input_mode = InputMode::Filtering;
                    }
                    (InputMode::Navigation, KeyCode::Char('j'), KeyModifiers::NONE) => {
                        self.select_next(list_len);
                    }
                    (InputMode::Navigation, KeyCode::Char('k'), KeyModifiers::NONE) => {
                        self.select_prev(list_len);
                    }
                    _ => {}
                }
                KeyAction::Continue
            }
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
    use crate::models::{CategoryTab, InputMode, KeyAction, Keybindings};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn make_state() -> AppState {
        AppState::new(
            mock_nodes(),
            Keybindings::default(),
            CategoryTab::all().to_vec(),
        )
    }

    #[test]
    fn test_filter_mode_transitions_and_input_precedence() {
        let mut state = make_state();
        assert_eq!(state.input_mode, InputMode::Navigation);

        state.search_query = "kept".into();
        state.selected_index = 2;
        assert_eq!(
            state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 4),
            KeyAction::Continue
        );
        assert_eq!(state.input_mode, InputMode::Filtering);
        assert_eq!(state.search_query, "kept");

        for c in ['j', 'k', '/'] {
            state.handle_key(make_key(KeyCode::Char(c), KeyModifiers::NONE), 4);
        }
        assert_eq!(state.search_query, "keptjk/");
        assert_eq!(state.selected_index, 0);

        state.selected_index = 2;
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 4),
            KeyAction::Continue
        );
        assert_eq!(state.input_mode, InputMode::Navigation);
        assert_eq!(state.search_query, "keptjk/");
        assert_eq!(state.selected_index, 2);
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 4),
            KeyAction::ExitDismiss
        );
    }

    #[test]
    fn test_navigation_mode_ignores_printable_keys_and_backspace() {
        let mut state = make_state();
        state.search_query = "kept".into();

        for c in ['j', 'k', 'x'] {
            state.handle_key(make_key(KeyCode::Char(c), KeyModifiers::NONE), 4);
        }
        state.handle_key(make_key(KeyCode::Backspace, KeyModifiers::NONE), 4);

        assert_eq!(state.search_query, "kept");
        assert_eq!(state.input_mode, InputMode::Navigation);
        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 4);
        assert_eq!(state.input_mode, InputMode::Filtering);
        assert_eq!(state.search_query, "kept");
    }

    /// Test E: Tab cycles categories correctly
    #[test]
    fn test_tab_cycles_categories() {
        let mut state = make_state();

        assert_eq!(state.current_category, CategoryTab::Workspaces);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Tabs);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Panes);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Agents);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::All);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Workspaces);
    }

    /// Shift+Tab goes backwards
    #[test]
    fn test_shift_tab_goes_backwards() {
        let mut state = make_state();

        state.handle_key(make_key(KeyCode::BackTab, KeyModifiers::SHIFT), 10);
        assert_eq!(state.current_category, CategoryTab::All);

        state.handle_key(make_key(KeyCode::BackTab, KeyModifiers::SHIFT), 10);
        assert_eq!(state.current_category, CategoryTab::Agents);
    }

    /// With a custom tab list, cycling follows the configured order and
    /// never lands on a hidden tab.
    #[test]
    fn test_custom_tabs_cycle_and_skip_hidden() {
        let mut state = AppState::new(
            mock_nodes(),
            Keybindings::default(),
            vec![CategoryTab::All, CategoryTab::Workspaces],
        );
        // Starts on the first configured tab, not the global default.
        assert_eq!(state.current_category, CategoryTab::All);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Workspaces);
        // Wraps within the configured list — Panes/Tabs/Agents are skipped.
        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::All);
        state.handle_key(make_key(KeyCode::BackTab, KeyModifiers::SHIFT), 10);
        assert_eq!(state.current_category, CategoryTab::Workspaces);
    }

    /// A single configured tab cycles to itself; a current category outside
    /// the configured list falls back into it instead of getting stuck.
    #[test]
    fn test_single_tab_and_out_of_list_current() {
        let mut state = AppState::new(mock_nodes(), Keybindings::default(), vec![CategoryTab::All]);
        state.current_category = CategoryTab::Panes; // e.g. persisted earlier
        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::All);
        state.handle_key(make_key(KeyCode::BackTab, KeyModifiers::SHIFT), 10);
        assert_eq!(state.current_category, CategoryTab::All);
    }

    /// Number keys should append to search query (not quick-select)
    #[test]
    fn test_number_keys_append_to_search() {
        let mut state = make_state();
        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 10);

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
        let mut state = make_state();
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 10),
            KeyAction::ExitDismiss
        );
    }

    /// Ctrl+C dismisses (no focus)
    #[test]
    fn test_ctrl_c_dismisses() {
        let mut state = make_state();
        assert_eq!(
            state.handle_key(make_key(KeyCode::Char('c'), KeyModifiers::CONTROL), 10),
            KeyAction::ExitDismiss
        );
    }

    /// Enter selects and focuses
    #[test]
    fn test_enter_selects() {
        let mut state = make_state();
        assert_eq!(
            state.handle_key(make_key(KeyCode::Enter, KeyModifiers::NONE), 10),
            KeyAction::ExitSelect
        );
    }

    /// Backspace modifies search query
    #[test]
    fn test_backspace_modifies_search() {
        let mut state = make_state();
        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 10);
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
        let mut state = make_state();

        state.selected_index = 3;
        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(
            state.selected_index, 0,
            "Tab should reset selected_index to 0"
        );
    }

    /// Esc in navigation dismisses without clearing the saved query.
    #[test]
    fn test_esc_in_navigation_dismisses_and_preserves_query() {
        let mut state = make_state();
        state.search_query = "saved".into();
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 10),
            KeyAction::ExitDismiss
        );
        assert_eq!(state.search_query, "saved");
    }

    #[test]
    fn test_filter_mode_preserves_navigation_and_control_bindings() {
        let mut state = make_state();
        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 4);

        state.handle_key(make_key(KeyCode::Down, KeyModifiers::NONE), 4);
        assert_eq!(state.selected_index, 1);
        state.handle_key(make_key(KeyCode::Char('n'), KeyModifiers::CONTROL), 4);
        assert_eq!(state.selected_index, 2);
        assert!(state.search_query.is_empty());

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 4);
        assert_eq!(state.current_category, CategoryTab::Tabs);
        assert!(state.search_query.is_empty());
        assert_eq!(
            state.handle_key(make_key(KeyCode::Enter, KeyModifiers::NONE), 4),
            KeyAction::ExitSelect
        );
    }

    #[test]
    fn test_ctrl_c_dismisses_while_filtering() {
        let mut state = make_state();
        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 4);
        state.handle_key(make_key(KeyCode::Char('x'), KeyModifiers::NONE), 4);

        assert_eq!(
            state.handle_key(make_key(KeyCode::Char('c'), KeyModifiers::CONTROL), 4),
            KeyAction::ExitDismiss
        );
        assert_eq!(state.search_query, "x");
    }

    #[test]
    fn test_custom_printable_movement_binding_respects_mode_precedence() {
        let mut keybindings = Keybindings::default();
        keybindings.move_down = vec!["j".into()];
        let mut state = AppState::new(mock_nodes(), keybindings, CategoryTab::all().to_vec());

        state.handle_key(make_key(KeyCode::Char('j'), KeyModifiers::NONE), 4);
        assert_eq!(state.selected_index, 1);
        assert!(state.search_query.is_empty());

        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 4);
        state.handle_key(make_key(KeyCode::Char('j'), KeyModifiers::NONE), 4);
        assert_eq!(state.selected_index, 0);
        assert_eq!(state.search_query, "j");
    }

    #[test]
    fn test_vim_navigation_wraps_and_handles_empty_lists() {
        let mut state = make_state();
        state.selected_index = 2;
        state.handle_key(make_key(KeyCode::Char('j'), KeyModifiers::NONE), 3);
        assert_eq!(state.selected_index, 0);
        state.handle_key(make_key(KeyCode::Char('k'), KeyModifiers::NONE), 3);
        assert_eq!(state.selected_index, 2);

        state.handle_key(make_key(KeyCode::Char('j'), KeyModifiers::NONE), 0);
        assert_eq!(state.selected_index, 0);
        state.handle_key(make_key(KeyCode::Char('k'), KeyModifiers::NONE), 0);
        assert_eq!(state.selected_index, 0);
        assert!(state.search_query.is_empty());
        assert_eq!(state.input_mode, InputMode::Navigation);
    }

    #[test]
    fn test_esc_preserves_cached_results_and_selection() {
        use crate::models::{AgentStatus, DisplayItem};
        use std::rc::Rc;

        let mut state = make_state();
        state.search_query = "kept".into();
        state.selected_index = 1;
        let cached = Rc::new(vec![DisplayItem::Workspace {
            name: "workspace".into(),
            id: "workspace-id".into(),
            pane_ids: vec![],
            agent_statuses: vec![AgentStatus::None],
            last_accessed_at: 1,
        }]);
        state.cached_displayed = cached.clone();
        state.cached_total = 7;
        state.cache_key = Some(42);

        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 3);
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 3),
            KeyAction::Continue
        );
        assert_eq!(state.search_query, "kept");
        assert_eq!(state.selected_index, 1);
        assert!(Rc::ptr_eq(&state.cached_displayed, &cached));
        assert_eq!(state.cached_total, 7);
        assert_eq!(state.cache_key, Some(42));
    }

    /// Ctrl+N walks down the list and wraps to the top
    #[test]
    fn test_ctrl_n_moves_down_and_wraps() {
        let mut state = make_state();

        let ctrl_n = make_key(KeyCode::Char('n'), KeyModifiers::CONTROL);
        for expected in [1, 2, 0] {
            state.handle_key(ctrl_n, 3);
            assert_eq!(state.selected_index, expected);
        }
    }

    /// Ctrl+P walks up the list, wrapping to the bottom from the top
    #[test]
    fn test_ctrl_p_moves_up_and_wraps() {
        let mut state = make_state();

        let ctrl_p = make_key(KeyCode::Char('p'), KeyModifiers::CONTROL);
        for expected in [2, 1, 0] {
            state.handle_key(ctrl_p, 3);
            assert_eq!(state.selected_index, expected);
        }
    }

    /// Ctrl+N / Ctrl+P on an empty list stay at 0 (no underflow panic)
    #[test]
    fn test_ctrl_n_p_empty_list() {
        let mut state = make_state();

        state.handle_key(make_key(KeyCode::Char('n'), KeyModifiers::CONTROL), 0);
        assert_eq!(state.selected_index, 0);
        state.handle_key(make_key(KeyCode::Char('p'), KeyModifiers::CONTROL), 0);
        assert_eq!(state.selected_index, 0);
    }

    /// Ctrl+N / Ctrl+P must not fall through to search input
    #[test]
    fn test_ctrl_n_does_not_type_into_search() {
        let mut state = make_state();

        state.handle_key(make_key(KeyCode::Char('n'), KeyModifiers::CONTROL), 3);
        state.handle_key(make_key(KeyCode::Char('p'), KeyModifiers::CONTROL), 3);
        assert!(state.search_query.is_empty());
    }
}
