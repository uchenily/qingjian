use serde::{Deserialize, Serialize};

use super::part_of_speech::PartOfSpeech;

/// 一条释义：词性 + 目标语言文本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sense {
    /// 该释义下的词性。数据源没有词性信息时为 `None`（如 CC-CEDICT），候选框只显示译文。
    pub part_of_speech: Option<PartOfSpeech>,

    /// 学习语言的译文，短语级别，不是整段解释。
    pub text: String,

    /// 生词：这条译词用户在候选里还没见过几轮（`Engine::annotate` 按词汇记录填，释义表里恒为 false），壳可以标出来。
    #[serde(default)]
    pub fresh: bool,
}
