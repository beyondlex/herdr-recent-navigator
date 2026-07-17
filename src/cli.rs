use clap::{Parser, Subcommand};

/// Herdr Recent Navigator — a recent items switcher for Herdr.
#[derive(Debug, Parser)]
#[command(name = "herdr-recent-navigator", version, about)]
pub struct Cli {
    /// Optional subcommand (track mode)
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Default view category tab to open.
    #[arg(long = "view", value_parser = ["workspaces", "tabs", "agents", "panes"])]
    pub view: Option<String>,

    /// Open the overlay pane (called by plugin_action keybinding).
    #[arg(long = "pane-open")]
    pub pane_open: bool,

    /// Use mock data instead of connecting to Herdr (for development).
    #[cfg(feature = "mock")]
    #[arg(long = "mock")]
    pub mock: bool,

    /// Write logs to this file instead of stderr (for development).
    #[arg(long = "log-file")]
    pub log_file: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Record a pane.focused event to the MRU state file.
    Track,
}
