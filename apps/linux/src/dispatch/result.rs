//! 按键处置结果：处置方式（C ABI 返回值）+ 要显示的帧 + 要上屏的文本。

use std::os::raw::c_int;

use crate::frame::Frame;

/// 按键处置结果（C ABI 返回值）。
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    /// 放行给应用。
    Passthrough = 0,
    /// 已消费，按 `out_frame` 更新显示。
    Consumed = 1,
    /// 已消费且有上屏文本，先 commit 再更新显示。
    Committed = 2,
}

impl KeyResult {
    pub fn to_c_int(self) -> c_int {
        self as c_int
    }
}

/// 一次按键的完整结果。
pub struct KeyOutcome {
    /// C ABI 返回值（放行 / 已消费 / 已上屏）。
    pub outcome: KeyResult,
    /// 要显示的帧（空帧表示收起候选窗）。
    pub frame: Frame,
    /// 要上屏的文本（无则空串）。
    pub commit: String,
}

impl KeyOutcome {
    /// 放行给应用，不更新显示。
    pub fn passthrough() -> Self {
        Self {
            outcome: KeyResult::Passthrough,
            frame: Frame::empty(),
            commit: String::new(),
        }
    }

    /// 已消费，更新显示，不上屏。
    pub fn consumed(frame: Frame) -> Self {
        Self {
            outcome: KeyResult::Consumed,
            frame,
            commit: String::new(),
        }
    }

    /// 已消费且有上屏文本，先 commit 再更新显示。
    pub fn committed(commit: String, frame: Frame) -> Self {
        Self {
            outcome: KeyResult::Committed,
            frame,
            commit,
        }
    }
}
