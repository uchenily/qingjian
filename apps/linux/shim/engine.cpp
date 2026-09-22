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
/// 译文通过 fcitx5 CandidateWord::setComment 显示在候选词右侧（fcitx5 ≥ 5.1.9）。
class QingjianCandidateWord : public fcitx::CandidateWord {
public:
    QingjianCandidateWord(fcitx::Text text, fcitx::Text comment)
        : fcitx::CandidateWord(std::move(text)) {
        if (comment.size() > 0) {
            setComment(std::move(comment));
        }
    }

    void select(fcitx::InputContext* inputContext) const override {
        if (inputContext) {
            inputContext->commitString(text().toString());
        }
    }
};

/// 把 QingjianFrame 里的 preedit 段翻译成 fcitx5 Text（带格式标记）。
fcitx::Text toPreedit(const QingjianFrame& frame) {
    fcitx::Text text;
    for (uintptr_t i = 0; i < frame.preedit_count; ++i) {
        const auto& seg = frame.preedit[i];
        const char* str = seg.text ? seg.text : "";
        // style: 0=普通（敲的拼音），1=纠错删除线，2=光标后剩余（与 Rust session.rs 对齐）
        fcitx::TextFormatFlags flags;
        if (seg.style == 1) {
            flags |= fcitx::TextFormatFlag::Strike;
        }
        text.append(std::string(str), flags);
    }
    return text;
}

/// 把 QingjianFrame 里的候选翻译成 fcitx5 CandidateList。
/// 序号用 setLabels 设成 1. 2. … 9.（页内位置），译文用 CandidateWord::setComment 显示在右侧。
std::unique_ptr<fcitx::CandidateList> toCandidates(const QingjianFrame& frame) {
    if (frame.candidate_count == 0) {
        return nullptr;
    }
    auto candList = std::make_unique<fcitx::CommonCandidateList>();
    candList->setPageSize(static_cast<int>(frame.candidate_count));

    // 序号标签：1. 2. … 9.（fcitx5 会把不足 10 个的补齐，这里按实际候选数给）
    std::vector<std::string> labels;
    labels.reserve(frame.candidate_count);
    for (uintptr_t i = 0; i < frame.candidate_count; ++i) {
        labels.push_back(std::to_string(i + 1) + ".");
    }
    candList->setLabels(labels);

    for (uintptr_t i = 0; i < frame.candidate_count; ++i) {
        const auto& view = frame.candidates[i];
        const char* str = view.text ? view.text : "";
        fcitx::Text comment;
        if (view.translation && *view.translation) {
            comment.append(std::string(view.translation));
        }
        auto word = std::make_unique<QingjianCandidateWord>(
            fcitx::Text(std::string(str)), std::move(comment));
        candList->append(std::move(word));
    }
    // 高亮当前选中：用页内 cursorIndex（frame.highlight 是页内下标）
    if (frame.highlight != static_cast<uintptr_t>(-1)
        && frame.highlight < frame.candidate_count) {
        candList->setCursorIndex(static_cast<int>(frame.highlight));
        candList->setGlobalCursorIndex(static_cast<int>(frame.highlight));
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

    // 拼音分段放候选窗顶部（服务端 preedit，单独一行，无序号）
    auto pinyin = toPreedit(frame);
    panel.setPreedit(pinyin);

    // 应用内预览区（client preedit）显示第一个候选结果；没有候选时退回拼音
    fcitx::Text preview;
    if (frame.candidate_count > 0 && frame.candidates[0].text && *frame.candidates[0].text) {
        preview.append(std::string(frame.candidates[0].text));
    } else {
        preview = pinyin;
    }
    // 光标位于预览文本末尾
    preview.setCursor(static_cast<int>(preview.toString().size()));
    panel.setClientPreedit(preview);

    // auxUp 显示提示
    if (frame.notice && *frame.notice) {
        fcitx::Text aux;
        aux.append(std::string(frame.notice));
        panel.setAuxUp(aux);
    } else {
        panel.setAuxUp(fcitx::Text());
    }

    // 候选表
    auto candList = toCandidates(frame);
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
