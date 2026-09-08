//! Pure helpers for the "Others" state-search dimension.
//!
//! Only pure, unit-testable functions — no IPC, no rendering.

/// SSH-family binaries whose first positional argument is a login target.
const SSH_BINS: &[&str] = &["ssh", "mosh", "rsh", "rlogin", "tssh"];

/// SSH options that consume a value, so the next token is NOT a target.
/// (Combined short forms like `-p2200` / `-oFoo=bar` are handled separately.)
const SSH_OPTS_WITH_VALUE: &[&str] = &[
    "-p", "-l", "-b", "-E", "-e", "-F", "-G", "-J", "-W", "-Q", "-S", "-D", "-L", "-R", "-O", "-o",
    "-c", "-i", "-I", "-w", "-B", "-K", "-M", "-P", "-U",
];

fn bin_is_ssh(cmd: &str) -> bool {
    let first = cmd.split_whitespace().next().unwrap_or("");
    let base = first.rsplit('/').next().unwrap_or(first);
    SSH_BINS.contains(&base)
}

/// Return `(user, port)` updates for a single SSH option token.
fn apply_ssh_opt(opt: &str, val: &str, user: &mut Option<String>, port: &mut Option<String>) {
    match opt {
        "-p" => *port = Some(val.to_string()),
        "-l" => *user = Some(val.to_string()),
        _ => {}
    }
}

/// Extract the ssh/mosh login target from a foreground command line.
///
/// Handles: separate `-p PORT` / `-o VALUE` / `-l USER` forms, combined short
/// forms (`-p2200`, `-oOption=Value`), `--` terminator, and jump hosts
/// (`-J`) which must NOT be treated as the destination. The first non-option
/// positional argument is the destination; everything after it is the remote
/// command and is ignored.
///
/// Returns something like `lex@192.168.1.50`, `host`, or `host:2200`.
pub fn parse_ssh_target(cmd: &str) -> Option<String> {
    if !bin_is_ssh(cmd) {
        return None;
    }
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }

    let mut user: Option<String> = None;
    let mut port: Option<String> = None;
    let mut dest: Option<String> = None;
    let mut i = 1; // skip the binary itself

    while i < tokens.len() {
        let t = tokens[i];
        if dest.is_some() {
            // First positional found: the rest is the remote command.
            break;
        }
        if t == "--" {
            if let Some(next) = tokens.get(i + 1) {
                dest = Some((*next).to_string());
            }
            break;
        }
        if t.starts_with('-') && t.len() > 1 {
            // Combined short form: `-p2200` or `-oOption=Value`.
            if let Some(opt) = SSH_OPTS_WITH_VALUE
                .iter()
                .find(|o| t.starts_with(**o) && t.len() > (**o).len())
            {
                let val = &t[opt.len()..];
                apply_ssh_opt(opt, val, &mut user, &mut port);
                i += 1;
                continue;
            }
            // `-p PORT` / `-o VALUE`: the value is the next token.
            if SSH_OPTS_WITH_VALUE.contains(&t) {
                i += 1;
                if let Some(val) = tokens.get(i) {
                    apply_ssh_opt(t, val, &mut user, &mut port);
                }
                i += 1;
                continue;
            }
            // Flag option without a value.
            i += 1;
            continue;
        }
        // Non-option token: this is the destination.
        dest = Some(t.to_string());
        break;
    }

    let dest = dest?;
    let target = match user {
        Some(u) if !dest.contains('@') => format!("{u}@{dest}"),
        _ => dest,
    };
    match port {
        // Skip `:port` when the host already carries one (e.g. IPv6 `[::1]`).
        Some(p) if !target.contains(':') => format!("{target}:{p}"),
        _ => target,
    }
    .into()
}

/// Common login shells by basename (an idle shell's foreground command).
const SHELL_NAMES: &[&str] = &[
    "zsh", "bash", "sh", "dash", "ksh", "tcsh", "fish", "csh", "ash", "nu", "elvish", "xonsh",
    "pwsh", "powershell",
];

/// Return true if the foreground command is (just) a login shell.
/// Foreground commands like `-zsh` / `-bash` carry a leading dash.
pub fn command_is_shell(cmd: &str) -> bool {
    let first = cmd.split_whitespace().next().unwrap_or("");
    let stripped = first.strip_prefix('-').unwrap_or(first);
    let base = stripped.rsplit('/').next().unwrap_or(stripped);
    SHELL_NAMES.contains(&base)
}

