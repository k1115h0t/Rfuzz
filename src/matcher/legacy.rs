use anyhow::{anyhow, Result};
use regex::Regex;

use crate::cli::{Cli, SetModeArg};
use crate::matcher::signature::ResponseSignature;

#[derive(Debug, Clone, Default)]
pub struct MatcherConfig {
    pub match_status: Option<StatusSpec>,
    pub filter_status: Option<StatusSpec>,
    pub match_size: Option<NumberSpec>,
    pub filter_size: Option<NumberSpec>,
    pub match_words: Option<NumberSpec>,
    pub filter_words: Option<NumberSpec>,
    pub match_lines: Option<NumberSpec>,
    pub filter_lines: Option<NumberSpec>,
    pub match_time: Option<TimeSpec>,
    pub filter_time: Option<TimeSpec>,
    pub match_regex: Vec<Regex>,
    pub filter_regex: Vec<Regex>,
    pub matcher_mode: SetMode,
    pub filter_mode: SetMode,
}

#[derive(Debug, Clone)]
pub struct StatusSpec {
    all: bool,
    ranges: Vec<RangeInclusiveU16>,
}

#[derive(Debug, Clone)]
struct RangeInclusiveU16 {
    start: u16,
    end: u16,
}

#[derive(Debug, Clone)]
pub struct NumberSpec {
    ranges: Vec<RangeInclusiveUsize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetMode {
    #[default]
    Or,
    And,
}

#[derive(Debug, Clone)]
pub enum TimeSpec {
    GreaterThan(u128),
    LessThan(u128),
    Values(NumberSpec),
}

#[derive(Debug, Clone)]
struct RangeInclusiveUsize {
    start: usize,
    end: usize,
}

impl MatcherConfig {
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        Ok(Self {
            match_status: cli
                .match_status
                .as_deref()
                .map(StatusSpec::parse)
                .transpose()?,
            filter_status: cli
                .filter_status
                .as_deref()
                .map(StatusSpec::parse)
                .transpose()?,
            match_size: cli
                .match_size
                .as_deref()
                .map(NumberSpec::parse)
                .transpose()?,
            filter_size: cli
                .filter_size
                .as_deref()
                .map(NumberSpec::parse)
                .transpose()?,
            match_words: cli
                .match_words
                .as_deref()
                .map(NumberSpec::parse)
                .transpose()?,
            filter_words: cli
                .filter_words
                .as_deref()
                .map(NumberSpec::parse)
                .transpose()?,
            match_lines: cli
                .match_lines
                .as_deref()
                .map(NumberSpec::parse)
                .transpose()?,
            filter_lines: cli
                .filter_lines
                .as_deref()
                .map(NumberSpec::parse)
                .transpose()?,
            match_time: cli.match_time.as_deref().map(TimeSpec::parse).transpose()?,
            filter_time: cli
                .filter_time
                .as_deref()
                .map(TimeSpec::parse)
                .transpose()?,
            match_regex: compile_regexes(&cli.match_regex)?,
            filter_regex: compile_regexes(&cli.filter_regex)?,
            matcher_mode: cli.matcher_mode.into(),
            filter_mode: cli.filter_mode.into(),
        })
    }

    pub fn should_output(&self, signature: &ResponseSignature, body: &str) -> bool {
        if self.is_filtered(signature, body) {
            return false;
        }
        if !self.has_matchers() {
            return true;
        }
        self.is_matched(signature, body)
    }

    fn has_matchers(&self) -> bool {
        self.match_status.is_some()
            || self.match_size.is_some()
            || self.match_words.is_some()
            || self.match_lines.is_some()
            || self.match_time.is_some()
            || !self.match_regex.is_empty()
    }

    fn is_filtered(&self, signature: &ResponseSignature, body: &str) -> bool {
        let checks = [
            self.filter_status
                .as_ref()
                .map(|spec| spec.matches(signature.status)),
            self.filter_size
                .as_ref()
                .map(|spec| spec.matches(signature.size)),
            self.filter_words
                .as_ref()
                .map(|spec| spec.matches(signature.words)),
            self.filter_lines
                .as_ref()
                .map(|spec| spec.matches(signature.lines)),
            self.filter_time
                .as_ref()
                .map(|spec| spec.matches(signature.elapsed_ms)),
        ];
        combine(
            checks
                .into_iter()
                .flatten()
                .chain(self.filter_regex.iter().map(|regex| regex.is_match(body))),
            self.filter_mode,
        )
    }

    fn is_matched(&self, signature: &ResponseSignature, body: &str) -> bool {
        let checks = [
            self.match_status
                .as_ref()
                .map(|spec| spec.matches(signature.status)),
            self.match_size
                .as_ref()
                .map(|spec| spec.matches(signature.size)),
            self.match_words
                .as_ref()
                .map(|spec| spec.matches(signature.words)),
            self.match_lines
                .as_ref()
                .map(|spec| spec.matches(signature.lines)),
            self.match_time
                .as_ref()
                .map(|spec| spec.matches(signature.elapsed_ms)),
        ];
        combine(
            checks
                .into_iter()
                .flatten()
                .chain(self.match_regex.iter().map(|regex| regex.is_match(body))),
            self.matcher_mode,
        )
    }
}

impl From<SetModeArg> for SetMode {
    fn from(value: SetModeArg) -> Self {
        match value {
            SetModeArg::Or => Self::Or,
            SetModeArg::And => Self::And,
        }
    }
}

