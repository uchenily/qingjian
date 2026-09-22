use serde::{Deserialize, Serialize};

use crate::shortcut::EXPRESSION_PREFIX;

/// 前缀模式键，配置文件 `[shortcut]` 分节。
///
/// 搜狗 / 微软那一家的做法：用不能开头拼任何音节的字母（`v` `u` `i`）一键进模式，
/// 不要修饰键，中文模式下零冲突。缺省 `v` 表达式（四则运算、中文数字）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeKeys {
    /// 表达式模式前缀：`v1+2`、`v123`。
    pub expression: char,
}

impl Default for ModeKeys {
    fn default() -> Self {
        Self {
            expression: EXPRESSION_PREFIX,
        }
    }
}

impl ModeKeys {
    /// 能当模式键的字母：不是任何拼音音节的开头。
    pub const CANDIDATES: [char; 3] = ['v', 'u', 'i'];

    /// 没有字母模式键：双拼下 v / u / i 都是音节键。
    pub const LETTERLESS: Self = Self {
        expression: '\0',
    };

    /// 表达式键是合法的候选字母。
    pub fn is_valid(&self) -> bool {
        Self::CANDIDATES.contains(&self.expression)
    }

    /// 非法配置退回缺省。
    pub fn sanitized(self) -> Self {
        if self.is_valid() {
            self
        } else {
            Self::default()
        }
    }

    pub fn is_expression(&self, input: &str, zhuyin: bool) -> bool {
        input.starts_with(self.expression)
            && (!zhuyin || crate::zhuyin::layout::map_key(self.expression).is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_v() {
        let keys = ModeKeys::default();
        assert!(keys.is_expression("v12", false));
        assert!(!keys.is_expression("nihao", false));
    }

    #[test]
    fn invalid_falls_back_to_defaults() {
        let pinyin_initial = ModeKeys { expression: 'z' };
        assert!(!pinyin_initial.is_valid());
        assert_eq!(pinyin_initial.sanitized(), ModeKeys::default());
    }

    #[test]
    fn deserializes_from_single_character_strings() {
        let keys: ModeKeys = toml::from_str("expression = \"i\"\n").unwrap();
        assert_eq!(keys.expression, 'i');
    }
}