/// Editor binaries whose visible buffer is a file being edited.
const EDITOR_NAMES: &[&str] = &[
    "nvim", "vim", "vi", "code", "cursor", "neovim", "emacs", "helix", "hx", "nano", "micro",
    "sublime", "kak", "lvim", "zed",
];

/// Return true if the foreground command is a file editor. Used to split
/// content-search rows into `file` (excerpt is the edited file's content)
/// vs `term` (excerpt is plain terminal/command output).
pub fn command_is_editor(cmd: &str) -> bool {
    let first = cmd.split_whitespace().next().unwrap_or("");
    let stripped = first.strip_prefix('-').unwrap_or(first);
    let base = stripped.rsplit('/').next().unwrap_or(stripped);
    EDITOR_NAMES.contains(&base)
}

/// True when a pane title looks like a shell prompt (`user@host: cwd`),
/// the OSC title shells set as their window title by default
/// (e.g. `you@work-host: ~/repo`, `deploy@10.0.0.5:/opt/app`).
/// A prompt-shaped title is a strong sign the pane is a shell/terminal,
/// so content matched in it is terminal output — never a file.
pub fn is_shell_prompt_title(title: &str) -> bool {
    let t = title.trim();
    let Some(at) = t.find('@') else {
        return false;
    };
    if at == 0 || t[..at].chars().any(char::is_whitespace) {
        return false;
    }
    let rest = &t[at + 1..];
    let Some(colon) = rest.find(':') else {
        return false;
    };
    if colon == 0 || rest[..colon].chars().any(char::is_whitespace) {
        return false;
    }
    let after = rest[colon + 1..].trim_start();
    after.starts_with('~') || after.starts_with('/') || after.starts_with('.')
}

// ── Terminal buffer content helpers ──

/// Strip ANSI SGR/SGR+OSC/charset escape sequences from terminal output.
/// Handles CSI (`ESC [ params bytes byte`), OSC (`ESC ] ... BEL|ESC \`),
/// and charset-selection escapes (`ESC ( B`). Idempotent on plain text.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: parameters (0x20..=0x3F), final byte (0x40..=0x7E).
            Some('[') => {
                chars.next();
                while let Some(&n) = chars.peek() {
                    let byte = n as u32;
                    if (0x20..=0x3F).contains(&byte) {
                        chars.next();
                    } else if (0x40..=0x7E).contains(&byte) {
                        chars.next();
                        break;
                    } else {
                        break; // malformed; stop consuming
                    }
                }
            }
            // OSC: terminated by BEL or ESC \.
            Some(']') => {
                chars.next();
                loop {
                    match chars.next() {
                        None | Some('\u{07}') => break,
                        Some('\u{1b}') => {
                            let _ = chars.next(); // consume the `\` of ESC \ ST
                            break;
                        }
                        Some(_) => {}
                    }
                }
            }
            // Charset selection: `ESC ( B`, `ESC ) B`, etc.
            Some('(') | Some(')') | Some('*') | Some('+') => {
                chars.next();
                chars.next();
            }
            _ => {}
        }
    }
    out
}

/// Sanitize a single-line excerpt: tabs and control chars become spaces.
fn sanitize_line(line: &str) -> String {
    line.chars()
        .map(|c| if c == '\t' || c.is_control() { ' ' } else { c })
        .collect()
}

/// Max chars of preceding context kept before the matched term.
const EXCERPT_PRE: usize = 14;
/// Max chars kept after the matched term.
const EXCERPT_POST: usize = 80;

/// Build an excerpt from `content` for the match at byte offset `mid`, so the
/// matched term sits near the LEFT edge (terminals wrap long lines, and the
/// Detail column is narrow — a line-anchored excerpt would bury the hit).
/// Long regions are trimmed with ellipses.
fn excerpt_around(content: &str, mid: usize, needle_len: usize) -> String {
    let line_start = content[..mid].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = content[mid..].find('\n').map(|i| mid + i).unwrap_or(content.len());
    let line = sanitize_line(&content[line_start..line_end]);
    // The sanitize step maps each char to a single char, so byte offsets in the
    // original line are still valid on the sanitized line.
    let rel = mid - line_start;
    let rel_c = line[..rel].chars().count();

    let line_c = line.chars().count();
    let start_c = rel_c.saturating_sub(EXCERPT_PRE);
    let end_c = (rel_c + needle_len + EXCERPT_POST).min(line_c);
    let mut out = String::with_capacity(end_c - start_c + 2);
    if start_c > 0 {
        out.push('…');
    }
    let start_b = line.char_indices().nth(start_c).map(|(i, _)| i).unwrap_or(line.len());
    let end_b = line.char_indices().nth(end_c).map(|(i, _)| i).unwrap_or(line.len());
    out.push_str(&line[start_b..end_b]);
    if end_c < line_c {
        out.push('…');
    }
    out
}

