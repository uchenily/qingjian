//! 当前中 / 英模式与语言栏更新回调，文本服务与语言栏按钮（[`super::ModeButton`]）共享。

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::UI::TextServices::{ITfLangBarItemSink, TF_LBI_ICON, TF_LBI_STATUS};

/// 当前中英模式 + 语言栏更新回调，文本服务与语言栏按钮共享（STA 单线程）。
pub(crate) struct ModeState {
    /// `true` 是英文模式。
    english: Cell<bool>,

    /// 上次写转换模式 compartment 是否成功。沉浸式 / AppContainer 进程（开始菜单 / 搜索 / 设置）里
    /// `SetValue` 会失败，此时 compartment 的值不可信，不应让它反向同步把青简切走（见
    /// [`sync_from_conversion_mode`](crate::com::service::TextService_Impl::sync_from_conversion_mode)）。
    compartment_ok: Cell<bool>,

    /// 系统登记进来的语言栏更新回调；由 [`super::ModeButton`] 的 `ITfSource` 登记 / 撤销。
    pub(super) sink: RefCell<Option<ITfLangBarItemSink>>,
}

impl ModeState {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            english: Cell::new(false),
            compartment_ok: Cell::new(true),
            sink: RefCell::new(None),
        })
    }

    pub(crate) fn english(&self) -> bool {
        self.english.get()
    }

    pub(crate) fn set_english(&self, english: bool) {
        self.english.set(english);
    }

    /// 上次写转换模式 compartment 是否成功。失败时 compartment 不可信，反向同步要跳过。
    pub(crate) fn compartment_ok(&self) -> bool {
        self.compartment_ok.get()
    }

    pub(crate) fn set_compartment_ok(&self, ok: bool) {
        self.compartment_ok.set(ok);
    }

    /// 通知系统重取图标 / 文字。
    pub(crate) fn notify(&self) {
        if let Some(sink) = self.sink.borrow().as_ref() {
            let _ = unsafe { sink.OnUpdate(TF_LBI_ICON | TF_LBI_STATUS) };
        }
    }
}
