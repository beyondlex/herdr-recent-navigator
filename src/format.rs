//! Pure formatting and layout utilities for the navigator UI.
//! This module contains ONLY pure functions — no rendering, no Frame.
//! All functions here are unit-testable without a terminal.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use std::collections::HashSet;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::models::CategoryTab;
use crate::mru::match_indices;

// ── Text truncation ──

/// Truncate a string to the given max display width,
/// appending '…' when truncation occurs.
pub fn truncate_to(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(s) <= max_width {
        return s.to_string();
    }
    let mut result = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw + 1 > max_width {
            result.push('…');
            break;
        }
        result.push(c);
        w += cw;
    }
    result
}

// ── Search highlighting ──

/// Build a list of styled spans, marking characters matching `query`
/// with the `highlight` style and the rest with `base`.
/// Merges contiguous same-style characters to reduce Span allocations.
pub fn highlight_text<'a>(text: &str, query: &str, base: Style, highlight: Style) -> Vec<Span<'a>> {
    if query.is_empty() {
        return vec![Span::styled(text.to_string(), base)];
    }
    let matched_indices = match_indices(text, query);
    if matched_indices.is_empty() {
        return vec![Span::styled(text.to_string(), base)];
    }
    let matched_set: HashSet<usize> = matched_indices.into_iter().collect();
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut in_highlight = matched_set.contains(&0);
    let mut seg_start = 0;
    for (ci, (byte_offset, _)) in text.char_indices().enumerate() {
        let is_match = matched_set.contains(&ci);
        if is_match != in_highlight {
            spans.push(if in_highlight {
                Span::styled(text[seg_start..byte_offset].to_string(), highlight)
            } else {
                Span::styled(text[seg_start..byte_offset].to_string(), base)
            });
            seg_start = byte_offset;
            in_highlight = is_match;
        }
    }
    spans.push(if in_highlight {
        Span::styled(text[seg_start..].to_string(), highlight)
    } else {
        Span::styled(text[seg_start..].to_string(), base)
    });
    spans
}

// ── Tab labels ──

pub fn tab_label(tab: &CategoryTab, narrow: bool) -> &'static str {
    if narrow {
        match tab {
            CategoryTab::Workspaces => "WS",
            CategoryTab::Tabs => "Tabs",
            CategoryTab::Panes => "Panes",
            CategoryTab::Agents => "Agents",
        }
    } else {
        tab.label()
    }
}

// ── Terminal size guard ──

pub fn min_terminal_size(area: Rect) -> bool {
    area.width >= 20 && area.height >= 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    // ── truncate_to tests ──

    #[test]
    fn test_truncate_to_short_string() {
        assert_eq!(truncate_to("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_to_exact_fit() {
        assert_eq!(truncate_to("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_to_truncates_with_ellipsis() {
        let result = truncate_to("hello world", 5);
        assert_eq!(
            result.chars().count(),
            5,
            "truncated 'hello world' at 5 chars"
        );
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_truncate_to_respects_unicode_width() {
        // "你好世界" each CJK char is width 2, total 8
        // truncate to 4 should keep 1 char + ellipsis (width 4)
        let result = truncate_to("你好世界", 4);
        // The result should be "你…" which has 2 chars
        assert_eq!(
            result.chars().count(),
            2,
            "Should keep 1 CJK char + ellipsis"
        );
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_truncate_to_empty_string() {
        assert_eq!(truncate_to("", 5), "");
    }

    #[test]
    fn test_truncate_to_zero_max() {
        assert_eq!(truncate_to("hello", 0), "");
    }

    // ── highlight_text tests ──

    #[test]
    fn test_highlight_text_no_query() {
        let result = highlight_text(
            "hello",
            "",
            Style::default(),
            Style::default().fg(Color::Yellow),
        );
        assert_eq!(result.len(), 1, "No query should return single span");
        assert_eq!(result[0].content, "hello");
    }

    #[test]
    fn test_highlight_text_empty_text() {
        let result = highlight_text(
            "",
            "test",
            Style::default(),
            Style::default().fg(Color::Yellow),
        );
        assert_eq!(result.len(), 1);
        assert!(result[0].content.is_empty());
    }

    #[test]
    fn test_highlight_text_matches_highlighted() {
        let base = Style::default();
        let hl = Style::default().fg(Color::Yellow);
        let result = highlight_text("hello", "he", base, hl);
        assert_eq!(
            result.len(),
            2,
            "Contiguous same-style chars merged into 2 spans"
        );
        assert_eq!(result[0].content, "he");
        assert_eq!(
            result[0].style.fg,
            Some(Color::Yellow),
            "'he' should be highlighted"
        );
        assert_eq!(result[1].content, "llo");
        assert_eq!(result[1].style.fg, None, "'llo' should have base style");
    }

    // ── tab_label tests ──

    #[test]
    fn test_tab_label_workspaces() {
        assert_eq!(tab_label(&CategoryTab::Workspaces, false), "Workspaces");
        assert_eq!(tab_label(&CategoryTab::Workspaces, true), "WS");
    }

    #[test]
    fn test_tab_label_tabs() {
        assert_eq!(tab_label(&CategoryTab::Tabs, false), "Tabs");
        assert_eq!(tab_label(&CategoryTab::Tabs, true), "Tabs");
    }

    #[test]
    fn test_tab_label_agents() {
        assert_eq!(tab_label(&CategoryTab::Agents, false), "Agents");
        assert_eq!(tab_label(&CategoryTab::Agents, true), "Agents");
    }

    // ── min_terminal_size tests ──

    #[test]
    fn test_min_terminal_size_too_small() {
        let small = Rect::new(0, 0, 19, 5);
        assert!(!min_terminal_size(small));
    }

    #[test]
    fn test_min_terminal_size_ok() {
        let ok = Rect::new(0, 0, 20, 4);
        assert!(min_terminal_size(ok));
    }

    #[test]
    fn test_min_terminal_size_exact_boundary() {
        let boundary = Rect::new(0, 0, 20, 4);
        assert!(min_terminal_size(boundary));
    }
}
