//! 双拼解出来的东西：键 → 拼音，让引擎在解码后复用全拼的切分与整句。

pub(super) enum EngineDecoded {
    Shuangpin(crate::shuangpin::Decoded),
}

impl EngineDecoded {
    pub fn pinyin(&self) -> &str {
        match self {
            Self::Shuangpin(d) => d.pinyin(),
        }
    }

    pub fn tail(&self) -> &str {
        match self {
            Self::Shuangpin(d) => d.tail(),
        }
    }

    pub fn segmentation(&self) -> Option<crate::parser::Segmentation> {
        match self {
            Self::Shuangpin(d) => d.segmentation(),
        }
    }

    pub fn marked(&self) -> String {
        match self {
            Self::Shuangpin(d) => d.marked(),
        }
    }

    pub fn keys_for(&self, pinyin_len: usize) -> usize {
        match self {
            Self::Shuangpin(d) => d.keys_for(pinyin_len),
        }
    }

    pub fn is_complete(&self) -> bool {
        match self {
            Self::Shuangpin(d) => d.is_complete(),
        }
    }

    /// 末尾是不是一个落单的声母键（双拼）。
    pub fn pending_initial(&self) -> bool {
        match self {
            Self::Shuangpin(d) => d.pending_initial(),
        }
    }
}
