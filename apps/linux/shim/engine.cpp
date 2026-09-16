// 青简输入法 fcitx5 addon 引擎实现。
//
// 职责（与 macOS IMK / Windows TSF 壳对齐）：
// - 把 fcitx5 的 KeyEvent 翻译成 C ABI 的按键参数（keysym / 修饰键 / 字符）
// - 调 Rust 侧 qingjian_key_event，按返回值更新 InputPanel 或 commit
// - 把 Rust 返回的 QingjianFrame 翻译成 fcitx5 的 preedit（TextFormat）与候选表（CommonCandidateList）
//
// 不碰排序 / 词库 / 翻译——那些都在 Rust 侧的 Engine 里。
#include "engine.h"

#include "abi.h"

#include <fcitx/inputcontext.h>
#include <fcitx/inputpanel.h>
#include <fcitx/candidatelist.h>
#include <fcitx/text.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/log.h>

#include <cstring>
#include <memory>
#include <string>

namespace qingjian {

namespace {

/// fcitx5 修饰键 → 青简 C ABI 修饰键位掩码。
uint32_t toModifiers(const fcitx::Key& key) {
    uint32_t mods = 0;
    if (key.states() & fcitx::KeyState::Shift) mods |= 1;       // MOD_SHIFT
    if (key.states() & fcitx::KeyState::CapsLock) mods |= 2;    // MOD_CAPS
    if (key.states() & fcitx::KeyState::Ctrl) mods |= 4;        // MOD_CTRL
    if (key.states() & fcitx::KeyState::Alt) mods |= 8;         // MOD_ALT
    if (key.states() & fcitx::KeyState::Super) mods |= 64;      // MOD_SUPER
    return mods;
}

/// 从 fcitx5 Key 取要送给 Rust 的 Unicode 字符。用 keySymToUnicode 把 keysym 转码点。
uint32_t toCharacter(const fcitx::Key& key) {
    return fcitx::Key::keySymToUnicode(key.sym());
}

/// 把 fcitx5 InputContext 的前端程序名取出来当 app 标识。
std::string appOf(fcitx::InputContext* ic) {
    if (!ic) return {};
    const auto& program = ic->program();
    if (!program.empty()) return program;
    return {};
}

/// 候选词：持有文本与译文，select 时把文本 commit 给应用。
/// （当前 MVP 由 Rust 侧在数字 / 空格选词时已 commit，这里 select 主要给鼠标点选用。）
class QingjianCandidateWord : public fcitx::CandidateWord {
public:
    QingjianCandidateWord(fcitx::Text text, std::string translation)
        : fcitx::CandidateWord(std::move(text)),
          translation_(std::move(translation)) {
        if (!translation_.empty()) {
            setComment(fcitx::Text(translation_));
        }
    }

