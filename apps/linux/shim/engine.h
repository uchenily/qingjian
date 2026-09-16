// 青简输入法的 fcitx5 addon：实现 fcitx::InputMethodEngine，把按键转给 Rust cdylib，
// 把 Rust 返回的帧翻译成 fcitx5 InputPanel 的 preedit 与候选表。
#pragma once

#include <fcitx/inputmethodengine.h>
#include <fcitx/instance.h>

#include "abi.h"

#include <string>

namespace qingjian {

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

    fcitx::Instance* instance_;
};

} // namespace qingjian
