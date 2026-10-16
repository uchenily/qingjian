//! 组句起始时读应用光标前的文字，给本地整句模型当前文（对应 macOS 壳的 `surrounding_text`）。
//! 在起组句的那次读写会话里做（此时选区还是原来的插入点，拼音还没插进去），不另开会话。

use std::mem::ManuallyDrop;

use windows::Win32::UI::TextServices::{
    ITfContext, ITfRange, TF_ANCHOR_START, TF_DEFAULT_SELECTION, TF_SELECTION,
};

/// 往前读多少字（与 macOS 壳的 `RESCORE_LOOKBACK` 一致）。
const LOOKBACK: i32 = 64;

/// 起组句时对输入框的判断：光标前的文字（读不到 / 没有选区时为 `None`）。
pub(crate) struct InputContext {
    /// 当前选区起点之前最多 [`LOOKBACK`] 个 UTF-16 单元的文本。
    pub(crate) before: Option<String>,
}

/// 起组句时读一次光标前文。
pub(crate) fn input_context(context: &ITfContext, ec: u32) -> InputContext {
    let Some(range) = selection_start(context, ec) else {
        return InputContext { before: None };
    };
    InputContext {
        before: text_before_caret(context, ec, range),
    }
}

/// `range`（已折成插入点）之前最多 [`LOOKBACK`] 个 UTF-16 单元的文本；读不到 / 为空是 `None`。
fn text_before_caret(context: &ITfContext, ec: u32, range: ITfRange) -> Option<String> {
    let _ = context;
    let mut shifted = 0i32;
    unsafe { range.ShiftStart(ec, -LOOKBACK, &mut shifted, std::ptr::null()) }.ok()?;
    if shifted == 0 {
        return None;
    }
    let mut buf = [0u16; LOOKBACK as usize];
    let mut fetched = 0u32;
    unsafe { range.GetText(ec, 0, &mut buf, &mut fetched) }.ok()?;
    let text = String::from_utf16_lossy(&buf[..fetched as usize]);
    (!text.is_empty()).then_some(text)
}

/// 选区折成起点（插入点）。
fn selection_start(context: &ITfContext, ec: u32) -> Option<ITfRange> {
    let mut selection = [TF_SELECTION::default()];
    let mut fetched = 0u32;
    unsafe {
        context
            .GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)
            .ok()?;
    }
    if fetched == 0 {
        return None;
    }
    // GetSelection 移交 range 的所有权（ManuallyDrop），取出后由这里释放。
    let range = unsafe { ManuallyDrop::take(&mut selection[0].range) }?;
    unsafe { range.Collapse(ec, TF_ANCHOR_START) }.ok()?;
    Some(range)
}
