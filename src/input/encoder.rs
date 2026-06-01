use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use base64::Engine;
use sha1::{Digest, Sha1};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Encoder {
    UrlEncode,
    Base64,
    Hex,
    Lower,
    Upper,
    Md5,
    Sha1,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncoderSet {
    chains: BTreeMap<String, Vec<Encoder>>,
}

impl EncoderSet {
    pub fn parse(raw: &[String]) -> Result<Self> {
        let mut chains = BTreeMap::new();
        for item in raw {
            let (keyword, chain) = item
                .split_once(':')
                .ok_or_else(|| anyhow!("-enc must use KEYWORD:encoder syntax"))?;
            let encoders = chain
                .split_whitespace()
                .map(Encoder::parse)
                .collect::<Result<Vec<_>>>()?;
            chains.insert(keyword.to_string(), encoders);
        }
        Ok(Self { chains })
    }

    pub fn apply_to_map(
        &self,
        values: &crate::template::render::InputMap,
    ) -> crate::template::render::InputMap {
        let mut encoded = values.clone();
        for (keyword, encoders) in &self.chains {
            if let Some(value) = values.get(keyword) {
                encoded.insert(keyword.clone(), apply_chain(value, encoders));
            }
        }
        encoded
    }
}

impl Encoder {
    fn parse(raw: &str) -> Result<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "urlencode" | "url" => Ok(Self::UrlEncode),
            "b64encode" | "base64" | "b64" => Ok(Self::Base64),
            "hex" | "hexencode" => Ok(Self::Hex),
            "lower" | "lowercase" => Ok(Self::Lower),
            "upper" | "uppercase" => Ok(Self::Upper),
            "md5" => Ok(Self::Md5),
            "sha1" => Ok(Self::Sha1),
            other => Err(anyhow!("unsupported encoder '{}'", other)),
        }
    }
}

fn apply_chain(value: &str, encoders: &[Encoder]) -> String {
    encoders
        .iter()
        .fold(value.to_string(), |current, encoder| match encoder {
            Encoder::UrlEncode => {
                url::form_urlencoded::byte_serialize(current.as_bytes()).collect()
            }
            Encoder::Base64 => base64::engine::general_purpose::STANDARD.encode(current),
            Encoder::Hex => hex::encode(current.as_bytes()),
            Encoder::Lower => current.to_ascii_lowercase(),
            Encoder::Upper => current.to_ascii_uppercase(),
            Encoder::Md5 => format!("{:x}", md5::compute(current.as_bytes())),
            Encoder::Sha1 => {
                let mut hasher = Sha1::new();
                hasher.update(current.as_bytes());
                hex::encode(hasher.finalize())
            }
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn applies_common_encoder_chain() {
        let encoders = EncoderSet::parse(&["FUZZ:urlencode b64encode".to_string()]).unwrap();
        let values = BTreeMap::from([("FUZZ".to_string(), "a b".to_string())]);
        let encoded = encoders.apply_to_map(&values);
        assert_eq!(encoded["FUZZ"], "YSti");
    }
}
