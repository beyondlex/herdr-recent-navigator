use crate::format::*;
use crate::models::{AgentStatus, AppState, CategoryTab, DisplayItem};
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
            accent: Color::Rgb(122, 162, 247),      // #7aa2f7
            surface0: Color::Rgb(36, 40, 59),       // #24283b
            surface1: Color::Rgb(65, 72, 104),      // #414868
            surface_dim: Color::Rgb(26, 27, 38),    // #1a1b26
            overlay0: Color::Rgb(86, 95, 137),      // #565f89
            overlay1: Color::Rgb(105, 113, 150),    // #69719e
            text: Color::Rgb(192, 202, 245),        // #c0caf5
            subtext0: Color::Rgb(169, 177, 214),    // #a9b1d6
            mauve: Color::Rgb(187, 154, 247),       // #bb9af7
            green: Color::Rgb(158, 206, 106),       // #9ece6a
            yellow: Color::Rgb(224, 175, 104),      // #e0af68
            red: Color::Rgb(247, 118, 142),         // #f7768e
            blue: Color::Rgb(122, 162, 247),        // #7aa2f7
            teal: Color::Rgb(125, 207, 255),        // #7dcfff
            peach: Color::Rgb(255, 158, 100),       // #ff9e64
        }
    }

    fn light() -> Self {
        Self {
            accent: Color::Rgb(46, 125, 233),       // #2e7de9
            surface0: Color::Rgb(203, 204, 209),    // #cbccd1
            surface1: Color::Rgb(184, 185, 191),    // #b8b9bf
            surface_dim: Color::Rgb(216, 217, 222), // #d8d9de
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

    fn for_theme(theme_name: Option<&str>) -> Self {
        match theme_name {
            Some(n) if is_light_theme(n) => Self::light(),
            _ => Self::dark(),
        }
    }
}

fn is_light_theme(name: &str) -> bool {
    name.ends_with("-latte")
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
    let p = Palette::for_theme(state.theme_name.as_deref());
    let area = frame.area();

    // Minimum terminal size guard
    if !min_terminal_size(area) {
        let msg = "Terminal too small — resize to at least 30×8";
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
    render_column_header(frame, &state.current_category, list_chunks[0], &p, narrow);
    render_list(
        frame,
        displayed,
        state.selected_index,
        state.search_query.as_str(),
        state.spinner_tick,
        list_chunks[1],
        &p,
    );
    render_status_bar(frame, chunks[3], &p, narrow);
}

// ── Sub-renderers ───────────────────────────────────────────────────────────

fn render_tabs(frame: &mut Frame, state: &AppState, area: Rect, p: &Palette, narrow: bool) {
    let tabs = CategoryTab::all();
    let sel_idx = tabs
        .iter()
        .position(|t| t == &state.current_category)
        .unwrap_or(0);
    let titles: Vec<Line> = tabs
        .iter()
        .map(|tab| {
            Line::from(Span::styled(
                format!(" {} ", tab_label(tab, narrow)),
                Style::default().fg(p.accent),
            ))
        })
        .collect();
    frame.render_widget(
        Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(p.surface0)),
            )
            .highlight_style(
                Style::default()
                    .fg(p.surface0)
                    .bg(p.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .divider(Span::raw("|").style(Style::default().fg(p.overlay0)))
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
    let prefix = " > ";
    let is_empty = state.search_query.is_empty();
    let text = if is_empty {
        "type to filter..."
    } else {
        &state.search_query
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
    let padding_right = 2;
    let line = Line::from(vec![
        Span::styled(format!("{}{}", prefix, text), text_style),
        Span::styled(
            " ".repeat(area.width.saturating_sub(
                (prefix.len() + text.len() + count_str.len() + padding_right) as u16,
            ) as usize),
            Style::default(),
        ),
        Span::styled(count_str, Style::default().fg(p.overlay0)),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(p.surface0)),
            )
            .style(Style::default().fg(p.text)),
        area,
    );
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
    };
    let cols = Layout::horizontal(constraints)
        .flex(Flex::Start)
        .split(Rect::new(0, 0, area.width, 1));
    let mut spans: Vec<Span> = vec![Span::raw(" ".repeat(cols[..col_start].iter().map(|c| c.width as usize).sum::<usize>()))];
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

fn render_status_bar(frame: &mut Frame, area: Rect, p: &Palette, narrow: bool) {
    let hints: Vec<Span> = if narrow {
        vec![
            Span::styled(" Tab", Style::default().fg(p.accent)),
            Span::styled("↔", Style::default().fg(p.overlay0)),
            Span::styled(" S-Tab", Style::default().fg(p.accent)),
            Span::styled("↔", Style::default().fg(p.overlay0)),
            Span::styled(" ↵", Style::default().fg(p.accent)),
            Span::styled("Go", Style::default().fg(p.overlay0)),
            Span::styled(" Esc", Style::default().fg(p.accent)),
            Span::styled("✕", Style::default().fg(p.overlay0)),
        ]
    } else {
        vec![
            Span::styled("   Tab", Style::default().fg(p.accent)),
            Span::styled(" Next Tab", Style::default().fg(p.overlay0)),
            Span::styled("   S-Tab", Style::default().fg(p.accent)),
            Span::styled(" Prev Tab", Style::default().fg(p.overlay0)),
            Span::styled("   Enter", Style::default().fg(p.accent)),
            Span::styled(" Focus", Style::default().fg(p.overlay0)),
            Span::styled("   Esc", Style::default().fg(p.accent)),
            Span::styled(" Close", Style::default().fg(p.overlay0)),
        ]
    };
    frame.render_widget(
        Paragraph::new(Line::from(hints))
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
    let display = truncate_to(text, width);
    let mut spans = highlight_text(&display, query, base_style, hl_style);
    let content_w = UnicodeWidthStr::width(display.as_str());
    let pad = width.saturating_sub(content_w);
    if pad > 0 {
        if align_right {
            spans.insert(0, Span::raw(" ".repeat(pad)));
        } else {
            spans.push(Span::raw(" ".repeat(pad)));
        }
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
    pub const TAB: [Constraint; 4] = [IDX, Constraint::Percentage(45), Constraint::Percentage(45), DOTS];
    pub const AGENT: [Constraint; 5] = [
        IDX,
        ICON,
        Constraint::Percentage(32),
        Constraint::Percentage(32),
        Constraint::Percentage(36),
    ];
    pub const WORKSPACE: [Constraint; 3] = [IDX, Constraint::Fill(1), DOTS];
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
    sp.extend(col_spans(name, &cols[1], ctx.query, ns, hl));
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

// ── Mobile / responsive helpers & layout — now in crate::format ──
// min_terminal_size, content_rect, truncate_to, tab_label, centered_rect
// are imported via `use crate::format::*;` at the top of this file.
