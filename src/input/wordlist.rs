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
        let (source, keyword) = raw.rsplit_once(':').unwrap_or((raw, "FUZZ"));

        if source.is_empty() {
            return Err(anyhow!("wordlist source cannot be empty"));
        }
        if !is_valid_keyword(keyword) {
            return Err(anyhow!("invalid wordlist keyword '{}'", keyword));
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
}
