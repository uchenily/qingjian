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
#include <fcitx-utils/event.h>

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

/// 候选来源标记，按 kind 返回短后缀；中文与 emoji 不标（返回空串）。
/// kind 值与 apps/linux/src/session.rs 的 KIND_* 常量对齐。
const char* sourceMarkOf(uint8_t kind) {
    switch (kind) {
        case 1:  return "[句]";   // 整句
        case 2:  return "[en]";   // 英文
        case 4:  return "[v]";    // 快捷
        case 6:  return "[自定义]"; // 自定义短语
        case 7:  return "[生]";   // 模型生成
        default: return "";       // 0 中文、5 emoji 不标
    }
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
        // 译文在前
        if (view.translation && *view.translation) {
            comment.append(std::string(view.translation));
        }
        // 后缀：来源标记 + ✦（经过神经模型处理：重排顶上的整句，或模型直接生成）。
        // 中文(0)与 emoji(5)无来源标记，且 rescored 只会落在整句上，故不会出现孤立的 ✦。
        const char* sourceMark = sourceMarkOf(view.kind);
        bool modelTouched = view.rescored || view.kind == 7;  // 7 = KIND_GENERATED
        if (sourceMark[0] != '\0' || modelTouched) {
            if (!comment.toString().empty()) {
                comment.append(" ");
            }
            if (sourceMark[0] != '\0') {
                comment.append(std::string(sourceMark));
            }
            if (modelTouched) {
                comment.append("✦");
            }
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
    // 起一个定时器驱动本地整句模型重排：组句期间周期性调 qingjian_poll，
    // 有非空帧就重画当前 InputContext 的候选窗。闲时（没在组句 / 没在等重排）停掉，
    // 下次 keyEvent 再启动，避免空转。
    auto* loop = &instance_->eventLoop();
    poll_timer_ = loop->addTimeEvent(
        CLOCK_MONOTONIC, fcitx::now(CLOCK_MONOTONIC) + POLL_INTERVAL_USEC,
        POLL_INTERVAL_USEC,
        [this](fcitx::EventSourceTime* timer, uint64_t /*usec*/) {
            int32_t got = onPollTick();
            // fcitx5（sd-event 后端）的 TimeEvent 是一次性的：触发后变 disabled。
            // 还要继续轮询（got == 0 或 1）就重设下一次时间并重新 enable。
            // 闲时（got == -1）onPollTick 内部已停定时器，不再重设。
            if (got != -1) {
                timer->setNextInterval(POLL_INTERVAL_USEC);
                timer->setEnabled(true);
            }
            return true;
        });
    if (!poll_timer_) {
        FCITX_ERROR() << "青简：创建轮询定时器失败，本地整句模型重排不会驱动";
    } else {
        // 初始停着，等第一次 keyEvent 启动
        poll_timer_->setEnabled(false);
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

    // �下当前 InputContext，重排定时器用它重画
    current_ic_ = ic;

    // 组句开始：确保轮询定时器在跑（闲时被停过的话这里重启）
    if (poll_timer_ && !poll_timer_->isEnabled()) {
        poll_timer_->setNextInterval(POLL_INTERVAL_USEC);
        poll_timer_->setEnabled(true);
    }

    // 释放事件一律放行（与 macOS / Windows 一致：只在 key down 处理）
    if (keyEvent.isRelease()) {
        return;
    }

    const auto& key = keyEvent.key();
    auto app = appOf(ic);
    QingjianFrame frame = {};
    char commit_buf[256] = {};

    // 取光标前文本给神经重排模型当上下文（与 macOS / Windows 壳对齐）。
    // 应用不支持 surrounding text 时传 nullptr，Engine 退回本会话历史。
    const char* context_ptr = nullptr;
    uint32_t cursor_chars = 0;
    if (ic->capabilityFlags().test(fcitx::CapabilityFlag::SurroundingText)) {
        const auto& surrounding = ic->surroundingText();
        if (surrounding.isValid() && !surrounding.text().empty()) {
            context_ptr = surrounding.text().c_str();
            cursor_chars = surrounding.cursor();
        }
    }
    qingjian_set_context(context_ptr, cursor_chars);

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
    if (current_ic_ == ic) {
        current_ic_ = nullptr;
    }
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

int32_t QingjianEngine::onPollTick() {
    // 没有活跃的输入上下文就不轮询
    auto* ic = current_ic_;
    if (!ic) return -1;
    QingjianFrame frame = {};
    int32_t got = qingjian_poll(&frame);
    if (got == 1) {
        // 有重排后的帧：重画当前候选窗
        paintFrame(ic, frame);
        qingjian_frame_free(&frame);
    } else if (got == -1) {
        // 闲时：停掉定时器，下次按键再启动
        if (poll_timer_) poll_timer_->setEnabled(false);
    }
    // got == 0：还在等防抖 / 后台打分，继续轮询
    return got;
}

} // namespace qingjian
