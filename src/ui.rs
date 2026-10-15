use crate::format::*;
use crate::models::{
    AgentStatus, AppState, CategoryTab, DisplayItem, InputMode, OtherSource, OthersFilter,
    WORKTREE_SEP,
};
use crate::theme::ThemeOverrides;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs};
use unicode_width::UnicodeWidthStr;

// ── Palette ──────────────────────────────────────────────────────────────────

#[allow(dead_code)]
struct Palette {
    accent: Color,
    surface0: Color,
    surface1: Color,
    surface_dim: Color,
    /// Colour for text drawn on top of an accent-coloured background (tab
    /// highlight, filter chips). Kept separate from `surface_dim` so a palette
    /// can pick a readable contrast colour independently of the surface.
    on_accent: Color,
    overlay0: Color,
    overlay1: Color,
    text: Color,
    subtext0: Color,
    mauve: Color,
    green: Color,
    yellow: Color,
    red: Color,
    blue: Color,
    teal: Color,
    peach: Color,
}

impl Palette {
    fn dark() -> Self {
        Self {
            accent: Color::Rgb(122, 162, 247),   // #7aa2f7
            surface0: Color::Rgb(36, 40, 59),    // #24283b
            surface1: Color::Rgb(65, 72, 104),   // #414868
            surface_dim: Color::Rgb(26, 27, 38), // #1a1b26
            on_accent: Color::Rgb(26, 27, 38),   // #1a1b26
            overlay0: Color::Rgb(86, 95, 137),   // #565f89
            overlay1: Color::Rgb(105, 113, 150), // #69719e
            text: Color::Rgb(192, 202, 245),     // #c0caf5
            subtext0: Color::Rgb(169, 177, 214), // #a9b1d6
            mauve: Color::Rgb(187, 154, 247),    // #bb9af7
            green: Color::Rgb(158, 206, 106),    // #9ece6a
            yellow: Color::Rgb(224, 175, 104),   // #e0af68
            red: Color::Rgb(247, 118, 142),      // #f7768e
            blue: Color::Rgb(122, 162, 247),     // #7aa2f7
            teal: Color::Rgb(125, 207, 255),     // #7dcfff
            peach: Color::Rgb(255, 158, 100),    // #ff9e64
        }
    }

    fn light() -> Self {
        Self {
            accent: Color::Rgb(46, 125, 233),       // #2e7de9
            surface0: Color::Rgb(203, 204, 209),    // #cbccd1
            surface1: Color::Rgb(216, 217, 222),    // #d8d9de
            surface_dim: Color::Rgb(239, 241, 245), // #eff1f5
            on_accent: Color::Rgb(239, 241, 245),   // #eff1f5
            overlay0: Color::Rgb(156, 157, 165),    // #9c9da5
            overlay1: Color::Rgb(139, 140, 148),    // #8b8d94
            text: Color::Rgb(55, 96, 191),          // #3760bf
            subtext0: Color::Rgb(97, 114, 176),     // #6172b0
            mauve: Color::Rgb(120, 71, 189),        // #7847bd
            green: Color::Rgb(88, 117, 57),         // #587539
            yellow: Color::Rgb(143, 94, 21),        // #8f5e15
            red: Color::Rgb(245, 42, 101),          // #f52a65
            blue: Color::Rgb(46, 125, 233),         // #2e7de9
            teal: Color::Rgb(0, 113, 151),          // #007197
            peach: Color::Rgb(177, 92, 0),          // #b15c00
        }
    }

    /// Inherit the terminal's own palette: default foreground/background for
    /// text and surfaces, ANSI colours for accents. This matches whatever theme
    /// the terminal is using, including Herdr's `[theme] name = "terminal"`.
    fn terminal() -> Self {
        Self {
            accent: Color::Blue,
            surface0: Color::DarkGray,
            surface1: Color::DarkGray,
            surface_dim: Color::Reset,
            on_accent: Color::Black,
            overlay0: Color::Gray,
            overlay1: Color::Gray,
            text: Color::Reset,
            subtext0: Color::Gray,
            mauve: Color::Magenta,
            green: Color::Green,
            yellow: Color::Yellow,
            red: Color::Red,
            blue: Color::Blue,
            teal: Color::Cyan,
            peach: Color::LightRed,
        }
    }

    fn for_theme(theme_name: Option<&str>) -> Self {
        match theme_name.map(str::trim) {
            // Herdr is configured to follow the host terminal, so do the same.
            Some(n) if n.eq_ignore_ascii_case("terminal") => Self::terminal(),
            Some(n) if is_light_theme(n) => Self::light(),
            // A Herdr theme, or no theme at all (which means Herdr's own default
            // theme): prefer the built-in dark palette over the host terminal.
            _ => Self::dark(),
        }
    }

    /// Layer Herdr's `[theme.custom]` overrides on top of this palette. Slots
    /// the user did not override keep their theme value.
    fn overlay(&mut self, o: &ThemeOverrides) {
        macro_rules! apply {
            ($($field:ident),+ $(,)?) => {
                $( if let Some(color) = o.$field { self.$field = color; } )+
            };
        }
        apply!(
            accent,
            surface0,
            surface1,
            surface_dim,
            overlay0,
            overlay1,
            text,
            subtext0,
            mauve,
            green,
            yellow,
            red,
            blue,
            teal,
            peach,
        );
        // Herdr's focused tab uses `panel_bg` text on the accent background.
        // `reset` means the palette's own dim surface, so keep the default.
        if let Some(color) = o.panel_bg.filter(|c| *c != Color::Reset) {
            self.on_accent = color;
        }
    }
}

fn is_light_theme(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    // Accept a bare "light" so an explicit manifest value works as written.
    name == "light"
        || name.ends_with("-latte")
        || name.ends_with("-light")
        || name.ends_with("-day")
        || name.ends_with("-dawn")
        || name.ends_with("-lotus")
}

// ── Braille spinner frames (herdr-compatible) ───────────────────────────────

const SPINNERS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn spinner_frame(tick: u32) -> &'static str {
    SPINNERS[(tick as usize / 3) % SPINNERS.len()]
}

