//! Linux 输入法的 Rust 部分：装配 Engine、按键分派、生成要绘制的帧。
//!
//! fcitx5 的输入法前端是 C++ addon（实现 `fcitx5::InputMethodEngine`），没有稳定的纯 Rust 写法。
//! 这里编译成 `cdylib`，导出一组 C ABI 函数；C++ shim（`shim/`）实现 fcitx5 接口、调这里的 ABI、
//! 把返回的 [`Frame`] 翻译成 fcitx5 `InputPanel` 的 preedit 与候选表。
//!
//! Engine 在 fcitx5 进程内（与 macOS 的 IMK 同进程模式一致），不需要 Windows 那套 IPC。
//! 按键分派逻辑与 macOS 壳的 `handle_text` / `handle_command`、Windows 壳的 `dispatch::key` 对齐：
//! 平台层只做「把系统按键翻译成 Core 的输入，把 Core 返回的帧画出来」，不碰排序 / 词库 / 翻译。

pub(crate) mod assembly;
mod dispatch;
mod error;
mod frame;
mod paths;
mod session;

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::sync::OnceLock;

use crate::assembly::Host;
use crate::dispatch::KeyResult;
use crate::dispatch::key::KeyInput;
use crate::frame::Frame;

// 进程级单例：一个 Engine + 当前会话的分派状态。fcitx5 的 IM engine 在 fcitx5 进程里跑，
// 同一时刻只有一个应用有键盘焦点，所以一个 Engine 持当前组句（与 macOS / Windows 一致）。
// 定义在 [`assembly`] 模块，这里只引用。
static HOST: OnceLock<std::sync::Mutex<Host>> = OnceLock::new();

/// 拿到进程级单例的可变借用，在闭包里操作。未初始化时返回 `None`。
fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.get()?.lock().ok().map(|mut host| f(&mut host))
}

/// 初始化：装配 Engine 并装进单例。fcitx5 加载 addon 时由 shim 调一次。
/// 返回 0 成功，非 0 是错误码（见 [`ErrorCode`]）。
#[unsafe(no_mangle)]
pub extern "C" fn qingjian_init() -> c_int {
    match assembly::init() {
        Ok(host) => {
            let _ = HOST.set(std::sync::Mutex::new(host));
            0
        }
        Err(error) => {
            tracing::error!(%error, "青简 Linux 初始化失败");
            ErrorCode::from(error).to_c_int()
        }
    }
}

/// 释放单例。fcitx5 卸载 addon 时调；之后若再用要先 `qingjian_init`。
#[unsafe(no_mangle)]
pub extern "C" fn qingjian_shutdown() {
    // OnceLock 没法取走，只能让它空着；进程退出时自然释放。
    // 真正的落盘在 focus_out / 定时 flush 里已经做过。
    if let Some(mutex) = HOST.get()
        && let Ok(mut host) = mutex.lock()
    {
        host.engine.flush_learning();
    }
}

/// 处理一次按键。返回 [`KeyResult`] 的 C 表示：
/// - `0` = 放行给应用（`Passthrough`）
/// - `1` = 已消费（进了组句或触发了上屏 / 翻页），shim 要按 `out_frame` 更新 InputPanel
/// - `2` = 已消费且产生了要上屏的文本（`out_commit` 非空），shim 先 commit 文本再按 `out_frame` 更新
///
/// `out_frame` 指向 shim 提供的、可容纳一帧的缓冲；为空帧时表示收起候选窗。
/// `out_commit` 指向 shim 提供的、足够大的缓冲，写入以 `\0` 结尾的 UTF-8 文本（无上屏文本时写空串）。
/// # Safety
/// `app` 可为空指针；`out_frame` 与 `out_commit` 要指向足够大的有效内存（`commit_len` 是 `out_commit` 容量），
/// 调用方用完 `out_frame` 指向的帧后要调 [`qingjian_frame_free`] 归还字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qingjian_key_event(
    sym: u32,
    code: u32,
    modifiers: u32,
    character: u32,
    is_release: bool,
    app: *const c_char,
    out_frame: *mut Frame,
    out_commit: *mut c_char,
    commit_len: usize,
) -> c_int {
    let app = unsafe { app_nullable(app) };
    let input = KeyInput {
        sym,
        code,
        modifiers,
        character: char::from_u32(character),
        is_release,
        app,
    };
    let result = with_host(|host| host.dispatch.key_event(&mut host.engine, &input));
    let Some(result) = result else {
        return KeyResult::Passthrough.to_c_int();
    };
    unsafe { write_frame(out_frame, result.frame) };
    unsafe { write_commit(out_commit, commit_len, &result.commit) };
    result.outcome.to_c_int()
}