    void select(fcitx::InputContext* inputContext) const override {
        if (inputContext) {
            inputContext->commitString(text().toString());
        }
    }

private:
    std::string translation_;
};

/// 把 QingjianFrame 里的 preedit 段翻译成 fcitx5 Text（带格式标记）。
fcitx::Text toPreedit(const QingjianFrame& frame) {
    fcitx::Text text;
    for (uintptr_t i = 0; i < frame.preedit_count; ++i) {
        const auto& seg = frame.preedit[i];
        const char* str = seg.text ? seg.text : "";
        // style: 0=普通, 1=高亮, 2=已确认
        fcitx::TextFormatFlags flags;
        if (seg.style == 1) {
            flags |= fcitx::TextFormatFlag::HighLight;
        } else if (seg.style == 2) {
            flags |= fcitx::TextFormatFlag::DontCommit;
        }
        text.append(std::string(str), flags);
    }
    return text;
}

/// 把 QingjianFrame 里的候选翻译成 fcitx5 CandidateList。
std::unique_ptr<fcitx::CandidateList> toCandidates(fcitx::InputContext* ic,
                                                    const QingjianFrame& frame) {
    if (frame.candidate_count == 0) {
        return nullptr;
    }
    auto candList = std::make_unique<fcitx::CommonCandidateList>();
    candList->setPageSize(static_cast<int>(frame.candidate_count));
    for (uintptr_t i = 0; i < frame.candidate_count; ++i) {
        const auto& view = frame.candidates[i];
        const char* str = view.text ? view.text : "";
        std::string translation = (view.translation && *view.translation)
                                      ? std::string(view.translation)
                                      : std::string();
        auto word = std::make_unique<QingjianCandidateWord>(
            fcitx::Text(std::string(str)), std::move(translation));
        candList->append(std::move(word));
    }
    // 高亮当前选中
    for (uintptr_t i = 0; i < frame.candidate_count; ++i) {
        if (frame.candidates[i].highlighted) {
            candList->setGlobalCursorIndex(static_cast<int>(i));
            break;
        }
    }
    // 布局：0=竖排, 1=横排
    candList->setLayoutHint(frame.layout == 1
                                ? fcitx::CandidateLayoutHint::Horizontal
                                : fcitx::CandidateLayoutHint::Vertical);
    return candList;
}

} // namespace

QingjianEngine::QingjianEngine(fcitx::Instance* instance)
    : instance_(instance) {
    // addon 加载时初始化 Rust 单例
    if (qingjian_init() != 0) {
        FCITX_ERROR() << "青简 Linux 初始化失败";
    }
}

QingjianEngine::~QingjianEngine() {
    // 不调 qingjian_shutdown：fcitx5 卸载 addon 时先 dlclose(libqingjian_linux.so) 再执行
    // 本析构，此时 qingjian_shutdown 符号已不可用会段错误。落盘在 focus_out / 定时 flush 里已做，
    // 进程退出时 Rust 侧的 OnceLock 自然释放。
}

void QingjianEngine::activate(const fcitx::InputMethodEntry& /*entry*/,
                              fcitx::InputContextEvent& /*event*/) {
    // 激活时不需要额外动作；Rust 单例已在构造时初始化
}

void QingjianEngine::deactivate(const fcitx::InputMethodEntry& /*entry*/,
                                fcitx::InputContextEvent& event) {
    auto* ic = event.inputContext();
    char commit_buf[256] = {};
    qingjian_reset(commit_buf, sizeof(commit_buf));
    if (commit_buf[0] != '\0') {
        commitText(ic, commit_buf);
    }
    hidePanel(ic);
}

void QingjianEngine::keyEvent(const fcitx::InputMethodEntry& /*entry*/,
                              fcitx::KeyEvent& keyEvent) {
    auto* ic = keyEvent.inputContext();
    if (!ic) {
        return;
    }

    // 释放事件一律放行（与 macOS / Windows 一致：只在 key down 处理）
    if (keyEvent.isRelease()) {
        return;
    }

    const auto& key = keyEvent.key();
    auto app = appOf(ic);
    QingjianFrame frame = {};
    char commit_buf[256] = {};

    int32_t result = qingjian_key_event(
        static_cast<uint32_t>(key.sym()),
        static_cast<uint32_t>(key.code()),
        toModifiers(key),
        toCharacter(key),
        0, // is_release = false
        app.empty() ? nullptr : app.c_str(),
        &frame,
        commit_buf,
        sizeof(commit_buf));

    if (result == QINGJIAN_KEY_PASSTHROUGH) {
        // 放行：先释放帧（可能有字符串），不更新面板
        qingjian_frame_free(&frame);
        return;
    }

    // 已消费
    keyEvent.filterAndAccept();

    // 先 commit（若有上屏文本）
    if (result == QINGJIAN_KEY_COMMITTED && commit_buf[0] != '\0') {
        commitText(ic, commit_buf);
    }

    // 再按帧更新面板
    paintFrame(ic, frame);
    qingjian_frame_free(&frame);
}

void QingjianEngine::reset(const fcitx::InputMethodEntry& /*entry*/,
                           fcitx::InputContextEvent& event) {
    auto* ic = event.inputContext();
    char commit_buf[256] = {};
    qingjian_reset(commit_buf, sizeof(commit_buf));
    if (commit_buf[0] != '\0') {
        commitText(ic, commit_buf);
    }
    hidePanel(ic);
}

void QingjianEngine::paintFrame(fcitx::InputContext* ic, const QingjianFrame& frame) {
    auto& panel = ic->inputPanel();

    // 空 preedit 且无候选：收起候选窗
    if (frame.preedit_count == 0 && frame.candidate_count == 0) {
        hidePanel(ic);
        return;
    }

    // preedit：只设客户端 preedit（应用光标处显示拼音），不设服务端 preedit，
    // 避免拼音在应用内和候选窗顶部重复显示（与 macOS inline 模式一致）。
    auto preedit = toPreedit(frame);
    panel.setClientPreedit(preedit);
    // auxUp 显示整句补全 / 提示
    if (frame.sentence && *frame.sentence) {
        fcitx::Text aux;
        aux.append(std::string(frame.sentence));
        panel.setAuxUp(aux);
    } else if (frame.notice && *frame.notice) {
        fcitx::Text aux;
        aux.append(std::string(frame.notice));
        panel.setAuxUp(aux);
    } else {
        panel.setAuxUp(fcitx::Text());
    }

    // 候选表
    auto candList = toCandidates(ic, frame);
    if (candList) {
        panel.setCandidateList(std::move(candList));
    } else {
        panel.setCandidateList(nullptr);
    }

    ic->updatePreedit();
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

void QingjianEngine::commitText(fcitx::InputContext* ic, const char* text) {
    if (!ic || !text || text[0] == '\0') return;
    ic->commitString(std::string(text));
}

void QingjianEngine::hidePanel(fcitx::InputContext* ic) {
    if (!ic) return;
    auto& panel = ic->inputPanel();
    panel.setClientPreedit(fcitx::Text());
    panel.setPreedit(fcitx::Text());
    panel.setAuxUp(fcitx::Text());
    panel.setCandidateList(nullptr);
    ic->updatePreedit();
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

} // namespace qingjian
