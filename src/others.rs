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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_ssh_host() {
        assert_eq!(parse_ssh_target("ssh deploy.example.com"), Some("deploy.example.com".into()));
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
}