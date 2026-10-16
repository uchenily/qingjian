use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// 主语言或学习语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    /// 中文，目前唯一的主语言。
    Chinese,

    /// 英语，目前唯一的学习语言。
    English,
}

impl Language {
    /// ISO 639-1 代码，用于配置与数据文件名。
    pub fn code(self) -> &'static str {
        match self {
            Self::Chinese => "zh",
            Self::English => "en",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown language code: {0}")]
pub struct UnknownLanguage(pub String);

impl FromStr for Language {
    type Err = UnknownLanguage;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "zh" | "zh-cn" | "chinese" => Ok(Self::Chinese),
            "en" | "english" => Ok(Self::English),
            other => Err(UnknownLanguage(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_codes_case_insensitively() {
        assert_eq!("en".parse::<Language>().unwrap(), Language::English);
        assert_eq!("EN".parse::<Language>().unwrap(), Language::English);
        assert_eq!(" English ".parse::<Language>().unwrap(), Language::English);
        assert_eq!("zh-cn".parse::<Language>().unwrap(), Language::Chinese);
    }

    #[test]
    fn rejects_unknown_codes() {
        let error = "ja".parse::<Language>().unwrap_err();
        assert_eq!(error.0, "ja");
    }

    #[test]
    fn codes_are_iso_639_1() {
        assert_eq!(Language::English.code(), "en");
        assert_eq!(Language::Chinese.code(), "zh");
    }
}