fn combine<I>(checks: I, mode: SetMode) -> bool
where
    I: IntoIterator<Item = bool>,
{
    let checks = checks.into_iter().collect::<Vec<_>>();
    if checks.is_empty() {
        return false;
    }
    match mode {
        SetMode::Or => checks.into_iter().any(|value| value),
        SetMode::And => checks.into_iter().all(|value| value),
    }
}

impl StatusSpec {
    pub fn parse(raw: &str) -> Result<Self> {
        if raw == "all" {
            return Ok(Self {
                all: true,
                ranges: Vec::new(),
            });
        }

        let mut ranges = Vec::new();
        for part in raw.split(',').filter(|part| !part.trim().is_empty()) {
            let part = part.trim();
            if let Some((start, end)) = part.split_once('-') {
                let start = start.parse::<u16>()?;
                let end = end.parse::<u16>()?;
                if start > end {
                    return Err(anyhow!("invalid status range '{}'", part));
                }
                ranges.push(RangeInclusiveU16 { start, end });
            } else {
                let value = part.parse::<u16>()?;
                ranges.push(RangeInclusiveU16 {
                    start: value,
                    end: value,
                });
            }
        }

        if ranges.is_empty() {
            return Err(anyhow!("empty status spec"));
        }
        Ok(Self { all: false, ranges })
    }

    pub fn matches(&self, value: u16) -> bool {
        self.all
            || self
                .ranges
                .iter()
                .any(|range| value >= range.start && value <= range.end)
    }
}

impl NumberSpec {
    pub fn parse(raw: &str) -> Result<Self> {
        let mut ranges = Vec::new();
        for part in raw.split(',').filter(|part| !part.trim().is_empty()) {
            let part = part.trim();
            if let Some((start, end)) = part.split_once('-') {
                let start = start.parse::<usize>()?;
                let end = end.parse::<usize>()?;
                if start > end {
                    return Err(anyhow!("invalid numeric range '{}'", part));
                }
                ranges.push(RangeInclusiveUsize { start, end });
            } else {
                let value = part.parse::<usize>()?;
                ranges.push(RangeInclusiveUsize {
                    start: value,
                    end: value,
                });
            }
        }
        if ranges.is_empty() {
            return Err(anyhow!("empty numeric spec"));
        }
        Ok(Self { ranges })
    }

    pub fn matches(&self, value: usize) -> bool {
        self.ranges
            .iter()
            .any(|range| value >= range.start && value <= range.end)
    }
}

impl TimeSpec {
    pub fn parse(raw: &str) -> Result<Self> {
        if let Some(value) = raw.strip_prefix('>') {
            return Ok(Self::GreaterThan(value.parse::<u128>()?));
        }
        if let Some(value) = raw.strip_prefix('<') {
            return Ok(Self::LessThan(value.parse::<u128>()?));
        }
        Ok(Self::Values(NumberSpec::parse(raw)?))
    }

    pub fn matches(&self, value: u128) -> bool {
        match self {
            Self::GreaterThan(min) => value > *min,
            Self::LessThan(max) => value < *max,
            Self::Values(values) => values.matches(value as usize),
        }
    }
}

fn compile_regexes(raw: &[String]) -> Result<Vec<Regex>> {
    raw.iter()
        .map(|pattern| Regex::new(pattern).map_err(Into::into))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(status: u16, size: usize, words: usize, lines: usize) -> ResponseSignature {
        ResponseSignature {
            status,
            size,
            words,
            lines,
            elapsed_ms: 1,
            location: None,
            title: None,
            body_hash: 0,
        }
    }

    fn timed_sig(elapsed_ms: u128) -> ResponseSignature {
        ResponseSignature {
            elapsed_ms,
            ..sig(200, 1, 1, 1)
        }
    }

    #[test]
    fn parses_status_ranges_and_all() {
        let spec = StatusSpec::parse("200,204,400-499").unwrap();
        assert!(spec.matches(200));
        assert!(spec.matches(404));
        assert!(!spec.matches(500));
        assert!(StatusSpec::parse("all").unwrap().matches(599));
    }

    #[test]
    fn filters_size_words_and_lines() {
        let matcher = MatcherConfig {
            filter_size: Some(NumberSpec::parse("100-200").unwrap()),
            filter_words: Some(NumberSpec::parse("10").unwrap()),
            filter_lines: Some(NumberSpec::parse("1-2").unwrap()),
            ..MatcherConfig::default()
        };
        assert!(!matcher.should_output(&sig(200, 150, 1, 3), ""));
        assert!(!matcher.should_output(&sig(200, 99, 10, 3), ""));
        assert!(!matcher.should_output(&sig(200, 99, 9, 2), ""));
        assert!(matcher.should_output(&sig(200, 99, 9, 3), ""));
    }

    #[test]
    fn matches_time_and_respects_and_mode() {
        let matcher = MatcherConfig {
            match_status: Some(StatusSpec::parse("200").unwrap()),
            match_time: Some(TimeSpec::parse(">100").unwrap()),
            matcher_mode: SetMode::And,
            ..MatcherConfig::default()
        };
        assert!(matcher.should_output(&timed_sig(150), ""));
        assert!(!matcher.should_output(&timed_sig(50), ""));
    }
}
