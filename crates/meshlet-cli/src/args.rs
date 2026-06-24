use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "meshlet",
    about = "Personal bookmark manager with CRDT sync",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Add a bookmark
    Add {
        /// URL to bookmark
        url: String,

        /// Manual title (skip auto-fetch)
        #[arg(long, short)]
        title: Option<String>,

        /// Comma-separated tags
        #[arg(long)]
        tag: Option<String>,

        /// Description
        #[arg(long)]
        desc: Option<String>,

        /// Don't fetch title/desc from web
        #[arg(long)]
        no_fetch: bool,

        /// Don't auto-update title on future fetches
        #[arg(long)]
        immutable: bool,
    },

    /// List all bookmarks
    List {
        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
    },

    /// Search bookmarks
    Search {
        /// Search keywords
        keywords: Vec<String>,

        /// Search substrings, not whole words
        #[arg(long)]
        deep: bool,

        /// Treat keywords as regex
        #[arg(long)]
        regex: bool,

        /// Match ALL keywords (default: match ANY)
        #[arg(long)]
        all: bool,

        /// Filter by tags
        #[arg(long)]
        tag: Option<String>,
    },

    /// Delete bookmarks by index
    Delete {
        /// One or more display indices to delete
        indices: Vec<usize>,
    },

    /// Edit a bookmark
    Edit {
        /// Display index of the bookmark to edit
        index: usize,

        /// Change URL
        #[arg(long)]
        url: Option<String>,

        /// Change title
        #[arg(long)]
        title: Option<String>,

        /// Replace all tags (comma-separated)
        #[arg(long)]
        tag: Option<String>,

        /// Append tags (comma-separated)
        #[arg(long)]
        tag_add: Option<String>,

        /// Remove tags (comma-separated)
        #[arg(long)]
        tag_delete: Option<String>,

        /// Change description
        #[arg(long)]
        desc: Option<String>,
    },
}