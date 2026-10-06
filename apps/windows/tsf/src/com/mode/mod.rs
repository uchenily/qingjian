//! 中 / 英输入模式：这里写 TSF 的「转换模式」compartment，系统任务栏据此显示「中」或「英」，
//! 只翻 `TF_CONVERSIONMODE_NATIVE` 位，其它位（全 / 半角等）保留。读回（反向同步）在 [`conversion`]；
//! 模式本身与语言栏按钮在 [`state`] / [`button`]。

mod button;
pub(crate) mod conversion;
mod state;

use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::TextServices::{
    GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION, ITfCompartment, ITfCompartmentMgr,
    ITfThreadMgr, TF_CONVERSIONMODE_NATIVE,
};
use windows::core::{Interface, Result};

pub(crate) use self::button::ModeButton;
pub(crate) use self::state::ModeState;

const NATIVE: i32 = TF_CONVERSIONMODE_NATIVE as i32;

/// 写转换模式 compartment（任务栏中 / 英指示器）。返回成功与否：沉浸式 / AppContainer 进程里
/// `SetValue` 会失败（`0x8000FFFF`），调用方据此决定要不要信任后续的 compartment 反向同步。
pub(crate) fn set_indicator(thread_mgr: &ITfThreadMgr, tid: u32, english: bool) -> Result<()> {
    write_conversion_mode(thread_mgr, tid, english)
}

pub(crate) fn conversion_compartment(thread_mgr: &ITfThreadMgr) -> Result<ITfCompartment> {
    let mgr: ITfCompartmentMgr = thread_mgr.cast()?;
    unsafe { mgr.GetCompartment(&GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION) }
}

/// `NATIVE` 未点亮 = 英文。读不到（没设过 / 类型不对）按中文起算：compartment 还没被任何 TIP
/// 写过时不应误判成英文。
pub(crate) fn is_english(compartment: &ITfCompartment) -> bool {
    read_mode(compartment) & NATIVE == 0
}

fn write_conversion_mode(thread_mgr: &ITfThreadMgr, tid: u32, english: bool) -> Result<()> {
    let compartment = conversion_compartment(thread_mgr)?;
    let current = read_mode(&compartment);
    let next = if english {
        current & !NATIVE
    } else {
        current | NATIVE
    };
    unsafe { compartment.SetValue(tid, &VARIANT::from(next)) }
}

/// 没设过 / 类型不对时按中文（`NATIVE` 亮）。
fn read_mode(compartment: &ITfCompartment) -> i32 {
    unsafe { compartment.GetValue() }
        .and_then(|variant| i32::try_from(&variant))
        .unwrap_or(NATIVE)
}