// ── Status helpers (herdr-compatible) ───────────────────────────────────────

fn state_dot(st: &AgentStatus, p: &Palette) -> (&'static str, Style) {
    match st {
        AgentStatus::Blocked => ("●", Style::default().fg(p.red)),
        AgentStatus::Working => ("●", Style::default().fg(p.yellow)),
        AgentStatus::Done => ("●", Style::default().fg(p.teal)),
        AgentStatus::Idle => ("○", Style::default().fg(p.green)),
        AgentStatus::None => ("·", Style::default().fg(p.overlay0)),
    }
}

fn agent_icon(st: &AgentStatus, tick: u32, p: &Palette) -> (&'static str, Style) {
    match st {
        AgentStatus::Blocked => ("◉", Style::default().fg(p.red)),
        AgentStatus::Working => (spinner_frame(tick), Style::default().fg(p.yellow)),
        AgentStatus::Done => ("●", Style::default().fg(p.teal)),
        AgentStatus::Idle => ("✓", Style::default().fg(p.green)),
        AgentStatus::None => ("○", Style::default().fg(p.overlay0)),
    }
}

fn num_span(i: usize, sel: bool, p: &Palette) -> Span<'static> {
    Span::styled(
        format!("{:>2}. ", i + 1),
        Style::default().fg(if sel { p.text } else { p.overlay0 }),
    )
}

fn name_style(sel: bool, p: &Palette) -> Style {
    if sel {
        Style::default().fg(p.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn ctx_style(sel: bool, p: &Palette) -> Style {
    Style::default().fg(if sel { p.text } else { p.overlay0 })
}

fn hl_style(sel: bool, p: &Palette) -> Style {
    let mut s = Style::default().fg(p.yellow);
    if sel {
        s = s.add_modifier(Modifier::BOLD);
    }
    s
}

fn row_sel_style(sel: bool, p: &Palette) -> Style {
    if sel {
        Style::default().bg(p.surface1).fg(p.text)
    } else {
        Style::default()
    }
}

// ── Public render entry point ───────────────────────────────────────────────

pub fn render(frame: &mut Frame, state: &AppState, displayed: &[DisplayItem], total: usize) {
    let p = {
        let mut p = Palette::for_theme(state.theme_name.as_deref());
        p.overlay(&state.theme_overrides);
        p
    };
    let area = frame.area();

    // Minimum terminal size guard
    if !min_terminal_size(area) {
        let msg = "Terminal too small — resize to at least 20×4";
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(p.red))))
                .style(Style::default().bg(p.surface_dim)),
            area,
        );
        return;
    }

    let narrow = area.width < 60;

    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(p.surface_dim)),
        area,
    );
    let inner = area;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);

    let matched = displayed.len();
    render_tabs(frame, state, chunks[0], &p, narrow);
    render_search(frame, state, chunks[1], matched, total, &p);

    let list_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(chunks[2]);
    // In the All tab a `ws `/`tab `/`pane ` filter swaps the list to
    // workspace/tab/pane entities, so the column header should match that
    // dimension's layout.
    let header_category = if state.current_category == CategoryTab::All {
        match OthersFilter::parse(&state.search_query).0 {
            Some(OthersFilter::Workspace) => CategoryTab::Workspaces,
            Some(OthersFilter::Tab) => CategoryTab::Tabs,
            Some(OthersFilter::Pane) => CategoryTab::Panes,
            _ => CategoryTab::All,
        }
    } else {
        state.current_category
    };
    render_column_header(frame, &header_category, list_chunks[0], &p, narrow);
    // With a filter prefix (`.` / `cmd ` / …) the All rows are keyed by
    // the remaining text (their `detail` carries the excerpt or the matched
    // value), so highlight with that — matching the raw query against them
    // would find nothing.
    let highlight_query = if state.current_category == CategoryTab::All {
        OthersFilter::parse(&state.search_query).1
    } else {
        state.search_query.as_str()
    };
    render_list(
        frame,
        displayed,
        state.selected_index,
        highlight_query,
        state.spinner_tick,
        list_chunks[1],
        &p,
    );
    render_status_bar(frame, chunks[3], &p, state);
}

// ── Sub-renderers ───────────────────────────────────────────────────────────

