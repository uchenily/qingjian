//! 候选窗口一次绘制的全部内容。窗口记住上一帧，本地整句模型结果到了只改一处再重画。

use super::preedit::Preedit;
use super::row::Row;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frame {
    /// 顶部拼音行；配置成只在行内显示时为 `None`。
    pub preedit: Option<Preedit>,

    /// 候选行。
    pub rows: Vec<Row>,

    /// 高亮行下标；不想高亮任何行就给 `usize::MAX`。
    pub highlighted: usize,

    /// 右下角页码。
    pub footer: Option<String>,

    /// 拼音行右侧的一句临时状态（删了什么词）。
    pub status: Option<String>,
}

impl Frame {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.preedit.is_none() && self.trailing().is_none()
    }

    /// 顶部要不要画一行（拼音或右侧文字任一存在）。
    pub fn has_top_line(&self) -> bool {
        self.preedit.is_some() || self.trailing().is_some()
    }

    /// 拼音行右侧画什么：只有状态。
    pub fn trailing(&self) -> Option<&str> {
        self.status.as_deref()
    }
}
