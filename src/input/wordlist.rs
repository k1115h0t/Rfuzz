use std::fs::File;
use std::io::{self, BufRead, BufReader};

use anyhow::{anyhow, Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordlistSpec {
    pub source: WordlistSource,
    pub keyword: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordlistSource {
    Path(String),
    Stdin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordlistData {
    pub keyword: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct WordlistLoadOptions {
    pub ignore_comments: bool,
    pub extensions: Vec<String>,
}

impl WordlistSpec {
    pub fn parse(raw: &str) -> Result<Self> {
        let (source, keyword) = parse_source_and_keyword(raw)?;

        if source.is_empty() {
            return Err(anyhow!("wordlist source cannot be empty"));
        }

        Ok(Self {
            source: if source == "-" {
                WordlistSource::Stdin
            } else {
                WordlistSource::Path(source.to_string())
            },
            keyword: keyword.to_string(),
        })
    }

    pub fn load(&self, options: &WordlistLoadOptions) -> Result<WordlistData> {
        let values = match &self.source {
            WordlistSource::Path(path) => {
                let file = File::open(path).with_context(|| format!("failed to open {}", path))?;
                read_lines(BufReader::new(file), options)?
            }
            WordlistSource::Stdin => read_lines(BufReader::new(io::stdin()), options)?,
        };
        Ok(WordlistData {
            keyword: self.keyword.clone(),
            values,
        })
    }
}

fn parse_source_and_keyword(raw: &str) -> Result<(&str, &str)> {
    let Some((source, keyword)) = raw.rsplit_once(':') else {
        return Ok((raw, "FUZZ"));
    };

    if is_valid_keyword(keyword) {
        return Ok((source, keyword));
    }

    if looks_like_windows_drive_path(source, keyword) {
        return Ok((raw, "FUZZ"));
    }

    Err(anyhow!(
        "invalid wordlist keyword '{}'; if this is a path containing ':', pass an explicit keyword such as '{}:FUZZ'",
        keyword,
        raw
    ))
}

fn looks_like_windows_drive_path(source: &str, suffix: &str) -> bool {
    source.len() == 1
        && source.as_bytes()[0].is_ascii_alphabetic()
        && (suffix.starts_with('\\') || suffix.starts_with('/'))
}

pub fn load_wordlists(
    specs: &[WordlistSpec],
    options: &WordlistLoadOptions,
) -> Result<Vec<WordlistData>> {
    specs.iter().map(|spec| spec.load(options)).collect()
}

fn read_lines<R: BufRead>(reader: R, options: &WordlistLoadOptions) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for line in reader
        .lines()
        .map(|line| line.map(|line| line.trim_end_matches('\r').to_string()))
    {
        let line = line?;
        if options.ignore_comments && line.trim_start().starts_with('#') {
            continue;
        }
        values.push(line.clone());
        for extension in &options.extensions {
            values.push(format!("{}{}", line, extension));
        }
    }
    Ok(values)
}

fn is_valid_keyword(keyword: &str) -> bool {
    let mut chars = keyword.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_uppercase())
        && chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn skips_comments_and_expands_extensions() {
        let options = WordlistLoadOptions {
            ignore_comments: true,
            extensions: vec![".php".to_string(), ".txt".to_string()],
        };
        let values = read_lines(Cursor::new("#c\nadmin\n"), &options).unwrap();
        assert_eq!(values, vec!["admin", "admin.php", "admin.txt"]);
    }

    #[test]
    fn keeps_windows_drive_paths_as_default_fuzz_wordlists() {
        let spec = WordlistSpec::parse(r"C:\wordlists\dirs.txt").unwrap();

        assert_eq!(spec.keyword, "FUZZ");
        assert_eq!(
            spec.source,
            WordlistSource::Path(r"C:\wordlists\dirs.txt".to_string())
        );
    }

    #[test]
    fn still_accepts_explicit_keyword_after_windows_path() {
        let spec = WordlistSpec::parse(r"C:\wordlists\dirs.txt:DIR").unwrap();

        assert_eq!(spec.keyword, "DIR");
        assert_eq!(
            spec.source,
            WordlistSource::Path(r"C:\wordlists\dirs.txt".to_string())
        );
    }

    #[test]
    fn rejects_invalid_explicit_keyword() {
        let error = WordlistSpec::parse("words.txt:bad").unwrap_err();

        assert!(error.to_string().contains("invalid wordlist keyword"));
    }
}