fn render_tabs(frame: &mut Frame, state: &AppState, area: Rect, p: &Palette, narrow: bool) {
    let tabs = &state.tabs;
    let sel_idx = tabs
        .iter()
        .position(|t| t == &state.current_category)
        .unwrap_or(0);
    let titles: Vec<Line> = tabs
        .iter()
        .map(|tab| {
            let label = if narrow {
                tab_label(tab, narrow).to_string()
            } else {
                format!(" {} ", tab_label(tab, narrow))
            };
            Line::from(Span::styled(label, Style::default().fg(p.accent)))
        })
        .collect();
    let divider = if narrow {
        Span::raw(" ").style(Style::default().fg(p.overlay0))
    } else {
        Span::raw("|").style(Style::default().fg(p.overlay0))
    };
    let borders = if p.surface_dim == Color::Reset {
        Borders::NONE
    } else {
        Borders::BOTTOM
    };
    frame.render_widget(
        Tabs::new(titles)
            .block(
                Block::default()
                    .borders(borders)
                    .border_style(Style::default().fg(p.surface_dim)),
            )
            .highlight_style(
                Style::default()
                    .fg(p.on_accent)
                    .bg(p.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .divider(divider)
            .select(sel_idx),
        area,
    );
}

fn render_search(
    frame: &mut Frame,
    state: &AppState,
    area: Rect,
    matched: usize,
    total: usize,
    p: &Palette,
) {
    // The mode lives in two cheap signals instead of a text badge: the prompt
    // takes the accent only while keys edit the filter, and Filtering shows a
    // real terminal cursor at the insertion point. A badge here would cost
    // 4-6 columns of the query on a 40-column popup.
    let filtering = state.input_mode == InputMode::Filtering;
    let prefix = " > ";
    // In the All tab a filter prefix (`cmd `, `.`, …) is lifted out of the
    // query and shown as a badge chip; the rest is the live search text.
    let (filter, rest) = if state.current_category == CategoryTab::All {
        OthersFilter::parse(&state.search_query)
    } else {
        (None, state.search_query.as_str())
    };
    let is_empty = rest.is_empty();
    let text = if is_empty {
        match state.input_mode {
            // Navigation mode is the one state where typing does not filter.
            InputMode::Navigation => "type / to filter",
            InputMode::TypeToSearch | InputMode::Filtering => "type to filter...",
        }
    } else {
        rest
    };
    let count_str = if is_empty {
        format!("{}", total)
    } else {
        format!("{}/{}", matched, total)
    };

    let text_style = if is_empty {
        Style::default().fg(p.overlay0)
    } else {
        Style::default().fg(p.text)
    };
    let prompt_style = match state.input_mode {
        InputMode::TypeToSearch => text_style,
        InputMode::Filtering => Style::default().fg(p.accent),
        InputMode::Navigation => Style::default().fg(p.overlay0),
    };
    let badge_w = match filter {
        Some(f) => f.label().len() + 4, // chip padding (" x ") + separator space
        None => 0,
    };
    let padding_right = 2;
    let pad = area.width.saturating_sub(
        (prefix.len() + badge_w + text.len() + count_str.len() + padding_right) as u16,
    ) as usize;

    let mut spans = vec![Span::styled(prefix, prompt_style)];
    if let Some(f) = filter {
        spans.push(filter_chip(f, p));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(text.to_string(), text_style));
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(count_str, Style::default().fg(p.overlay0)));
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(p.surface0)),
            )
            .style(Style::default().fg(p.text)),
        area,
    );

    // The insertion point is what says "typing goes to the filter" — the frame
    // hides the cursor on every other mode. Clamped so a query wider than the
    // pane cannot move it off the popup.
    if filtering {
        let x = (area.x as usize + prefix.len() + badge_w + rest.len()) as u16;
        frame.set_cursor_position((x.min(area.x + area.width.saturating_sub(1)), area.y));
    }
}

/// Colored chip shown left of the input while an All filter is active.
fn filter_chip(f: OthersFilter, p: &Palette) -> Span<'static> {
    let color = match f {
        OthersFilter::Content => p.accent,
        OthersFilter::Source(src) => source_color(&src, p),
        OthersFilter::Workspace => p.peach,
        OthersFilter::Tab => p.red,
        OthersFilter::Pane => p.blue,
    };
    Span::styled(
        format!(" {} ", f.label()),
        Style::default()
            .fg(p.on_accent)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

fn render_column_header(
    frame: &mut Frame,
    category: &CategoryTab,
    area: Rect,
    p: &Palette,
    narrow: bool,
) {
    if area.width < 10 || narrow {
        return;
    }
    let style = Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD);
    let (labels, col_start, constraints): (&[&str], usize, &[Constraint]) = match category {
        CategoryTab::Panes => (&["Pane", "Tab", "Workspace", "Agent"], 1, &col_layout::PANE),
        CategoryTab::Tabs => (&["Tab", "Workspace", "Agent"], 1, &col_layout::TAB),
        CategoryTab::Agents => (&["Agent", "Tab", "Workspace"], 2, &col_layout::AGENT),
        CategoryTab::Workspaces => (&["Workspace", "Agent"], 1, &col_layout::WORKSPACE),
        CategoryTab::All => (
            &["Type", "Detail", "Context", "Tab", "Workspace"],
            1,
            &col_layout::OTHER,
        ),
    };
    let cols = Layout::horizontal(constraints)
        .flex(Flex::Start)
        .split(Rect::new(0, 0, area.width, 1));
    let mut spans: Vec<Span> = vec![Span::raw(
        " ".repeat(
            cols[..col_start]
                .iter()
                .map(|c| c.width as usize)
                .sum::<usize>(),
        ),
    )];
    let label_count = labels.len();
    for (i, &label) in labels.iter().enumerate() {
        let cw = cols[col_start + i].width as usize;
        let align_right = *category == CategoryTab::Panes && i == label_count - 1;
        let truncated = truncate_to(label, cw);
        let w = UnicodeWidthStr::width(truncated.as_str());
        let pad = cw.saturating_sub(w);
        if align_right {
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            spans.push(Span::styled(truncated, style));
        } else {
            spans.push(Span::styled(truncated, style));
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(p.surface_dim)),
        area,
    );
}

fn render_list(
    frame: &mut Frame,
    items: &[DisplayItem],
    selected_index: usize,
    search_query: &str,
    tick: u32,
    area: Rect,
    p: &Palette,
) {
    let row_width = area.width as usize;
    let ctx = RowCtx {
        sel: false,
        rw: row_width,
        query: search_query,
        tick,
        p,
    };
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let sel = i == selected_index;
            let row_ctx = RowCtx {
                sel,
                rw: ctx.rw,
                query: ctx.query,
                tick: ctx.tick,
                p: ctx.p,
            };
            match item {
                DisplayItem::Workspace {
                    name,
                    agent_statuses,
                    ..
                } => row_workspace(i, name, agent_statuses, &row_ctx),
                DisplayItem::Tab {
                    name,
                    workspace,
                    agent_statuses,
                    ..
                } => row_tab(i, name, workspace, agent_statuses, &row_ctx),
                DisplayItem::Agent {
                    agent_id,
                    status,
                    tab,
                    workspace,
                    ..
                } => row_agent(i, agent_id, status, tab, workspace, &row_ctx),
                DisplayItem::Pane {
                    pane_name,
                    workspace,
                    tab,
                    agent_id,
                    status,
                    ..
                } => row_pane(i, pane_name, workspace, tab, agent_id, status, &row_ctx),
                DisplayItem::Other {
                    source,
                    detail,
                    context,
                    tab,
                    workspace,
                    ..
                } => row_other(i, source, detail, context, tab, workspace, &row_ctx),
            }
        })
        .collect();
    let visible_rows = area.height as usize;
    let scroll_offset = if visible_rows == 0 || items.len() <= visible_rows {
        0
    } else {
        let max_offset = items.len() - visible_rows;
        let target = selected_index.saturating_sub(visible_rows / 2);
        target.min(max_offset)
    };
    let mut list_state = ListState::default();
    list_state.select(Some(selected_index));
    *list_state.offset_mut() = scroll_offset;
    frame.render_stateful_widget(
        List::new(list_items)
            .block(Block::default().borders(Borders::NONE))
            .highlight_style(Style::default()),
        area,
        &mut list_state,
    );
}

