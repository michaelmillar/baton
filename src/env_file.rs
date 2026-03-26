use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

pub fn load(path: &Path) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();

    if !path.exists() {
        return Ok(vars);
    }

    let content = std::fs::read_to_string(path)?;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim().to_string();
            let value = strip_quotes(value.trim());
            vars.insert(key, value);
        }
    }

    Ok(vars)
}

fn strip_quotes(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn load_from_str(content: &str) -> HashMap<String, String> {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        load(f.path()).unwrap()
    }

    #[test]
    fn simple_vars() {
        let vars = load_from_str("FOO=bar\nBAZ=qux\n");
        assert_eq!(vars["FOO"], "bar");
        assert_eq!(vars["BAZ"], "qux");
    }

    #[test]
    fn quoted_values() {
        let vars = load_from_str("A=\"hello world\"\nB='single'\n");
        assert_eq!(vars["A"], "hello world");
        assert_eq!(vars["B"], "single");
    }

    #[test]
    fn comments_and_blanks() {
        let vars = load_from_str("# comment\n\nFOO=bar\n  # another\nBAZ=qux\n");
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn spaces_around_equals() {
        let vars = load_from_str("KEY = value\n");
        assert_eq!(vars["KEY"], "value");
    }

    #[test]
    fn value_with_equals() {
        let vars = load_from_str("URL=postgres://user:pass@host/db?opt=1\n");
        assert_eq!(vars["URL"], "postgres://user:pass@host/db?opt=1");
    }

    #[test]
    fn missing_file_returns_empty() {
        let vars = load(Path::new("/nonexistent/.env")).unwrap();
        assert!(vars.is_empty());
    }

    #[test]
    fn empty_value() {
        let vars = load_from_str("EMPTY=\n");
        assert_eq!(vars["EMPTY"], "");
    }
}
