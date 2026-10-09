//! 本地整句模型的两个节拍：停键后的防抖（到点才把整句路径送去后台打分）与结果轮询。
//! Linux 壳没有自己的定时器，靠 shim 侧 fcitx5 事件循环周期性调 [`crate::qingjian_poll`] 驱动；
//! 常数与 macOS 壳的 `RescoreMonitor`、Windows 壳的 `RescoreState` 相同。

use std::time::{Duration, Instant};

/// 停键多久才请求重排：比一般的击键间隔短，连着敲时不请求。
const DEBOUNCE: Duration = Duration::from_millis(80);

/// 轮询间隔：模型一次几十毫秒。shim 侧的 fcitx5 定时器按这个间隔调 `qingjian_poll`。
#[allow(dead_code)]
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// 最长等多久；后台线程卡住时兜底。
const MAX_WAIT: Duration = Duration::from_secs(2);

/// 重排的进行态。纯 Rust、不依赖平台定时器，由 shim 的轮询驱动 [`Self::advance`]。
#[derive(Debug, Default)]
pub(crate) struct RescoreState {
    /// 最近一次缓冲变化的时间；有它表示在等防抖到点。
    wanted_since: Option<Instant>,

    /// 本轮请求发出的时间；有它表示在等后台结果。
    polling_since: Option<Instant>,
}

impl RescoreState {
    /// 又敲了一键：重新计时。
    pub(crate) fn schedule(&mut self) {
        self.wanted_since = Some(Instant::now());
    }

    /// 什么都不等。
    pub(crate) fn stop(&mut self) {
        self.wanted_since = None;
        self.polling_since = None;
    }

    /// 推进一轮：防抖到点就发请求（返回 `true` 让调用方调 `Engine::request_rescoring`），
    /// 在等结果就收一次（返回 `true` 让调用方调 `Engine::poll_rescoring`）。
    /// 返回 `(should_request, should_poll)`。
    pub(crate) fn advance(&mut self) -> (bool, bool) {
        let mut should_request = false;
        let mut should_poll = false;
        if self.wanted_since.is_some_and(|since| since.elapsed() >= DEBOUNCE) {
            should_request = true;
        }
        if self.polling_since.is_some() {
            should_poll = true;
        }
        (should_request, should_poll)
    }

    /// 请求已发出：开始等结果。
    pub(crate) fn start_polling(&mut self) {
        self.wanted_since = None;
        self.polling_since = Some(Instant::now());
    }

    /// 等结果等太久了。
    pub(crate) fn expired(&self) -> bool {
        self.polling_since
            .is_some_and(|since| since.elapsed() > MAX_WAIT)
    }

    /// 有没有在等（防抖或轮询）；shim 据此决定要不要继续驱动轮询。
    pub(crate) fn active(&self) -> bool {
        self.wanted_since.is_some() || self.polling_since.is_some()
    }
}
