// 青简输入法的 fcitx5 addon：实现 fcitx::InputMethodEngine，把按键转给 Rust cdylib，
// 把 Rust 返回的帧翻译成 fcitx5 InputPanel 的 preedit 与候选表。
#pragma once

#include <fcitx/inputmethodengine.h>
#include <fcitx/instance.h>
#include <fcitx-utils/event.h>
#include <fcitx-utils/trackableobject.h>

#include "abi.h"

#include <string>

namespace qingjian {

/// qingjian_poll 的轮询间隔（微秒），与 Rust 侧 `rescore::POLL_INTERVAL`（20ms）对齐。
constexpr uint64_t POLL_INTERVAL_USEC = 20000;

/// fcitx5 输入法引擎。Rust 侧是进程级单例（一个 Engine 持当前组句），所以这里不持组句状态，
/// 只做「fcitx5 事件 → C ABI → fcitx5 InputPanel」的翻译。
///
/// 会话生命周期：fcitx5 的 InputContext 随时可能失焦、销毁、切输入法（锁屏 / 切窗口 / 密码框）。
/// 这里用弱引用（`TrackableObjectReference`）追踪当前 IC，并监听 FocusOut / Destroyed /
/// CapabilityChanged 事件，及时清组句、停轮询、放行密码字段，避免对已失效的 IC 操作。
class QingjianEngine : public fcitx::InputMethodEngine {
public:
    QingjianEngine(fcitx::Instance* instance);
    ~QingjianEngine() override;

    /// fcitx5 激活本输入法时调。
    void activate(const fcitx::InputMethodEntry& entry,
                  fcitx::InputContextEvent& event) override;

    /// fcitx5 停用本输入法时调：清缓冲、落盘、收起候选窗。
    void deactivate(const fcitx::InputMethodEntry& entry,
                    fcitx::InputContextEvent& event) override;

    /// 按键。核心入口：转给 Rust，按返回值更新 InputPanel 或 commit。
    void keyEvent(const fcitx::InputMethodEntry& entry,
                  fcitx::KeyEvent& keyEvent) override;

    /// 输入上下文被销毁 / 失焦时调：通知 Rust 落盘并清缓冲。
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

    /// 焦点离开：调 Rust 侧 qingjian_focus_out 清组句、落盘，收起候选窗，清当前 IC 弱引用。
    void handleFocusOut(fcitx::InputContext* ic);

    /// 输入上下文销毁：若它是当前 IC，清弱引用、停轮询。
    void handleDestroyed(fcitx::InputContext* ic);

    /// 能力变化（普通框 ↔ 密码框 ↔ 禁用）：密码 / 禁用字段放行，清组句。
    void handleCapabilityChanged(fcitx::InputContext* ic);

    /// 当前 IC 是否是密码 / 禁用字段（不该经过输入法）。
    static bool isPrivateField(fcitx::InputContext* ic);

    fcitx::Instance* instance_;
    /// 驱动本地整句模型重排的重复定时器。
    std::unique_ptr<fcitx::EventSourceTime> poll_timer_;
    /// 最近一次按键的 InputContext 的弱引用。重排定时器用它重画；IC 销毁后自动失效。
    fcitx::TrackableObjectReference<fcitx::InputContext> current_ic_;
    /// 事件监听器（FocusOut / Destroyed / CapabilityChanged），析构时自动解绑。
    /// watchEvent 返回 unique_ptr<HandlerTableEntry<EventHandler>>，删除即解绑。
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>> focus_out_watcher_;
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>> destroyed_watcher_;
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>> capability_watcher_;
};

} // namespace qingjian