/// Status bar: `(key, label)` hints, most essential first, packed into whatever
/// width the pane gives and dropped from the end when they do not fit — so a long
/// binding name (`C-Space`) shortens the bar instead of truncating it mid-word.
/// What is advertised depends on the input mode, and in every mode the keys named
/// are the ones the user configured.
fn render_status_bar(frame: &mut Frame, area: Rect, p: &Palette, state: &AppState) {
    fn first(v: &[String]) -> &str {
        v.first().map(|s| s.as_str()).unwrap_or("?")
    }
    let kb = &state.keybindings;
    let move_keys = format!("{}/{}", first(&kb.move_up), first(&kb.move_down));

    // Most essential first, least guessable last: the tab strip above the list
    // already shows the categories, so they get no hint here, and on a narrow
    // pane the last thing to fall off is how to start filtering — the empty
    // prompt says `type / to filter` for that. The hardcoded vim movement keys
    // are left out on purpose: navigation mode only switches on for someone who
    // bound a letter to a navigation action, so they chose those keys while
    // writing the config, which is also where the README documents them. Naming
    // them here would spend the scarcest column budget on keys the reader knows.
    let hints: Vec<(&str, &str)> = match state.input_mode {
        InputMode::TypeToSearch => vec![
            (first(&kb.select), "Focus"),
            (first(&kb.dismiss), "Close"),
            (first(&kb.next_category), "Next"),
            (first(&kb.previous_category), "Prev"),
            (move_keys.as_str(), "Move"),
        ],
        InputMode::Navigation => vec![
            (first(&kb.select), "Go"),
            (first(&kb.dismiss), "Close"),
            (move_keys.as_str(), "Move"),
            ("/", "Filter"),
        ],
        InputMode::Filtering => vec![
            (first(&kb.select), "Go"),
            (first(&kb.force_quit), "Close"),
            (first(&kb.dismiss), "Nav"),
            (move_keys.as_str(), "Move"),
        ],
    };

    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    for (key, label) in hints {
        // Display columns, not bytes: a binding name can carry a non-ASCII key
        // (`move_up = ["←"]`), which counts as one column but several bytes. The
        // three pad columns are the hint's own key/label space plus a two-column
        // gutter, which is what keeps `Esc Close  Tab Next` from reading as one
        // long label.
        let width = UnicodeWidthStr::width(key) + UnicodeWidthStr::width(label) + 3;
        if used + width > area.width as usize {
            break;
        }
        spans.push(Span::raw("  "));
        spans.push(Span::styled(key, Style::default().fg(p.accent)));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(label, Style::default().fg(p.overlay0)));
        used += width;
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(p.surface0)),
            )
            .style(Style::default().fg(p.overlay0)),
        area,
    );
}

// ── Flex column helper ──────────────────────────────────────────────────────

/// Build styled spans for a single flex column.
/// Truncates text to `width`, applies search highlighting,
/// pads right (left-align) or left (right-align).
///
/// Left-aligned columns reserve their last char as a gutter, so a column
/// whose content fills the whole allocation never touches the next one.
fn flex_col(
    text: &str,
    width: usize,
    query: &str,
    base_style: Style,
    hl_style: Style,
    align_right: bool,
) -> Vec<Span<'static>> {
    if width == 0 {
        return vec![];
    }
    let (content_w, gutter) = if align_right {
        (width, 0)
    } else {
        (width.saturating_sub(1), 1)
    };
    let display = truncate_to(text, content_w);
    let mut spans = highlight_text(&display, query, base_style, hl_style);
    let display_w = UnicodeWidthStr::width(display.as_str());
    let pad = content_w.saturating_sub(display_w);
    if pad > 0 {
        if align_right {
            spans.insert(0, Span::raw(" ".repeat(pad)));
        } else {
            spans.push(Span::raw(" ".repeat(pad)));
        }
    }
    if gutter > 0 {
        spans.push(Span::raw(" ".repeat(gutter)));
    }
    spans
}

// ── Column layout profiles ───────────────────────────────────────────────────

mod col_layout {
    use ratatui::layout::Constraint;

    pub const IDX: Constraint = Constraint::Length(4);
    pub const ICON: Constraint = Constraint::Length(2);
    pub const PANE: [Constraint; 5] = [
        IDX,
        Constraint::Percentage(28),
        Constraint::Percentage(22),
        Constraint::Percentage(28),
        Constraint::Percentage(22),
    ];
    pub const DOTS: Constraint = Constraint::Length(8);
    pub const TAB: [Constraint; 4] = [
        IDX,
        Constraint::Percentage(45),
        Constraint::Percentage(45),
        DOTS,
    ];
    pub const AGENT: [Constraint; 5] = [
        IDX,
        ICON,
        Constraint::Percentage(32),
        Constraint::Percentage(32),
        Constraint::Percentage(36),
    ];
    pub const WORKSPACE: [Constraint; 3] = [IDX, Constraint::Fill(1), DOTS];
    pub const OTHER: [Constraint; 6] = [
        IDX,
        Constraint::Length(6),
        Constraint::Percentage(26),
        Constraint::Percentage(26),
        Constraint::Percentage(16),
        Constraint::Percentage(24),
    ];
}

fn col_spans(text: &str, col: &Rect, query: &str, base: Style, hl: Style) -> Vec<Span<'static>> {
    flex_col(text, col.width as usize, query, base, hl, false)
}

// ── Row builders ────────────────────────────────────────────────────────────

struct RowCtx<'a> {
    sel: bool,
    rw: usize,
    query: &'a str,
    tick: u32,
    p: &'a Palette,
}

