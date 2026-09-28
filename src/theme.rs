//! Herdr `[theme.custom]` colour overrides.
//!
//! Herdr applies its palette in the order: built-in theme, `[theme.custom]`,
//! then `[theme.custom.light]` / `[theme.custom.dark]`. Plugins never receive
//! the resolved colours (only a theme *name*), so the navigator approximates the
//! base theme with its built-in dark/light palettes and layers the user's
//! `[theme.custom]` overrides on top — the same base-then-override order Herdr
//! uses.
//!
//! The `light`/`dark` sub-tables are intentionally ignored: a plugin gets no
//! signal about the current light/dark appearance, so there is no reliable way
//! to choose between them.
//!
//! `panel_bg` is the popup-chrome token: Herdr fills the plugin-popup frame with
//! it and draws the active tab in it on the accent background, so the navigator
//! maps it to the popup background (`surface_dim`) and to `on_accent` — even
//! though Herdr's config reference only shows it inside examples. `sidebar_bg`
//! is desktop-sidebar-only and has no equivalent in the popup.

use ratatui::style::Color;

/// Colour overrides read from Herdr's `[theme.custom]` table.
///
/// Field names are the navigator's palette slots; `from_toml` maps Herdr's
/// tokens onto them, which is not always one-to-one.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ThemeOverrides {
    pub accent: Option<Color>,
    pub surface0: Option<Color>,
    pub surface1: Option<Color>,
    pub surface_dim: Option<Color>,
    pub overlay0: Option<Color>,
    pub overlay1: Option<Color>,
    pub text: Option<Color>,
    pub subtext0: Option<Color>,
    pub mauve: Option<Color>,
    pub green: Option<Color>,
    pub yellow: Option<Color>,
    pub red: Option<Color>,
    pub blue: Option<Color>,
    pub teal: Option<Color>,
    pub peach: Option<Color>,
    /// Herdr's panel colour; also drives `on_accent` (active tab / chips).
    pub panel_bg: Option<Color>,
}

impl ThemeOverrides {
    /// Build overrides from a `[theme.custom]` value.
    ///
    /// Unknown keys and unparseable values are ignored, as are the `light` and
    /// `dark` sub-tables. Some slots are fed from a differently-named Herdr
    /// token: the popup background (`surface_dim`) from `panel_bg`, and the
    /// selected row (`surface1`) from `selection_bg` / `active_row_bg`.
    pub fn from_toml(custom: &toml::Value) -> Self {
        let Some(table) = custom.as_table() else {
            return Self::default();
        };
        let get = |key: &str| {
            table
                .get(key)
                .and_then(|v| v.as_str())
                .and_then(parse_color)
        };

        // Herdr's `surface_dim` is a separator/scrollbar colour, not a
        // background; its popup background is `panel_bg`.
        let surface_dim = get("panel_bg");
        // Herdr's sidebar selection is `selection_bg` (then `active_row_bg`);
        // its own `surface1` is a different surface (the dragged row).
        let surface1 = get("selection_bg").or_else(|| get("active_row_bg"));

        Self {
            accent: get("accent"),
            surface0: get("surface0"),
            surface1,
            surface_dim,
            overlay0: get("overlay0"),
            overlay1: get("overlay1"),
            text: get("text"),
            subtext0: get("subtext0"),
            mauve: get("mauve"),
            green: get("green"),
            yellow: get("yellow"),
            red: get("red"),
            blue: get("blue"),
            teal: get("teal"),
            peach: get("peach"),
            panel_bg: get("panel_bg"),
        }
    }
}

/// Parse a Herdr colour value (`hex`, `rgb(r,g,b)`, a reset alias, or a common
/// named colour) into a terminal colour. Returns `None` for anything else, so an
/// unrecognised value falls back to the theme palette rather than breaking.
pub fn parse_color(raw: &str) -> Option<Color> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "reset" | "default" | "none" | "transparent") {
        return Some(Color::Reset);
    }

    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }

    if let Some(inner) = lower.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let mut parts = inner.split(',').map(|p| p.trim().parse::<u8>());
        return match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Ok(r)), Some(Ok(g)), Some(Ok(b)), None) => Some(Color::Rgb(r, g, b)),
            _ => None,
        };
    }

    named_color(&lower)
}

