// 青简输入法 Rust cdylib 的 C ABI 声明。与 apps/linux/src/lib.rs、frame.rs 一一对应。
// C++ shim 通过这组函数调 Rust 侧的 Engine，再把返回的帧翻译成 fcitx5 InputPanel。
//
// 重要：QingjianFrame / QingjianPreeditSegment / QingjianCandidateView 的字段顺序与类型
// 必须与 Rust 侧 #[repr(C)] 结构体完全一致，否则内存错位。
#pragma once

#include <cstdint>
#include <cstddef>

#ifdef __cplusplus
extern "C" {
#endif

/// 按键处置结果（与 Rust 侧 KeyResult 对齐）。
enum QingjianKeyResult : int32_t {
    /// 放行给应用。
    QINGJIAN_KEY_PASSTHROUGH = 0,
    /// 已消费，按 out_frame 更新显示。
    QINGJIAN_KEY_CONSUMED = 1,
    /// 已消费且有上屏文本，先 commit 再更新显示。
    QINGJIAN_KEY_COMMITTED = 2,
};

/// preedit 的一段（文本 + 样式标记）。与 Rust PreeditSegment 对齐。
/// style: 0=普通, 1=纠错删除线, 2=光标后剩余。
struct QingjianPreeditSegment {
    const char* text;
    uint8_t style;
};

/// 候选窗里的一格。与 Rust CandidateView 对齐。
/// kind: 0=中文, 1=整句, 2=英文, 3=云端, 4=快捷, 5=emoji, 6=自定义。
struct QingjianCandidateView {
    const char* text;
    const char* translation;
    uint8_t kind;
    /// 是否高亮（当前选中）。Rust 侧是 bool，C++ 用 uint8_t 对齐（1 字节）。
    uint8_t highlighted;
};

/// 一帧：要绘制的 preedit + 候选表 + 翻页状态。
/// 所有字符串指针由 Rust 侧 CString::into_raw 分配，用完要调 qingjian_frame_free 归还。
/// 字段顺序与 Rust Frame 完全一致（#[repr(C)]）。
struct QingjianFrame {
    /// preedit 段数组指针。
    QingjianPreeditSegment* preedit;
    /// preedit 段数。
    uintptr_t preedit_count;
    /// 光标在 preedit 拼接文本里的字符位置。
    uintptr_t cursor;

    /// 候选数组指针。
    QingjianCandidateView* candidates;
    /// 候选数。
    uintptr_t candidate_count;
    /// 当前页内高亮候选下标（页内，从 0 起）；SIZE_MAX 表示不高亮。
    uintptr_t highlight;

    /// 当前页码（从 0 起）。
    uintptr_t page;
    /// 总页数。
    uintptr_t page_count;

    /// 排布：0 竖排、1 横排。
    uint8_t layout;
    /// 外观：0 跟随系统、1 浅色、2 深色。
    uint8_t theme;
    // 注意：Rust 侧 layout+theme 是两个 u8，后面有填充到指针对齐。这里保持字段顺序即可。

    /// 整句补全（preedit 右侧，Tab 上屏）；空指针表示无。
    const char* sentence;
    /// 屏幕提示（删候选后的「已删除…」）；空指针表示无。
    const char* notice;
};

/// 初始化：装配 Engine 并装进单例。返回 0 成功，非 0 是错误码。
int32_t qingjian_init(void);

/// 释放单例。fcitx5 卸载 addon 时调。
void qingjian_shutdown(void);

/// 处理一次按键。
/// - sym: xkb keysym
/// - code: keycode（物理键）
/// - modifiers: 修饰键位掩码（Shift=1, CapsLock=2, Ctrl=4, Alt=8, Super=64）
/// - character: 该键产生的字符的 Unicode 码点；功能键传 0
/// - is_release: 按键释放（key up）
/// - app: 当前应用标识（桌面进程名）；可传 nullptr
/// - out_frame: 接收要显示的帧（shim 分配，Rust 填）
/// - out_commit: 接收要上屏的文本（shim 分配，Rust 填，\0 结尾）
/// - commit_len: out_commit 的容量
/// 返回 QingjianKeyResult。
int32_t qingjian_key_event(uint32_t sym,
                           uint32_t code,
                           uint32_t modifiers,
                           uint32_t character,
                           int is_release,
                           const char* app,
                           QingjianFrame* out_frame,
                           char* out_commit,
                           size_t commit_len);

/// 焦点离开当前输入上下文：把缓冲原样上屏、收起候选窗、落盘学习数据。
int32_t qingjian_focus_out(char* out_commit, size_t commit_len);

/// 重置（切应用 / 切输入法 / 失焦又聚焦）：清缓冲、收候选窗。
int32_t qingjian_reset(char* out_commit, size_t commit_len);

/// 轮询异步结果（云联想、释义兜底、神经重排）。返回 1 表示有新帧要画，0 表示无更新。
int32_t qingjian_poll(QingjianFrame* out_frame);

/// 取当前应该显示的帧（不触发计算）。shim 在 InputContext 重建时调。
int32_t qingjian_current_frame(QingjianFrame* out_frame);

/// 释放 Frame 内部字符串。Frame 由 qingjian_key_event / qingjian_poll / qingjian_current_frame 写入后，
/// shim 用完要调这个归还，否则泄漏。frame 为空指针时安全。
void qingjian_frame_free(QingjianFrame* frame);

#ifdef __cplusplus
}
#endif