fn row_workspace(i: usize, name: &str, a: &[AgentStatus], ctx: &RowCtx) -> ListItem<'static> {
    let ns = name_style(ctx.sel, ctx.p);
    let hl = hl_style(ctx.sel, ctx.p);

    let rw = ctx.rw as u16;
    let cols = Layout::horizontal(col_layout::WORKSPACE)
        .flex(Flex::Start)
        .split(Rect::new(0, 0, rw, 1));

    let mut sp = vec![num_span(i, ctx.sel, ctx.p)];
    match name.split_once(WORKTREE_SEP) {
        Some((repo, wt)) => {
            let total = cols[1].width as usize;
            let repo_w = (UnicodeWidthStr::width(repo) + 1).min(total);
            sp.extend(flex_col(repo, repo_w, ctx.query, ns, hl, false));
            let wt = format!("{}{}", WORKTREE_SEP.trim_start(), wt);
            let ws = ctx_style(ctx.sel, ctx.p);
            sp.extend(flex_col(&wt, total - repo_w, ctx.query, ws, hl, false));
        }
        None => sp.extend(col_spans(name, &cols[1], ctx.query, ns, hl)),
    }
    let dots_w = a.len() * 2;
    let dots_col_w = cols[2].width as usize;
    let pad = dots_col_w.saturating_sub(dots_w);
    if !a.is_empty() {
        for st in a {
            let (dot, dot_style) = state_dot(st, ctx.p);
            sp.push(Span::styled(format!(" {dot}"), dot_style));
        }
    }
    if pad > 0 {
        sp.push(Span::raw(" ".repeat(pad)));
    }
    ListItem::new(Line::from(sp)).style(row_sel_style(ctx.sel, ctx.p))
}

fn row_tab(i: usize, name: &str, ws: &str, a: &[AgentStatus], ctx: &RowCtx) -> ListItem<'static> {
    let cs = ctx_style(ctx.sel, ctx.p);
    let hl = hl_style(ctx.sel, ctx.p);
    let ns = name_style(ctx.sel, ctx.p);

    let rw = ctx.rw as u16;
    let cols = Layout::horizontal(col_layout::TAB)
        .flex(Flex::Start)
        .split(Rect::new(0, 0, rw, 1));

    let mut sp = vec![num_span(i, ctx.sel, ctx.p)];
    sp.extend(col_spans(name, &cols[1], ctx.query, ns, hl));
    sp.extend(col_spans(ws, &cols[2], ctx.query, cs, hl));
    let dots_w = a.len() * 2;
    let dots_col_w = cols[3].width as usize;
    let pad = dots_col_w.saturating_sub(dots_w);
    if !a.is_empty() {
        for st in a {
            let (dot, dot_style) = state_dot(st, ctx.p);
            sp.push(Span::styled(format!(" {dot}"), dot_style));
        }
    }
    if pad > 0 {
        sp.push(Span::raw(" ".repeat(pad)));
    }
    ListItem::new(Line::from(sp)).style(row_sel_style(ctx.sel, ctx.p))
}

fn row_agent(
    i: usize,
    aid: &str,
    s: &AgentStatus,
    tab: &str,
    ws: &str,
    ctx: &RowCtx,
) -> ListItem<'static> {
    let cs = ctx_style(ctx.sel, ctx.p);
    let hl = hl_style(ctx.sel, ctx.p);
    let ns = name_style(ctx.sel, ctx.p);
    let (icon, icon_style) = agent_icon(s, ctx.tick, ctx.p);

    let rw = ctx.rw as u16;
    let cols = Layout::horizontal(col_layout::AGENT)
        .flex(Flex::Start)
        .split(Rect::new(0, 0, rw, 1));

    let mut sp = vec![num_span(i, ctx.sel, ctx.p)];
    sp.push(Span::styled(format!("{} ", icon), icon_style));
    sp.extend(col_spans(aid, &cols[2], ctx.query, ns, hl));
    sp.extend(col_spans(tab, &cols[3], ctx.query, cs, hl));
    sp.extend(col_spans(ws, &cols[4], ctx.query, cs, hl));
    ListItem::new(Line::from(sp)).style(row_sel_style(ctx.sel, ctx.p))
}

fn row_pane(
    i: usize,
    pn: &str,
    ws: &str,
    tab: &str,
    aid: &Option<String>,
    s: &AgentStatus,
    ctx: &RowCtx,
) -> ListItem<'static> {
    let cs = ctx_style(ctx.sel, ctx.p);
    let hl = hl_style(ctx.sel, ctx.p);
    let ns = if pn == "untitled" {
        if ctx.sel {
            Style::default().fg(ctx.p.overlay1)
        } else {
            Style::default().fg(ctx.p.overlay0)
        }
    } else {
        name_style(ctx.sel, ctx.p)
    };

    let rw = ctx.rw as u16;
    let cols = Layout::horizontal(col_layout::PANE)
        .flex(Flex::Start)
        .split(Rect::new(0, 0, rw, 1));

    let mut sp = vec![num_span(i, ctx.sel, ctx.p)];
    sp.extend(col_spans(pn, &cols[1], ctx.query, ns, hl));
    sp.extend(col_spans(tab, &cols[2], ctx.query, cs, hl));
    sp.extend(col_spans(ws, &cols[3], ctx.query, cs, hl));

    if let Some(a) = aid {
        let (icon, icon_style) = agent_icon(s, ctx.tick, ctx.p);
        let agent_style = Style::default().fg(icon_style.fg.unwrap_or(ctx.p.overlay0));
        let col_w = cols[4].width as usize;
        let icon_w = UnicodeWidthStr::width(icon);
        let name_w = col_w.saturating_sub(icon_w + 1);
        let display = truncate_to(a, name_w);
        let content_w = icon_w + 1 + UnicodeWidthStr::width(display.as_str());
        let pad = col_w.saturating_sub(content_w);
        if pad > 0 {
            sp.push(Span::raw(" ".repeat(pad)));
        }
        sp.push(Span::styled(format!("{} ", icon), icon_style));
        sp.extend(highlight_text(&display, ctx.query, agent_style, hl));
    }
    ListItem::new(Line::from(sp)).style(row_sel_style(ctx.sel, ctx.p))
}

