//! 候选窗口的一行：序号、候选词、annotation 片段。只是 Core 输出的展示形态，不含任何排序或查词。

use qingjian_core::Candidate;

/// annotation 片段的深浅。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// 译文。
    Gloss,

    /// 生词的译文（用户还没在候选里见过几轮，`Sense::fresh`），用强调色。
    Fresh,

    /// 词性与分隔符，最浅。
    Faint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 显示用序号文本，如 `1`。
    pub index: String,

    /// 候选词。
    pub text: String,

    /// 右侧 annotation，按顺序绘制；没有译文时为空。
    pub annotation: Vec<(String, Tone)>,
}

impl Row {
    pub fn from_candidate(position: usize, candidate: &Candidate) -> Self {
        let mut annotation = Vec::new();
        // emoji 候选来自哪个词，放在最前
        if let Some(reading) = &candidate.reading {
            annotation.push((reading.clone(), Tone::Gloss));
        }
        if let Some(translation) = &candidate.translation {
            for (i, sense) in translation.senses().iter().enumerate() {
                if i > 0 || !annotation.is_empty() {
                    annotation.push((" · ".to_owned(), Tone::Faint));
                }
                if let Some(pos) = sense.part_of_speech {
                    annotation.push((format!("{pos} "), Tone::Faint));
                }
                let tone = if sense.fresh {
                    Tone::Fresh
                } else {
                    Tone::Gloss
                };
                annotation.push((sense.text.clone(), tone));
            }
        }
        Self {
            index: (position + 1).to_string(),
            text: if matches!(candidate.kind, qingjian_core::CandidateKind::Custom(_)) {
                qingjian_core::CustomPhrase::preview(&candidate.text, 60)
            } else {
                candidate.text.clone()
            },
            annotation,
        }
    }
}
