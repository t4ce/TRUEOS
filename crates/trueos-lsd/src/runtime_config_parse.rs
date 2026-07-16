extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RuntimeConfig {
    scalars: BTreeMap<String, String>,
    lists: BTreeMap<String, Vec<String>>,
}

impl RuntimeConfig {
    pub(crate) fn parse(text: &str) -> Self {
        let mut config = Self::default();
        let mut section: Option<(usize, String)> = None;

        for raw_line in text.lines() {
            let without_comment = strip_comment(raw_line);
            let line = without_comment.trim_end();
            if line.trim().is_empty() || line.trim_start().starts_with("---") {
                continue;
            }

            let indent = line.len().saturating_sub(line.trim_start().len());
            let trimmed = line.trim_start();
            if let Some(item) = trimmed.strip_prefix('-') {
                if let Some((section_indent, key)) = section.as_ref()
                    && indent > *section_indent
                {
                    let item = unquote(item.trim());
                    if !item.is_empty() {
                        config.lists.entry(key.clone()).or_default().push(item);
                    }
                }
                continue;
            }

            let Some((raw_key, raw_value)) = split_key_value(trimmed) else {
                continue;
            };
            let key = raw_key.trim();
            if key.is_empty() {
                continue;
            }

            if section
                .as_ref()
                .is_some_and(|(section_indent, _)| indent <= *section_indent)
            {
                section = None;
            }

            let full_key = match section.as_ref() {
                Some((section_indent, section_key)) if indent > *section_indent => {
                    alloc::format!("{section_key}.{key}")
                }
                _ => key.to_string(),
            };
            let value = raw_value.trim();
            if value.is_empty() {
                section = Some((indent, full_key));
                continue;
            }

            if value.starts_with('[') && value.ends_with(']') {
                let values = parse_inline_list(&value[1..value.len() - 1]);
                config.lists.insert(full_key, values);
            } else {
                config.scalars.insert(full_key, unquote(value));
            }
        }

        config
    }

    pub(crate) fn scalar(&self, key: &str) -> Option<&str> {
        self.scalars.get(key).map(String::as_str)
    }

    pub(crate) fn list(&self, key: &str) -> Option<&[String]> {
        self.lists.get(key).map(Vec::as_slice)
    }
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            continue;
        }
        if ch == '#' && quote.is_none() {
            return &line[..index];
        }
    }
    line
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            continue;
        }
        if ch == ':' && quote.is_none() {
            return Some((&line[..index], &line[index + 1..]));
        }
    }
    None
}

fn parse_inline_list(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut start = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            continue;
        }
        if ch == ',' && quote.is_none() {
            let item = unquote(value[start..index].trim());
            if !item.is_empty() {
                values.push(item);
            }
            start = index + 1;
        }
    }

    let item = unquote(value[start..].trim());
    if !item.is_empty() {
        values.push(item);
    }
    values
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() < 2 {
        return value.to_string();
    }

    let first = value.as_bytes()[0];
    let last = value.as_bytes()[value.len() - 1];
    if first == b'\'' && last == b'\'' {
        return value[1..value.len() - 1].to_string();
    }
    if first != b'"' || last != b'"' {
        return value.to_string();
    }

    let mut out = String::new();
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(next) => out.push(next),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::RuntimeConfig;

    #[test]
    fn parses_upstream_style_sections_and_lists() {
        let config = RuntimeConfig::parse(
            r#"
classic: false
color:
  when: auto
ignore-globs:
  - .git
  - "*.tmp"
sorting:
  column: extension
  reverse: true
hyperlink: always
"#,
        );

        assert_eq!(config.scalar("classic"), Some("false"));
        assert_eq!(config.scalar("color.when"), Some("auto"));
        assert_eq!(config.scalar("sorting.column"), Some("extension"));
        assert_eq!(config.scalar("sorting.reverse"), Some("true"));
        assert_eq!(config.scalar("hyperlink"), Some("always"));
        assert_eq!(
            config.list("ignore-globs").unwrap(),
            &[
                alloc::string::String::from(".git"),
                alloc::string::String::from("*.tmp")
            ]
        );
    }

    #[test]
    fn parses_inline_lists_and_quoted_comments() {
        let config = RuntimeConfig::parse(
            r#"
ignore-globs: ["*.tmp", 'draft #1', target]
hyperlink: never # trailing comment
"#,
        );

        assert_eq!(config.list("ignore-globs").unwrap().len(), 3);
        assert_eq!(config.list("ignore-globs").unwrap()[1], "draft #1");
        assert_eq!(config.scalar("hyperlink"), Some("never"));
    }
}