/// Per-source color so the Type column is scannable at a glance.
fn source_color(source: &OtherSource, p: &Palette) -> Color {
    match source {
        OtherSource::Ssh => p.teal,
        OtherSource::Cmd => p.yellow,
        OtherSource::File => p.mauve,
        OtherSource::Terminal => p.blue,
        OtherSource::Cwd => p.green,
    }
}

fn source_style(source: &OtherSource, p: &Palette) -> Style {
    Style::default()
        .fg(source_color(source, p))
        .add_modifier(Modifier::BOLD)
}

fn row_other(
    i: usize,
    source: &OtherSource,
    detail: &str,
    context: &str,
    tab: &str,
    ws: &str,
    ctx: &RowCtx,
) -> ListItem<'static> {
    let cs = ctx_style(ctx.sel, ctx.p);
    let hl = hl_style(ctx.sel, ctx.p);
    let ns = name_style(ctx.sel, ctx.p);

    let rw = ctx.rw as u16;
    let cols = Layout::horizontal(col_layout::OTHER)
        .flex(Flex::Start)
        .split(Rect::new(0, 0, rw, 1));

    let mut sp = vec![num_span(i, ctx.sel, ctx.p)];

    // Type column (fixed width, right-side type reads like a tag).
    let type_col_w = cols[1].width as usize;
    let type_text = source.label();
    let tw = UnicodeWidthStr::width(type_text);
    let pad = type_col_w.saturating_sub(tw + 1) + 1; // 1-char gutter
    sp.push(Span::styled(
        type_text.to_string(),
        source_style(source, ctx.p),
    ));
    sp.push(Span::raw(" ".repeat(pad)));

    sp.extend(col_spans(detail, &cols[2], ctx.query, ns, hl));
    // Context (file path etc.): front-truncate so the tail — basename and
    // `:line` — stays visible; `-` ("no context") sits dimmer.
    if context == "-" {
        let dim = Style::default().fg(if ctx.sel { ctx.p.text } else { ctx.p.overlay1 });
        sp.extend(col_spans(context, &cols[3], "", dim, hl));
    } else {
        let display = truncate_front(context, cols[3].width.saturating_sub(1) as usize);
        sp.extend(col_spans(&display, &cols[3], ctx.query, cs, hl));
    }
    sp.extend(col_spans(tab, &cols[4], ctx.query, cs, hl));
    sp.extend(col_spans(ws, &cols[5], ctx.query, cs, hl));
    ListItem::new(Line::from(sp)).style(row_sel_style(ctx.sel, ctx.p))
}
// min_terminal_size, content_rect, truncate_to, tab_label, centered_rect
// are imported via `use crate::format::*;` at the top of this file.

#[cfg(test)]
mod status_hint_tests {
    use super::*;
    use crate::data::mock_nodes;
    use crate::models::Keybindings;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;

    /// Every word a hint may end on. A rendered bar that ends anywhere else was
    /// cut through a key or a label.
    const HINT_WORDS: [&str; 8] = [
        "Go", "Close", "Filter", "Move", "Nav", "Focus", "Next", "Prev",
    ];

    /// A vim-style binding set — the only configuration that turns modes on.
    fn vim_bindings() -> Keybindings {
        Keybindings {
            move_up: vec!["k".into()],
            move_down: vec!["j".into()],
            ..Default::default()
        }
    }

    fn build_state(mode: InputMode, kb: Keybindings, query: &str) -> AppState {
        let mut state = AppState::new(mock_nodes(), kb, CategoryTab::all().to_vec());
        state.input_mode = mode;
        state.search_query = query.into();
        state
    }

    /// The `(mode, bindings)` pairs a user can actually be in: modes need a
    /// printable navigation binding, the default keymap stays modeless.
    fn reachable_modes() -> [(InputMode, Keybindings); 3] {
        [
            (InputMode::TypeToSearch, Keybindings::default()),
            (InputMode::Navigation, vim_bindings()),
            (InputMode::Filtering, vim_bindings()),
        ]
    }

