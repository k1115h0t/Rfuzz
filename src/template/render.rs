use std::collections::BTreeMap;

use anyhow::{anyhow, Result};

use super::{Template, TemplatePart};

pub type InputMap = BTreeMap<String, String>;

impl Template {
    pub fn render(&self, input: &InputMap) -> Result<String> {
        let mut rendered = String::new();
        for part in &self.parts {
            match part {
                TemplatePart::Literal(value) => rendered.push_str(value),
                TemplatePart::Placeholder { keyword } => {
                    let value = input
                        .get(keyword)
                        .ok_or_else(|| anyhow!("missing value for keyword '{}'", keyword))?;
                    rendered.push_str(value);
                }
            }
        }
        Ok(rendered)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::template::Template;

    #[test]
    fn renders_native_placeholder() {
        let keywords = BTreeSet::from(["DIR".to_string()]);
        let template = Template::compile("https://example.com/${{DIR}}$", &keywords).unwrap();
        let input = BTreeMap::from([("DIR".to_string(), "admin".to_string())]);
        assert_eq!(
            template.render(&input).unwrap(),
            "https://example.com/admin"
        );
    }
}
