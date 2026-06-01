use std::collections::BTreeSet;

use anyhow::{anyhow, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub parts: Vec<TemplatePart>,
    pub mode: TemplateMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateMode {
    Native,
    Bare,
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplatePart {
    Literal(String),
    Placeholder { keyword: String },
}

impl Template {
    pub fn compile(input: &str, keywords: &BTreeSet<String>) -> Result<Self> {
        if input.contains("${{") {
            let template = parse_native(input)?;
            template.validate_keywords(keywords)?;
            if template.literal_parts_contain_keyword(keywords) {
                tracing::warn!(
                    "template contains native placeholders and bare keywords; native placeholders take precedence"
                );
            }
            Ok(template)
        } else {
            let template = parse_bare(input, keywords);
            template.validate_keywords(keywords)?;
            Ok(template)
        }
    }

    pub fn placeholders(&self) -> BTreeSet<String> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                TemplatePart::Literal(_) => None,
                TemplatePart::Placeholder { keyword } => Some(keyword.clone()),
            })
            .collect()
    }

    pub fn validate_keywords(&self, keywords: &BTreeSet<String>) -> Result<()> {
        for keyword in self.placeholders() {
            if !keywords.contains(&keyword) {
                return Err(anyhow!(
                    "template keyword '{}' has no matching wordlist",
                    keyword
                ));
            }
        }
        Ok(())
    }

    fn literal_parts_contain_keyword(&self, keywords: &BTreeSet<String>) -> bool {
        self.parts.iter().any(|part| match part {
            TemplatePart::Literal(value) => keywords.iter().any(|keyword| value.contains(keyword)),
            TemplatePart::Placeholder { .. } => false,
        })
    }
}

fn parse_native(input: &str) -> Result<Template> {
    let mut parts = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = input[cursor..].find("${{") {
        let start = cursor + relative_start;
        if start > cursor {
            parts.push(TemplatePart::Literal(input[cursor..start].to_string()));
        }

        let keyword_start = start + 3;
        let close = input[keyword_start..]
            .find("}}$")
            .ok_or_else(|| anyhow!("unclosed native placeholder starting at byte {}", start))?;
        let keyword_end = keyword_start + close;
        let keyword = &input[keyword_start..keyword_end];
        if keyword.is_empty() {
            return Err(anyhow!("native placeholder keyword cannot be empty"));
        }
        if !is_valid_keyword(keyword) {
            return Err(anyhow!("invalid keyword '{}'", keyword));
        }
        parts.push(TemplatePart::Placeholder {
            keyword: keyword.to_string(),
        });
        cursor = keyword_end + 3;
    }

    if cursor < input.len() {
        parts.push(TemplatePart::Literal(input[cursor..].to_string()));
    }
    Ok(Template {
        parts,
        mode: TemplateMode::Native,
    })
}

fn parse_bare(input: &str, keywords: &BTreeSet<String>) -> Template {
    let ordered_keywords = {
        let mut values = keywords.iter().cloned().collect::<Vec<_>>();
        values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        values
    };

    let mut parts = Vec::new();
    let mut cursor = 0;
    while cursor < input.len() {
        let next_match = ordered_keywords
            .iter()
            .filter_map(|keyword| {
                input[cursor..]
                    .find(keyword)
                    .map(|pos| (cursor + pos, keyword))
            })
            .min_by(|(left_pos, left_kw), (right_pos, right_kw)| {
                left_pos
                    .cmp(right_pos)
                    .then_with(|| right_kw.len().cmp(&left_kw.len()))
            });

        let Some((start, keyword)) = next_match else {
            parts.push(TemplatePart::Literal(input[cursor..].to_string()));
            break;
        };

        if start > cursor {
            parts.push(TemplatePart::Literal(input[cursor..start].to_string()));
        }
        parts.push(TemplatePart::Placeholder {
            keyword: keyword.clone(),
        });
        cursor = start + keyword.len();
    }

    if parts
        .iter()
        .any(|part| matches!(part, TemplatePart::Placeholder { .. }))
    {
        Template {
            parts,
            mode: TemplateMode::Bare,
        }
    } else {
        Template {
            parts: vec![TemplatePart::Literal(input.to_string())],
            mode: TemplateMode::Static,
        }
    }
}

fn is_valid_keyword(keyword: &str) -> bool {
    let mut chars = keyword.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_uppercase())
        && chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keywords(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_native_template_parts() {
        let template = Template::compile("https://x/${{DIR}}$/file", &keywords(&["DIR"])).unwrap();
        assert_eq!(
            template.parts,
            vec![
                TemplatePart::Literal("https://x/".to_string()),
                TemplatePart::Placeholder {
                    keyword: "DIR".to_string()
                },
                TemplatePart::Literal("/file".to_string())
            ]
        );
    }

    #[test]
    fn supports_bare_keyword_compatibility() {
        let template =
            Template::compile("https://HOST/FUZZ", &keywords(&["HOST", "FUZZ"])).unwrap();
        assert_eq!(template.mode, TemplateMode::Bare);
        assert_eq!(template.placeholders(), keywords(&["HOST", "FUZZ"]));
    }

    #[test]
    fn errors_when_keyword_is_missing() {
        let error = Template::compile("https://x/${{DIR}}$", &keywords(&["FUZZ"])).unwrap_err();
        assert!(error.to_string().contains("DIR"));
    }

    #[test]
    fn rejects_unclosed_native_placeholder() {
        let error = Template::compile("https://x/${{DIR", &keywords(&["DIR"])).unwrap_err();
        assert!(error.to_string().contains("unclosed"));
    }
}
