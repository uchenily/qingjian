use serde::{Deserialize, Serialize};

/// 一个中文词的释义：词性、英文译词。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossEntry {
    /// 中文词。
    pub word: String,

    /// 词性缩写（`n.` / `v.` …，Core `PartOfSpeech` 认得的那套）；模型给不出就没有。
    pub pos: Option<String>,

    /// 英文译词，最多两个，按常用度。
    pub en: Vec<String>,
}

impl GlossEntry {
    /// 没给出译词的条目没有用。
    pub fn is_useful(&self) -> bool {
        !self.en.is_empty()
    }
}