/// 焦点离开当前输入上下文：把缓冲原样上屏、收起候选窗、落盘学习数据。
/// `out_commit` 同 `qingjian_key_event`。
/// # Safety
/// `out_commit` 要指向容量 `commit_len` 的有效内存。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qingjian_focus_out(out_commit: *mut c_char, commit_len: usize) -> c_int {
    let result = with_host(|host| host.dispatch.focus_out(&mut host.engine));
    if let Some(result) = result {
        unsafe { write_commit(out_commit, commit_len, &result.commit) };
        result.outcome.to_c_int()
    } else {
        KeyResult::Passthrough.to_c_int()
    }
}

/// 重置当前组句（不落盘）：清空缓冲、收起候选窗。用于应用要求立刻结束输入。
/// # Safety
/// `out_commit` 要指向容量 `commit_len` 的有效内存。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qingjian_reset(out_commit: *mut c_char, commit_len: usize) -> c_int {
    let result = with_host(|host| host.dispatch.reset(&mut host.engine));
    if let Some(result) = result {
        unsafe { write_commit(out_commit, commit_len, &result.commit) };
        result.outcome.to_c_int()
    } else {
        KeyResult::Passthrough.to_c_int()
    }
}

/// 轮询异步结果（云联想、释义兜底、神经重排）。fcitx5 主循环空闲时调，返回非空帧时 shim 重画候选窗。
/// 没有更新时 `out_frame` 写成空帧。
/// # Safety
/// `out_frame` 要指向有效的 `Frame` 内存；调用方用完要调 [`qingjian_frame_free`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qingjian_poll(out_frame: *mut Frame) -> c_int {
    let frame = with_host(|host| host.dispatch.poll(&mut host.engine));
    if let Some(Some(frame)) = frame {
        unsafe { write_frame(out_frame, frame) };
        1
    } else {
        0
    }
}

/// 取当前应该显示的帧（不触发任何计算）。shim 在 InputContext 重建、刷新显示时调。
/// # Safety
/// `out_frame` 要指向有效的 `Frame` 内存；调用方用完要调 [`qingjian_frame_free`]。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qingjian_current_frame(out_frame: *mut Frame) -> c_int {
    let frame = with_host(|host| host.dispatch.current_frame(&host.engine));
    if let Some(frame) = frame {
        unsafe { write_frame(out_frame, frame) };
        1
    } else {
        0
    }
}

/// 释放 shim 分配的 Frame 内部字符串。Frame 里的字符串是 Rust 侧 `CString::into_raw` 出来的，
/// shim 用完要调这个归还，否则泄漏。
/// # Safety
/// `frame` 要指向 [`qingjian_key_event`] / [`qingjian_poll`] / [`qingjian_current_frame`] 写入的帧，
/// 或空指针。释放后不要再访问。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qingjian_frame_free(frame: *mut Frame) {
    if frame.is_null() {
        return;
    }
    unsafe { (*frame).free_strings() };
}

/// 把 Rust 的 `Frame` 写进 shim 提供的缓冲（move 语义）。Frame 里的字符串通过 `CString::into_raw` 交出所有权，
/// shim 用完调 [`qingjian_frame_free`] 归还。
unsafe fn write_frame(out: *mut Frame, frame: Frame) {
    if out.is_null() {
        // 没地方写，就地释放避免泄漏
        let mut frame = frame;
        frame.free_strings();
        return;
    }
    unsafe {
        // 先释放 shim 那一帧里可能残留的字符串（上一帧没释放的防御）
        (*out).free_strings();
        *out = frame;
    }
}

/// 把上屏文本写进 shim 提供的缓冲（`\0` 结尾）。
unsafe fn write_commit(out: *mut c_char, len: usize, text: &str) {
    if out.is_null() || len == 0 {
        return;
    }
    let bytes = text.as_bytes();
    let copy = bytes.len().min(len.saturating_sub(1));
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, copy);
        *out.add(copy) = 0;
    }
}

/// 把 C 字符串指针转成 `Option<String>`；空指针或读失败返回 `None`。
unsafe fn app_nullable(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}

/// 错误码。
#[repr(i32)]
#[derive(Clone, Copy)]
enum ErrorCode {
    Other = 1,
}

impl ErrorCode {
    fn from(error: error::InitError) -> Self {
        tracing::error!(%error, "初始化错误");
        Self::Other
    }

    fn to_c_int(self) -> c_int {
        self as c_int
    }
}