/// Find the first case-insensitive occurrence of `needle` in `content` and
/// return an excerpt around it — the `detail` for a `File` row.
///
/// Search is exact-first (fast for ASCII/CJK), falling back to a Unicode-aware
/// lowercase comparison. `content` should already be ANSI-stripped.
pub fn content_excerpt(content: &str, needle: &str) -> Option<String> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    let pos = content
        .find(needle)
        .or_else(|| content.to_lowercase().find(&needle.to_lowercase()))?;
    Some(excerpt_around(content, pos, needle.chars().count()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_ansi ──

    #[test]
    fn strips_sgr_and_csi() {
        assert_eq!(
            strip_ansi("\x1b[38;5;196mred\x1b[0m plain"),
            "red plain"
        );
        assert_eq!(strip_ansi("\x1b[1mB\x1b[0m"), "B");
    }

    #[test]
    fn strips_osc_title_sequences() {
        assert_eq!(
            strip_ansi("\x1b]0;tmux title\x07text"),
            "text"
        );
        assert_eq!(
            strip_ansi("\x1b]633;prop=;row=1\x1b\\row"),
            "row"
        );
    }

    #[test]
    fn strips_charset_select() {
        assert_eq!(strip_ansi("\x1b(Babc"), "abc");
    }

    #[test]
    fn leaves_plain_text_unchanged() {
        assert_eq!(strip_ansi("plain text with no escapes"), "plain text with no escapes");
    }

    // ── content_excerpt ──

    #[test]
    fn finds_case_insensitive_match() {
        let content = "line one\nDeployService started on port 8081\nline three";
        let ex = content_excerpt(content, "deployservice").unwrap();
        assert!(ex.eq_ignore_ascii_case("deployservice started on port 8081"));
    }

    #[test]
    fn crops_long_lines_anchoring_match_left() {
        let mut line = String::new();
        line.extend(std::iter::repeat_n('a', 200));
        line.insert_str(140, "NEEDLE"); // total 206 chars
        let content = line;
        let ex = content_excerpt(&content, "NEEDLE").unwrap();
        assert!(ex.contains("NEEDLE"));
        assert!(ex.starts_with('…'), "crop left marker: {ex}");
        // Match must sit near the LEFT edge so a narrow Detail column still shows it.
        let prefix = ex[..ex.find("NEEDLE").unwrap()].chars().count();
        let leading_marker = usize::from(ex.starts_with('…'));
        assert!(prefix - leading_marker <= EXCERPT_PRE, "match buried at char {prefix}: {ex}");
        assert!(ex.chars().count() <= EXCERPT_PRE + 6 + EXCERPT_POST + 2);
    }

    #[test]
    fn match_anchored_left_on_wrapped_terminal_line() {
        // Long wrapped line (nvim buffer) with gutter decorations where the hit
        // is deep into the line — a line-anchored excerpt would start at the
        // gutter and truncate before the match.
        let content = concat!(
            "│\u{10f105}                               32/32 ││  2    ",
            "plans 目录里的一行 md, 词尾是 thers 时摘录需左对齐到命中处",
        );
        let ex = content_excerpt(content, "thers").unwrap();
        let prefix = ex[..ex.find("thers").unwrap()].chars().count();
        let leading_marker = usize::from(ex.starts_with('…'));
        assert!(prefix - leading_marker <= EXCERPT_PRE, "match buried at char {prefix}: {ex}");
    }

    #[test]
    fn returns_single_line() {
        let content = "head\nmiddle tail\nother";
        let ex = content_excerpt(content, "tail").unwrap();
        assert_eq!(ex, "middle tail");
    }

    #[test]
    fn sanitizes_control_chars() {
        let content = "a\tb\x00c wrap";
        let ex = content_excerpt(content, "wrap").unwrap();
        assert_eq!(ex, "a b c wrap");
    }

    #[test]
    fn returns_none_when_absent() {
        assert_eq!(content_excerpt("abc def", "zzz"), None);
        assert_eq!(content_excerpt("abc", "  "), None);
    }

    #[test]
    fn parses_bare_ssh_host() {
        assert_eq!(
            parse_ssh_target("ssh deploy.example.com"),
            Some("deploy.example.com".into())
        );
    }

    #[test]
    fn parses_user_at_host() {
        assert_eq!(parse_ssh_target("ssh lex@192.168.1.50"), Some("lex@192.168.1.50".into()));
    }

    #[test]
    fn parses_custom_port_separate_arg() {
        assert_eq!(
            parse_ssh_target("ssh -p 2200 lex@192.168.1.50"),
            Some("lex@192.168.1.50:2200".into())
        );
    }

    #[test]
    fn parses_custom_port_combined_arg() {
        assert_eq!(
            parse_ssh_target("ssh -p2200 lex@192.168.1.50"),
            Some("lex@192.168.1.50:2200".into())
        );
    }

    #[test]
    fn skips_options_with_separate_values() {
        assert_eq!(
            parse_ssh_target("ssh -o ServerAliveInterval=30 -l root 10.0.0.7"),
            Some("root@10.0.0.7".into())
        );
    }

    #[test]
    fn skips_combined_option_value() {
        assert_eq!(
            parse_ssh_target("ssh -oServerAliveInterval=30 10.0.0.7"),
            Some("10.0.0.7".into())
        );
    }

    #[test]
    fn jump_host_is_not_the_destination() {
        assert_eq!(
            parse_ssh_target("ssh -J bastion@gateway.example.com prod@10.9.9.9"),
            Some("prod@10.9.9.9".into())
        );
    }

    #[test]
    fn handles_double_dash_terminator() {
        assert_eq!(
            parse_ssh_target("ssh -- prod@10.9.9.9 uptime"),
            Some("prod@10.9.9.9".into())
        );
    }

    #[test]
    fn mosh_is_ssh_family() {
        assert_eq!(parse_ssh_target("mosh user@laptop.local"), Some("user@laptop.local".into()));
    }

    #[test]
    fn returns_none_for_non_ssh() {
        assert_eq!(parse_ssh_target("docker compose up -d"), None);
        assert_eq!(parse_ssh_target("nvim src/main.rs"), None);
    }

    #[test]
    fn returns_none_for_bare_binary() {
        assert_eq!(parse_ssh_target("ssh"), None);
    }

    #[test]
    fn login_shell_is_shell() {
        assert!(command_is_shell("-zsh"));
        assert!(command_is_shell("zsh"));
        assert!(command_is_shell("/bin/bash"));
    }

    #[test]
    fn non_shell_is_not_shell() {
        assert!(!command_is_shell("nvim src/main.rs"));
        assert!(!command_is_shell("psql -U poste -h 127.0.0.1"));
        assert!(!command_is_shell("ssh prod@10.9.9.9"));
        assert!(!command_is_shell(""));
    }

    #[test]
    fn editor_is_editor() {
        assert!(command_is_editor("nvim src/main.rs"));
        assert!(command_is_editor("/usr/local/bin/nvim plans/x.md"));
        assert!(command_is_editor("code --wait src/app.rs"));
        assert!(command_is_editor("hx"));
        assert!(command_is_editor("emacs -nw README.md"));
    }

    #[test]
    fn shell_prompt_title_shapes() {
        assert!(is_shell_prompt_title("you@work-host: ~/repo"));
        assert!(is_shell_prompt_title("deploy@10.0.0.5:/opt/app"));
        assert!(is_shell_prompt_title("ali@203.0.113.7: ~"));
        assert!(is_shell_prompt_title("root@server:/var/log"));
        assert!(is_shell_prompt_title("user@192.168.4.1:~/web"));
    }

    #[test]
    fn non_shell_prompt_title_shapes() {
        assert!(!is_shell_prompt_title("/usr/local/bin/nvim"));
        assert!(!is_shell_prompt_title("OC | 项目手机端适配"));
        assert!(!is_shell_prompt_title("caffeinate -i"));
        assert!(!is_shell_prompt_title("user@host"));
        assert!(!is_shell_prompt_title(""));
    }

    #[test]
    fn non_editor_is_not_editor() {
        assert!(!command_is_editor("-zsh"));
        assert!(!command_is_editor("git log --oneline"));
        assert!(!command_is_editor("htop"));
        assert!(!command_is_editor("ssh prod@10.9.9.9"));
        assert!(!command_is_editor(""));
        assert!(!command_is_editor("docker compose up -d"));
    }
}