fn parse_hex(hex: &str) -> Option<Color> {
    let mut bytes = Vec::with_capacity(4);
    match hex.len() {
        // #rgb: each nibble doubles (`f` -> `ff`).
        3 => {
            for c in hex.chars() {
                let d = c.to_digit(16)? as u8;
                bytes.push(d * 17);
            }
        }
        // #rrggbb and #rrggbbaa (alpha dropped).
        6 | 8 => {
            let chars: Vec<char> = hex.chars().collect();
            for pair in chars.chunks(2) {
                let hi = pair.first()?.to_digit(16)? as u8;
                let lo = pair.get(1)?.to_digit(16)? as u8;
                bytes.push(hi * 16 + lo);
            }
        }
        _ => return None,
    }
    match bytes.as_slice() {
        [r, g, b, ..] => Some(Color::Rgb(*r, *g, *b)),
        _ => None,
    }
}

/// A small set of common named colours. Herdr accepts more names than this;
/// unknown names are ignored and fall back to the theme palette.
fn named_color(lower: &str) -> Option<Color> {
    let compact: String = lower
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .collect();
    Some(match compact.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "cyan" | "aqua" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(parse_color("#1a1b26"), Some(Color::Rgb(0x1a, 0x1b, 0x26)));
        assert_eq!(parse_color("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_color("#89b4faff"), Some(Color::Rgb(0x89, 0xb4, 0xfa)));
        assert_eq!(parse_color("#gggggg"), None);
        assert_eq!(parse_color("#12"), None);
    }

    #[test]
    fn parses_rgb_function() {
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(parse_color("RGB(1,2,3)"), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(parse_color("rgb(1, 2)"), None);
        assert_eq!(parse_color("rgb(300,0,0)"), None);
    }

    #[test]
    fn parses_reset_aliases() {
        for alias in ["reset", "default", "none", "transparent", " DEFAULT "] {
            assert_eq!(parse_color(alias), Some(Color::Reset), "{alias}");
        }
    }

    #[test]
    fn parses_common_named_colors() {
        assert_eq!(parse_color("black"), Some(Color::Black));
        assert_eq!(parse_color("Blue"), Some(Color::Blue));
        assert_eq!(parse_color("dark-grey"), Some(Color::DarkGray));
        assert_eq!(parse_color("rebeccapurple"), None);
    }

    #[test]
    fn from_toml_maps_tokens_to_slots() {
        let custom: toml::Value = toml::from_str(
            "accent = \"#a6e3a1\"\npanel_bg = \"reset\"\nselection_bg = \"#313244\"\n",
        )
        .unwrap();
        let o = ThemeOverrides::from_toml(&custom);
        assert_eq!(o.accent, Some(Color::Rgb(0xa6, 0xe3, 0xa1)));
        assert_eq!(o.surface_dim, Some(Color::Reset)); // from panel_bg
        assert_eq!(o.panel_bg, Some(Color::Reset)); // kept for on_accent
        assert_eq!(o.surface1, Some(Color::Rgb(0x31, 0x32, 0x44))); // from selection_bg
    }

    #[test]
    fn from_toml_does_not_conflate_colliding_tokens() {
        let custom: toml::Value = toml::from_str(
            "panel_bg = \"#111111\"\nsurface_dim = \"#222222\"\n\
             selection_bg = \"#333333\"\nsurface1 = \"#444444\"\n\n[light]\npanel_bg = \"#ffffff\"\n",
        )
        .unwrap();
        let o = ThemeOverrides::from_toml(&custom);
        // Popup background follows `panel_bg`, not Herdr's `surface_dim`.
        assert_eq!(o.surface_dim, Some(Color::Rgb(0x11, 0x11, 0x11)));
        // Selected row follows `selection_bg`, not Herdr's `surface1`.
        assert_eq!(o.surface1, Some(Color::Rgb(0x33, 0x33, 0x33)));
    }

    #[test]
    fn from_toml_ignores_unknown_and_invalid() {
        let custom: toml::Value =
            toml::from_str("accent = \"not-a-color\"\nnope = \"#fff\"\n").unwrap();
        let o = ThemeOverrides::from_toml(&custom);
        assert_eq!(o.accent, None);
    }
}
