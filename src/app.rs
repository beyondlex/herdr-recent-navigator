use crate::models::{Action, AppState, CategoryTab, InputMode, KeyAction, Keybindings};

impl AppState {
    pub fn new(
        nodes: Vec<crate::models::NavigationNode>,
        keybindings: Keybindings,
        tabs: Vec<CategoryTab>,
    ) -> Self {
        // Only a user who bound movement to a printable key can be given the
        // modal filter; typing must stay free for everyone else.
        let input_mode = if keybindings.has_printable_navigation() {
            InputMode::Navigation
        } else {
            InputMode::TypeToSearch
        };
        AppState {
            nodes,
            keybindings,
            // Start on the first configured tab (run_inner re-applies the
            // persisted / `--view` category afterwards).
            current_category: tabs.first().copied().unwrap_or(CategoryTab::Workspaces),
            agent_sort: crate::models::AgentSort::Recent,
            tabs,
            search_query: String::new(),
            input_mode,
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
        if self.input_mode == InputMode::TypeToSearch {
            self.handle_key_type_to_search(key, list_len)
        } else {
            self.handle_key_modal(key, list_len)
        }
    }

    /// The default, modeless behaviour: any key nobody bound is typed straight
    /// into the query.
    fn handle_key_type_to_search(
        &mut self,
        key: crossterm::event::KeyEvent,
        list_len: usize,
    ) -> KeyAction {
        use crossterm::event::{KeyCode, KeyModifiers};

        match self.keybindings.action_for(&key) {
            Some(Action::Dismiss) => {
                if self.search_query.is_empty() {
                    KeyAction::ExitDismiss
                } else {
                    self.search_query.clear();
                    self.selected_index = 0;
                    KeyAction::Continue
                }
            }
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
            Some(Action::Backspace) => {
                self.search_query.pop();
                self.selected_index = 0;
                KeyAction::Continue
            }
            Some(Action::MoveUp) => {
                self.select_prev(list_len);
                KeyAction::Continue
            }
            Some(Action::MoveDown) => {
                self.select_next(list_len);
                KeyAction::Continue
            }
            None => {
                // Fall through: character input for search
                if let KeyCode::Char(c) = key.code
                    && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
                {
                    self.search_query.push(c);
                    self.selected_index = 0;
                }
                KeyAction::Continue
            }
        }
    }

    /// Vim-style two-mode input, only reachable when movement is bound to a
    /// printable key: `/` (or `Esc`) moves the keys between driving the list and
    /// editing the query. Outside the filter `j`/`k` move and `h`/`l` switch tabs.
    fn handle_key_modal(&mut self, key: crossterm::event::KeyEvent, list_len: usize) -> KeyAction {
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
            Some(Action::Dismiss) => self.back_out(),
            None if key.code == KeyCode::Esc => self.back_out(),
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
            Some(Action::Backspace) => {
                self.search_query.pop();
                self.selected_index = 0;
                KeyAction::Continue
            }
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
                    (InputMode::Navigation, KeyCode::Char('h'), KeyModifiers::NONE) => {
                        self.cycle_category(-1);
                        self.selected_index = 0;
                    }
                    (InputMode::Navigation, KeyCode::Char('l'), KeyModifiers::NONE) => {
                        self.cycle_category(1);
                        self.selected_index = 0;
                    }
                    _ => {}
                }
                KeyAction::Continue
            }
        }
    }

    /// `Esc` steps back out one layer at a time: leave the filter, then drop the
    /// query, then close — so a double tap never destroys a search by surprise.
    fn back_out(&mut self) -> KeyAction {
        if self.input_mode == InputMode::Filtering {
            self.input_mode = InputMode::Navigation;
            return KeyAction::Continue;
        }
        if !self.search_query.is_empty() {
            self.search_query.clear();
            self.selected_index = 0;
            return KeyAction::Continue;
        }
        KeyAction::ExitDismiss
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

    /// A user who bound movement to letters, keeping the arrows and `C-n`/`C-p`
    /// they had before. `j` cannot both move the list and be typed, so this is the
    /// configuration that switches on the modal filter.
    fn make_vim_state() -> AppState {
        AppState::new(
            mock_nodes(),
            Keybindings {
                move_up: vec!["k".into(), "Up".into(), "C-p".into()],
                move_down: vec!["j".into(), "Down".into(), "C-n".into()],
                ..Default::default()
            },
            CategoryTab::all().to_vec(),
        )
    }

    /// Binding any navigation action to a typable key is what opts in; the
    /// defaults (`Up`/`Down`/`Tab`/`C-p`) are not typable, so existing users stay
    /// on type-to-search.
    #[test]
    fn test_only_printable_navigation_bindings_enable_modes() {
        assert_eq!(make_state().input_mode, InputMode::TypeToSearch);
        assert_eq!(make_vim_state().input_mode, InputMode::Navigation);
        assert!(!Keybindings::default().has_printable_navigation());
        for kb in [
            Keybindings {
                move_down: vec!["C-j".into()],
                ..Default::default()
            },
            Keybindings {
                dismiss: vec!["q".into()],
                ..Default::default()
            },
            Keybindings {
                next_category: vec!["S-Tab".into()],
                ..Default::default()
            },
        ] {
            assert!(!kb.has_printable_navigation(), "{kb:?}");
        }
    }

    /// Default users keep the old behaviour: typing filters at once, `/`,
    /// `j`/`k` and `h`/`l` are text, Backspace deletes, Esc clears before it closes.
    #[test]
    fn test_modeless_typing_filters_at_once() {
        let mut state = make_state();
        for c in "hj/kl".chars() {
            state.handle_key(make_key(KeyCode::Char(c), KeyModifiers::NONE), 10);
        }
        assert_eq!(state.search_query, "hj/kl");
        assert_eq!(state.input_mode, InputMode::TypeToSearch);
        assert_eq!(state.current_category, CategoryTab::Workspaces);

        state.handle_key(make_key(KeyCode::Backspace, KeyModifiers::NONE), 10);
        assert_eq!(state.search_query, "hj/k");

        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 10),
            KeyAction::Continue
        );
        assert!(state.search_query.is_empty());
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 10),
            KeyAction::ExitDismiss
        );
    }

    #[test]
    fn test_filter_mode_transitions_and_input_precedence() {
        let mut state = make_vim_state();
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
        // Esc steps back one layer at a time: the query goes before the popup.
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 4),
            KeyAction::Continue
        );
        assert!(state.search_query.is_empty());
        assert_eq!(
            state.handle_key(make_key(KeyCode::Esc, KeyModifiers::NONE), 4),
            KeyAction::ExitDismiss
        );
    }

    #[test]
    fn test_navigation_mode_ignores_unbound_printable_keys() {
        let mut state = make_vim_state();
        state.search_query = "kept".into();

        state.handle_key(make_key(KeyCode::Char('x'), KeyModifiers::NONE), 4);
        assert_eq!(state.search_query, "kept");
        assert_eq!(state.input_mode, InputMode::Navigation);

        // Bound letters still move the list; Backspace still edits the query.
        state.handle_key(make_key(KeyCode::Char('j'), KeyModifiers::NONE), 4);
        assert_eq!(state.selected_index, 1);
        state.handle_key(make_key(KeyCode::Backspace, KeyModifiers::NONE), 4);
        assert_eq!(state.search_query, "kep");

        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 4);
        assert_eq!(state.input_mode, InputMode::Filtering);
        assert_eq!(state.search_query, "kep");
    }

    /// In navigation mode `h`/`l` step through the category tabs next to the
    /// configured `Tab`/`S-Tab`, wrapping like them; while filtering they are text.
    #[test]
    fn test_navigation_mode_h_l_cycle_categories() {
        let mut state = make_vim_state();
        assert_eq!(state.current_category, CategoryTab::Workspaces);

        state.selected_index = 2;
        state.handle_key(make_key(KeyCode::Char('l'), KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Tabs);
        assert_eq!(state.selected_index, 0);

        state.handle_key(make_key(KeyCode::Char('h'), KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Workspaces);
        state.handle_key(make_key(KeyCode::Char('h'), KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::All);

        state.handle_key(make_key(KeyCode::Tab, KeyModifiers::NONE), 10);
        assert_eq!(state.current_category, CategoryTab::Workspaces);
        assert!(state.search_query.is_empty());

        state.handle_key(make_key(KeyCode::Char('/'), KeyModifiers::NONE), 10);
        for c in ['h', 'l'] {
            state.handle_key(make_key(KeyCode::Char(c), KeyModifiers::NONE), 10);
        }
        assert_eq!(state.search_query, "hl");
        assert_eq!(state.current_category, CategoryTab::Workspaces);
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

    #[test]
    fn test_filter_mode_preserves_navigation_and_control_bindings() {
        let mut state = make_vim_state();
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
        let mut state = make_vim_state();
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
        let keybindings = Keybindings {
            move_down: vec!["j".into()],
            ..Default::default()
        };
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
        // Opting in through a category key still leaves j/k on the built-in
        // movement path rather than a binding of the user's.
        let mut state = AppState::new(
            mock_nodes(),
            Keybindings {
                next_category: vec!["l".into()],
                ..Default::default()
            },
            CategoryTab::all().to_vec(),
        );
        assert_eq!(state.input_mode, InputMode::Navigation);

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

        let mut state = make_vim_state();
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
