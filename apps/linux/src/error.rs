//! Linux 壳的错误类型。

use thiserror::Error;

/// 初始化阶段（装配 Engine）的错误。
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum InitError {
    #[error("找不到数据目录（设 QINGJIAN_DATA_DIR 指向仓库根，或把数据装到 /usr/share/qingjian）")]
    NoDataDir,

    #[error("词库加载失败：{0}")]
    Dictionary(String),

    #[error("语言模型加载失败：{0}")]
    LanguageModel(String),

    #[error("释义表加载失败：{0}")]
    Glossary(String),

    #[error("英文词表加载失败：{0}")]
    English(String),

    #[error("学习数据目录建不起来")]
    UserDir,
}

impl InitError {
    /// 从词库错误转换，保留原始信息。
    pub fn from_dictionary(error: impl std::fmt::Display) -> Self {
        Self::Dictionary(error.to_string())
    }
}