    /// Renders only the status strip of a `width`-column pane, so the assertions
    /// are about the bar itself and not about popup geometry.
    fn status(width: u16, mode: InputMode, kb: Keybindings) -> (String, Vec<Option<Color>>) {
        let state = build_state(mode, kb, "");
        let mut terminal = Terminal::new(TestBackend::new(width, 2)).unwrap();
        terminal
            .draw(|frame| {
                let p = Palette::for_theme(None);
                render_status_bar(frame, Rect::new(0, 0, width, 2), &p, &state);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut line = String::new();
        let mut fg = Vec::new();
        for x in 0..width {
            let cell = buf.cell((x, 1)).unwrap();
            line.push_str(cell.symbol());
            fg.push(cell.style().fg);
        }
        (line.trim_end().to_string(), fg)
    }

    /// Renders only the search row; also reports whether the frame asked the
    /// backend to show a cursor and where.
    fn search(width: u16, mode: InputMode, query: &str) -> (String, Option<Position>) {
        let state = build_state(mode, Keybindings::default(), query);
        let mut terminal = Terminal::new(TestBackend::new(width, 2)).unwrap();
        terminal.hide_cursor().unwrap();
        terminal
            .draw(|frame| {
                let p = Palette::for_theme(None);
                render_search(frame, &state, Rect::new(0, 0, width, 2), 1, 1, &p);
            })
            .unwrap();
        let backend = terminal.backend();
        let line = (0..width)
            .map(|x| backend.buffer().cell((x, 0)).unwrap().symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string();
        let cursor = backend.cursor_visible().then(|| backend.cursor_position());
        (line, cursor)
    }

    #[test]
    fn bar_carries_no_mode_tag() {
        for width in [30u16, 60, 100] {
            for (mode, kb) in reachable_modes() {
                let (line, _) = status(width, mode, kb);
                // A one-character tag reads as a truncated key name, so the bar
                // starts on the first hint instead.
                assert!(!line.starts_with('n'), "{mode:?} width={width}: {line:?}");
                assert!(!line.starts_with('i'), "{mode:?} width={width}: {line:?}");
                assert!(
                    line.starts_with("  Enter "),
                    "{mode:?} width={width}: {line:?}"
                );
            }
        }
    }

    #[test]
    fn keys_are_accented_and_labels_are_dim() {
        let p = Palette::for_theme(None);
        let (line, fg) = status(60, InputMode::Navigation, vim_bindings());
        assert!(line.starts_with("  Enter Go "), "{line:?}");
        assert_eq!(&fg[2..7], &[Some(p.accent); 5][..], "{line:?}");
        assert_eq!(&fg[8..10], &[Some(p.overlay0); 2][..], "{line:?}");
    }

    #[test]
    fn hints_are_split_by_a_two_column_gutter() {
        // One space would make `Go Esc Close` read as a single hint, so the gutter
        // between hints stays wider than the space inside one.
        for (mode, kb, expect) in [
            (
                InputMode::TypeToSearch,
                Keybindings::default(),
                "  Enter Focus  Esc Close",
            ),
            (
                InputMode::Navigation,
                vim_bindings(),
                "  Enter Go  Esc Close",
            ),
            (
                InputMode::Filtering,
                vim_bindings(),
                "  Enter Go  C-c Close",
            ),
        ] {
            let (line, _) = status(90, mode, kb);
            assert!(line.starts_with(expect), "{mode:?}: {line:?}");
        }
    }

    #[test]
    fn modeless_bar_keeps_the_classic_vocabulary() {
        let (bar, _) = status(90, InputMode::TypeToSearch, Keybindings::default());
        for hint in [
            "Enter Focus",
            "Esc Close",
            "Tab Next",
            "S-Tab Prev",
            "Up/Down Move",
        ] {
            assert!(bar.contains(hint), "{bar:?}");
        }
        // Typing filters on the first keystroke, so nothing about modes belongs
        // in this bar.
        assert!(!bar.contains("Filter"), "{bar:?}");
        assert!(!bar.contains("j/k"), "{bar:?}");
    }

    #[test]
    fn wide_panes_keep_every_hint() {
        let (nav, _) = status(90, InputMode::Navigation, vim_bindings());
        for hint in ["Enter Go", "Esc Close", "k/j Move", "/ Filter"] {
            assert!(nav.contains(hint), "{nav:?}");
        }
        let (filtering, _) = status(90, InputMode::Filtering, vim_bindings());
        for hint in ["Enter Go", "C-c Close", "Esc Nav", "k/j Move"] {
            assert!(filtering.contains(hint), "{filtering:?}");
        }
        // Categories are the tab strip's job, so no bar spends a column on them.
        assert!(!nav.contains("Tab"), "{nav:?}");
        assert!(!filtering.contains("Tab"), "{filtering:?}");
    }

    #[test]
    fn navigation_names_only_the_configured_keys() {
        // j/k move the list in navigation mode on top of the configured keys, but
        // they stay unadvertised: only someone who bound letters in the config can
        // be here, and that is the same person who read the README.
        let kb = Keybindings {
            move_down: vec!["j".into()],
            ..Default::default()
        };
        let (nav, _) = status(90, InputMode::Navigation, kb);
        assert!(nav.contains("Up/j Move"), "{nav:?}");
        assert!(!nav.contains("j/k"), "{nav:?}");
    }

    #[test]
    fn a_non_ascii_binding_is_packed_by_column_not_by_byte() {
        // "←/→" is three columns in seven bytes, and those four spare bytes are
        // enough to push `/ Filter` off a 41-column bar if the packing counts
        // bytes instead of columns.
        let kb = Keybindings {
            move_up: vec!["←".into()],
            move_down: vec!["→".into()],
            ..Default::default()
        };
        let (line, _) = status(41, InputMode::Navigation, kb);
        assert!(line.ends_with("/ Filter"), "{line:?}");
    }

    #[test]
    fn narrow_panes_drop_whole_hints_never_cut_one_mid_word() {
        let kb = Keybindings {
            next_category: vec!["C-Space".into()],
            dismiss: vec!["A-Meta-Backspace".into()],
            move_up: vec!["k".into()],
            move_down: vec!["j".into()],
            ..Default::default()
        };
        for width in 20..=90 {
            for mode in [
                InputMode::TypeToSearch,
                InputMode::Navigation,
                InputMode::Filtering,
            ] {
                let (line, _) = status(width, mode, kb.clone());
                let last = line.rsplit(' ').next().unwrap_or_default();
                assert!(
                    HINT_WORDS.contains(&last),
                    "width={width} {mode:?} ends mid-hint: {line:?}"
                );
            }
        }
    }

    #[test]
    fn narrower_bars_only_lose_whole_hints_from_the_tail() {
        let kb = Keybindings {
            next_category: vec!["A-Rather-Long-Binding".into()],
            move_up: vec!["k".into()],
            move_down: vec!["j".into()],
            ..Default::default()
        };
        for (mode, _) in reachable_modes() {
            let widest = status(200, mode, kb.clone()).0;
            for width in 20..=200 {
                let (line, _) = status(width, mode, kb.clone());
                assert!(
                    widest.starts_with(&line),
                    "width={width} {mode:?} reordered or cut: {line:?} vs {widest:?}"
                );
            }
        }
    }

    /// The prompt glyph is what tells the filter and the modes apart; the row
    /// itself never spends columns on a name for the mode.
    fn prompt_color(mode: InputMode, query: &str) -> Option<Color> {
        let state = build_state(mode, Keybindings::default(), query);
        let mut terminal = Terminal::new(TestBackend::new(40, 2)).unwrap();
        terminal
            .draw(|frame| {
                let p = Palette::for_theme(None);
                render_search(frame, &state, Rect::new(0, 0, 40, 2), 1, 1, &p);
            })
            .unwrap();
        terminal.backend().buffer().cell((1, 0)).unwrap().style().fg
    }

    #[test]
    fn prompt_stays_three_columns_and_only_its_color_shows_the_mode() {
        let p = Palette::for_theme(None);
        for mode in [
            InputMode::TypeToSearch,
            InputMode::Navigation,
            InputMode::Filtering,
        ] {
            let (line, _) = search(40, mode, "her");
            assert!(line.starts_with(" > her"), "{mode:?}: {line:?}");
            assert!(!line.contains("NAV"), "{mode:?}: {line:?}");
            assert!(!line.contains("FILTER"), "{mode:?}: {line:?}");
        }

        // The modeless prompt is styled exactly like the row text, so a default
        // install keeps the pre-mode appearance byte for byte.
        assert_eq!(prompt_color(InputMode::TypeToSearch, "her"), Some(p.text));
        assert_eq!(prompt_color(InputMode::TypeToSearch, ""), Some(p.overlay0));
        assert_eq!(prompt_color(InputMode::Filtering, "her"), Some(p.accent));
        assert_eq!(prompt_color(InputMode::Navigation, "her"), Some(p.overlay0));
    }

    #[test]
    fn empty_prompt_only_prompts_for_slash_in_navigation_mode() {
        for mode in [InputMode::TypeToSearch, InputMode::Filtering] {
            let (line, _) = search(40, mode, "");
            assert!(line.contains("type to filter..."), "{mode:?}: {line:?}");
        }
        let (nav, _) = search(40, InputMode::Navigation, "");
        assert!(nav.contains("type / to filter"), "{nav:?}");
        assert!(!nav.contains("type to filter"), "{nav:?}");
    }

    #[test]
    fn cursor_sits_at_the_insertion_point_only_while_filtering() {
        let (_, modeless) = search(40, InputMode::TypeToSearch, "her");
        assert_eq!(modeless, None, "modeless typing keeps the cursor hidden");

        let (_, nav_cursor) = search(40, InputMode::Navigation, "her");
        assert_eq!(nav_cursor, None, "navigation must leave the cursor hidden");

        let (_, filtering_cursor) = search(40, InputMode::Filtering, "her");
        assert_eq!(filtering_cursor, Some(Position { x: 6, y: 0 }));

        let (_, empty) = search(40, InputMode::Filtering, "");
        assert_eq!(empty, Some(Position { x: 3, y: 0 }));

        // A query wider than the pane must not push the cursor off the popup.
        let (_, overflow) = search(10, InputMode::Filtering, &"x".repeat(40));
        assert_eq!(overflow, Some(Position { x: 9, y: 0 }));
    }
}

#[cfg(test)]
mod palette_tests {
    use super::*;

    #[test]
    fn detects_light_theme_suffixes() {
        for name in [
            "one-light",
            "catppuccin-latte",
            "tokyonight-day",
            "rose-pine-dawn",
            "kanagawa-lotus",
        ] {
            assert!(is_light_theme(name), "{name} should be detected as light");
        }
    }

    #[test]
    fn detects_bare_light_value() {
        assert!(is_light_theme("light"));
        assert!(is_light_theme("  Light  "));
    }

    #[test]
    fn detects_light_case_insensitively() {
        assert!(is_light_theme("One-Light"));
        assert!(is_light_theme("Catppuccin-Latte"));
    }

    #[test]
    fn rejects_dark_themes() {
        for name in [
            "dark",
            "tokyonight",
            "tokyonight-storm",
            "catppuccin-mocha",
            "one-dark",
            "gruvbox",
            "lightning",
            "daylight-saving",
        ] {
            assert!(!is_light_theme(name), "{name} should not be light");
        }
    }

    #[test]
    fn for_theme_picks_light_palette_for_light_names() {
        let light = Palette::light();
        let picked = Palette::for_theme(Some("one-light"));
        assert_eq!(picked.text, light.text);
        assert_eq!(picked.surface_dim, light.surface_dim);
    }

    #[test]
    fn for_theme_uses_terminal_palette_only_when_explicit() {
        let term = Palette::terminal();
        assert_eq!(
            Palette::for_theme(Some("terminal")).surface_dim,
            term.surface_dim
        );
        assert_eq!(
            Palette::for_theme(Some("  Terminal ")).surface_dim,
            term.surface_dim
        );
    }

    #[test]
    fn terminal_palette_dim_text_is_visible_on_selected_row() {
        let term = Palette::terminal();
        assert_ne!(term.overlay0, term.surface1);
    }

    #[test]
    fn for_theme_prefers_herdr_theme_over_terminal_by_default() {
        let dark = Palette::dark();
        let light = Palette::light();
        // An unset theme means Herdr's own default theme, so prefer the
        // built-in dark palette rather than the host terminal.
        assert_eq!(Palette::for_theme(None).text, dark.text);
        // Any non-light Herdr theme name uses the built-in dark palette.
        assert_eq!(Palette::for_theme(Some("tokyonight")).text, dark.text);
        assert_eq!(Palette::for_theme(Some("catppuccin")).text, dark.text);
        assert_eq!(Palette::for_theme(Some("osaka-jade")).text, dark.text);
        // Light Herdr theme names use the built-in light palette.
        assert_eq!(Palette::for_theme(Some("one-light")).text, light.text);
    }

    #[test]
    fn overlay_applies_only_set_slots() {
        let base = Palette::dark();
        let mut p = Palette::dark();
        let o = ThemeOverrides {
            accent: Some(Color::Rgb(9, 9, 9)),
            surface_dim: Some(Color::Reset),
            panel_bg: Some(Color::Rgb(7, 7, 7)),
            ..ThemeOverrides::default()
        };
        p.overlay(&o);
        assert_eq!(p.accent, Color::Rgb(9, 9, 9));
        assert_eq!(p.surface_dim, Color::Reset);
        // `panel_bg` also drives the accent foreground.
        assert_eq!(p.on_accent, Color::Rgb(7, 7, 7));
        // Untouched slots keep their theme value.
        assert_eq!(p.text, base.text);
        assert_eq!(p.green, base.green);
    }

    #[test]
    fn overlay_keeps_default_on_accent_when_panel_bg_is_reset() {
        let base = Palette::light();
        let mut p = Palette::light();
        p.overlay(&ThemeOverrides {
            panel_bg: Some(Color::Reset),
            ..ThemeOverrides::default()
        });
        assert_eq!(p.on_accent, base.on_accent);
    }
}
