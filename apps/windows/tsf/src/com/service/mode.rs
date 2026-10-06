//! 中 / 英模式：切模式先把组着的内容落定；指示器走语言栏按钮 + 转换模式 compartment，并推给 Server 的状态条；
//! 用户点任务栏中 / 英时由 compartment 回调反向同步。

use windows::Win32::UI::TextServices::ITfLangBarItemMgr;
use windows::core::Interface;

use super::TextService_Impl;
use crate::com::log::log;
use crate::com::mode::{self, ModeButton, conversion};

impl TextService_Impl {
    /// 切模式：先把组着的内容原样落定，再刷指示器。
    pub(super) fn set_english_mode(&self, english: bool) {
        self.commit_pending();
        self.mode_state.set_english(english);
        self.refresh_mode_indicator();
        log(if english {
            "切到英文模式"
        } else {
            "切到中文模式"
        });
    }

    /// 语言栏按钮换图标、写转换模式 compartment、把模式推给 Server（悬浮状态条）。
    /// 写 compartment 失败时记下：沉浸式进程里 compartment 不可信，后续反向同步要跳过，
    /// 否则读回的默认值（`NATIVE` 未亮）会把青简误切成英文。
    pub(super) fn refresh_mode_indicator(&self) {
        let english = self.mode_state.english();
        self.mode_state.notify();
        if let Some(thread_mgr) = self.thread_mgr.borrow().as_ref() {
            match mode::set_indicator(thread_mgr, self.client_id.get(), english) {
                Ok(()) => self.mode_state.set_compartment_ok(true),
                Err(error) => {
                    log(&format!("设置中英指示器失败: {error}"));
                    self.mode_state.set_compartment_ok(false);
                }
            }
        }
        if let Some(client) = self.engine.borrow_mut().as_mut()
            && let Err(error) = client.mode_changed(english)
        {
            log(&format!("上报中英模式失败: {error}"));
        }
    }

    pub(super) fn advise_conversion_sink(&self) {
        let Some(thread_mgr) = self.thread_mgr.borrow().clone() else {
            return;
        };
        match conversion::advise(&thread_mgr) {
            Ok(advice) => *self.conversion_sink.borrow_mut() = Some(advice),
            Err(error) => log(&format!("监听转换模式失败: {error}")),
        }
    }

    pub(super) fn unadvise_conversion_sink(&self) {
        if let Some((source, cookie)) = self.conversion_sink.borrow_mut().take() {
            conversion::unadvise(&source, cookie);
        }
    }

    /// 用户在任务栏点了中 / 英：读回 `NATIVE` 位，与当前不同才切（相同是自己那次写触发的，防回环）。
    /// 写 compartment 失败的进程（开始菜单 / 搜索 / 设置等沉浸式宿主）里 compartment 不可信，
    /// 跳过反向同步——青简自己的 `mode_state` 才是真相，否则会被默认值（英文）误切。
    /// 失焦时系统 / 其他输入法常把 compartment 重置成默认值（英文），这种噪声也不该反向同步
    /// （用户点青简自己的语言栏按钮走 `toggle_mode`，不经这里）。
    pub(super) fn sync_from_conversion_mode(&self) {
        if !self.mode_state.compartment_ok() {
            return;
        }
        if !self.shared.foreground() {
            return;
        }
        let Some(thread_mgr) = self.thread_mgr.borrow().clone() else {
            return;
        };
        let Ok(compartment) = mode::conversion_compartment(&thread_mgr) else {
            return;
        };
        let english = mode::is_english(&compartment);
        if english != self.mode_state.english() {
            self.set_english_mode(english);
        }
    }

    pub(super) fn add_lang_bar_item(&self) {
        let button = ModeButton::create(self.mode_state.clone());
        if let Some(thread_mgr) = self.thread_mgr.borrow().as_ref() {
            match thread_mgr.cast::<ITfLangBarItemMgr>() {
                Ok(mgr) => {
                    if let Err(error) = unsafe { mgr.AddItem(&button) } {
                        log(&format!("登记中英指示器失败: {error}"));
                    }
                }
                Err(error) => log(&format!("取语言栏管理器失败: {error}")),
            }
        }
        *self.mode_button.borrow_mut() = Some(button);
    }

    pub(super) fn remove_lang_bar_item(&self) {
        if let Some(button) = self.mode_button.borrow_mut().take()
            && let Some(thread_mgr) = self.thread_mgr.borrow().as_ref()
            && let Ok(mgr) = thread_mgr.cast::<ITfLangBarItemMgr>()
        {
            let _ = unsafe { mgr.RemoveItem(&button) };
        }
    }
}
