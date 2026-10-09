// 青简输入法的 fcitx5 addon：实现 fcitx::InputMethodEngine，把按键转给 Rust cdylib，
// 把 Rust 返回的帧翻译成 fcitx5 InputPanel 的 preedit 与候选表。
#pragma once

#include <fcitx/inputmethodengine.h>
#include <fcitx/instance.h>
#include <fcitx-utils/event.h>

#include "abi.h"

#include <string>

namespace qingjian {

/// qingjian_poll 的轮询间隔（微秒），与 Rust 侧 `rescore::POLL_INTERVAL`（20ms）对齐。
constexpr uint64_t POLL_INTERVAL_USEC = 20000;

/// fcitx5 输入法引擎。每个 InputContext 对应一次按键调用；Rust 侧是进程级单例，
/// 所以这里不持状态，只做「fcitx5 事件 → C ABI → fcitx5 InputPanel」的翻译。
class QingjianEngine : public fcitx::InputMethodEngine {
public:
    QingjianEngine(fcitx::Instance* instance);
    ~QingjianEngine() override;

    /// fcitx5 激活本输入法时调。
    void activate(const fcitx::InputMethodEntry& entry,
                  fcitx::InputContextEvent& event) override;

    /// fcitx5 停用本输入法时调：重置 Rust 侧缓冲。
    void deactivate(const fcitx::InputMethodEntry& entry,
                    fcitx::InputContextEvent& event) override;

    /// 按键。核心入口：转给 Rust，按返回值更新 InputPanel 或 commit。
    void keyEvent(const fcitx::InputMethodEntry& entry,
                  fcitx::KeyEvent& keyEvent) override;

    /// 输入上下文被销毁 / 失焦时调：通知 Rust 落盘。
    void reset(const fcitx::InputMethodEntry& entry,
               fcitx::InputContextEvent& event) override;

private:
    /// 把 Rust 返回的帧翻译成 fcitx5 InputPanel 的 preedit + 候选表。
    void paintFrame(fcitx::InputContext* ic, const QingjianFrame& frame);

    /// 把上屏文本 commit 给应用。
    void commitText(fcitx::InputContext* ic, const char* text);

    /// 收起候选窗。
    void hidePanel(fcitx::InputContext* ic);

    /// 定时器到点：调 qingjian_poll，有非空帧就重画当前候选窗。
    /// 返回 qingjian_poll 的值：1=有帧、0=继续轮询、-1=闲时（已停定时器）。
    int32_t onPollTick();

    fcitx::Instance* instance_;
    /// 驱动本地整句模型重排的重复定时器。
    std::unique_ptr<fcitx::EventSourceTime> poll_timer_;
    /// 最近一次按键的 InputContext，重排定时器用它重画。
    fcitx::InputContext* current_ic_ = nullptr;
};

} // namespace qingjian
