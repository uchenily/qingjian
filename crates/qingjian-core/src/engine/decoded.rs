//! 输入串解出来的东西：双拼键 → 拼音，全拼就是自身。让引擎在解码后复用同一套切分与整句逻辑，
//! 不必到处判 `shuangpin.is_some()`。

pub(super) enum EngineDecoded {
    Shuangpin(crate::shuangpin::Decoded),
    /// 全拼：`pinyin` 就是键串本身。切分仍由查询层的 `parser::segment` / `segment_longest_prefix` 处理，
    /// 所以 `segmentation()` 返回 `None`、`is_complete()` 按 `is_fully_segmentable` 判断。
    Pinyin { pinyin: String, complete: bool },
}

impl EngineDecoded {
    pub fn pinyin(&self) -> &str {
        match self {
            Self::Shuangpin(d) => d.pinyin(),
            Self::Pinyin { pinyin, .. } => pinyin,
        }
    }

    #[allow(dead_code)]
    pub fn tail(&self) -> &str {
        match self {
            Self::Shuangpin(d) => d.tail(),
            // 全拼的 tail 由调用方（segmentation + 剩余）处理，这里返回空
            Self::Pinyin { .. } => "",
        }
    }

    pub fn segmentation(&self) -> Option<crate::parser::Segmentation> {
        match self {
            Self::Shuangpin(d) => d.segmentation(),
            // 全拼能完整切分时返回第一条切分（与双拼的唯一切分对齐）
            Self::Pinyin { pinyin, complete } if *complete => {
                crate::parser::segment(pinyin).ok().and_then(|s| s.into_iter().next())
            }
            Self::Pinyin { .. } => None,
        }
    }

    /// 多条切分与尾段：双拼无歧义只有一条，全拼用 `parser::segment`。切不动时返回 `Err`，
    /// 由调用方决定 fallback（如 `segment_longest_prefix`）。
    pub fn segmentations(&self) -> Result<(Vec<crate::parser::Segmentation>, String), crate::parser::ParseError> {
        match self {
            Self::Shuangpin(d) => d
                .segmentation()
                .map(|s| (vec![s], d.tail().to_owned()))
                .ok_or(crate::parser::ParseError::NoSegmentation),
            Self::Pinyin { pinyin, .. } => {
                crate::parser::segment(pinyin).map(|s| (s, String::new()))
            }
        }
    }

    pub fn marked(&self) -> String {
        match self {
            Self::Shuangpin(d) => d.marked(),
            // 全拼的 marked 由 `join_marked` / `marked_rest` 处理，这里返回 pinyin 原样
            Self::Pinyin { pinyin, .. } => pinyin.clone(),
        }
    }

    pub fn keys_for(&self, pinyin_len: usize) -> usize {
        match self {
            Self::Shuangpin(d) => d.keys_for(pinyin_len),
            // 全拼键与拼音一一对应
            Self::Pinyin { .. } => pinyin_len,
        }
    }

    pub fn is_complete(&self) -> bool {
        match self {
            Self::Shuangpin(d) => d.is_complete(),
            Self::Pinyin { complete, .. } => *complete,
        }
    }

    /// 末尾是不是一个落单的声母键（双拼）。全拼没有这个概念，恒为假。
    pub fn pending_initial(&self) -> bool {
        match self {
            Self::Shuangpin(d) => d.pending_initial(),
            Self::Pinyin { .. } => false,
        }
    }
}
