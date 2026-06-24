use std::io::Write;
use std::process::Command;

use anyhow::{Context, Result};

const EDITOR_TEMPLATE: &str = r##"# Lines beginning with # are comments
# Enter your bookmark details and save + exit to continue.
# Line 1: URL
# Line 2: Title
# Line 3: Comma-separated tags
# Line 4: Description (optional)
#
"##;

pub struct EditorData {
    pub url: String,
    pub title: String,
    pub tags: String,
    pub desc: String,
}

pub fn open_editor(initial_url: Option<&str>) -> Result<Option<EditorData>> {
    let mut tempfile = tempfile::Builder::new()
        .prefix("meshlet-")
        .suffix(".txt")
        .tempfile()
        .context("failed to create tempfile")?;

    write!(tempfile, "{}", EDITOR_TEMPLATE)?;
    if let Some(url) = initial_url {
        writeln!(tempfile, "{}", url)?;
    }
    tempfile.flush()?;

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let path = tempfile.path().to_path_buf();

    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .with_context(|| format!("failed to run editor '{}'", editor))?;

    if !status.success() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path).context("failed to read edited file")?;

    let lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();

    if lines.is_empty() || lines[0].trim().is_empty() {
        return Ok(None);
    }

    let url = lines.first().map(|s| s.trim().to_string()).unwrap_or_default();
    let title = lines.get(1).map(|s| s.trim().to_string()).unwrap_or_default();
    let tags = lines.get(2).map(|s| s.trim().to_string()).unwrap_or_default();
    let desc = lines.get(3).map(|s| s.trim().to_string()).unwrap_or_default();

    if url.is_empty() {
        return Ok(None);
    }

    Ok(Some(EditorData {
        url,
        title,
        tags,
        desc,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_contains_key_instructions() {
        assert!(EDITOR_TEMPLATE.contains("URL"));
        assert!(EDITOR_TEMPLATE.contains("Title"));
        assert!(EDITOR_TEMPLATE.contains("tags"));
    }
}