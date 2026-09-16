//! 一次要绘制的组句状态：preedit 行加候选页。C ABI 友好（所有字符串是 `*mut c_char`，shim 用完调 [`Frame::free_strings`]）。
//!
//! 与 `qingjian-platform` 的 `protocol::Frame` 同构，但 Linux 同进程、不走 serde，直接传结构体更简单。
//! 字段语义与 macOS 候选窗口的 `Frame`、Windows 协议的 `Frame` 对齐。

use std::ffi::CString;
use std::os::raw::c_char;

/// 一段 preedit 文本及其样式。
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct PreeditSegment {
    /// 文本（UTF-8，`\0` 结尾）。由 Rust 分配，shim 只读；整帧释放时一起归还。
    pub text: *mut c_char,
    /// 样式：0 普通（敲的拼音）、1 纠错删除线、2 光标后剩余。
    pub style: u8,
}

/// 一个候选词。
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct CandidateView {
    /// 上屏文本。
    pub text: *mut c_char,
    /// 译文（右侧标注），无则空指针。
    pub translation: *mut c_char,
    /// 来源类型：0 中文、1 整句、2 英文、3 云端、4 快捷、5 emoji、6 自定义。
    pub kind: u8,
    /// 是否高亮（页内）。
    pub highlighted: bool,
}

/// 一次要绘制的全部内容。空帧（`preedit_count == 0 && candidate_count == 0`）表示收起候选窗。
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// preedit 段数组指针。
    pub preedit: *mut PreeditSegment,
    /// preedit 段数。
    pub preedit_count: usize,
    /// 光标在 preedit 拼接文本里的字符位置。
    pub cursor: usize,

    /// 候选数组指针。
    pub candidates: *mut CandidateView,
    /// 候选数。
    pub candidate_count: usize,
    /// 当前页内高亮候选下标（页内，从 0 起）；`usize::MAX` 表示不高亮。
    pub highlight: usize,

    /// 当前页码（从 0 起）。
    pub page: usize,
    /// 总页数。
    pub page_count: usize,

    /// 排布：0 竖排、1 横排。
    pub layout: u8,
    /// 外观：0 跟随系统、1 浅色、2 深色。
    pub theme: u8,

    /// 整句补全（preedit 右侧，Tab 上屏）；空指针表示无。
    pub sentence: *mut c_char,
    /// 屏幕提示（删候选后的「已删除…」）；空指针表示无。
    pub notice: *mut c_char,
}

impl Frame {
    /// 空帧：没有在组句，shim 据此收起候选窗。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 是否空帧。
    pub fn is_empty(&self) -> bool {
        self.preedit_count == 0 && self.candidate_count == 0
    }

    /// 释放所有 Rust 分配的字符串。shim 用完一帧后调。
    pub fn free_strings(&mut self) {
        for i in 0..self.preedit_count {
            unsafe {
                let seg = &mut *self.preedit.add(i);
                if !seg.text.is_null() {
                    drop(CString::from_raw(seg.text));
                    seg.text = std::ptr::null_mut();
                }
            }
        }
        if !self.preedit.is_null() && self.preedit_count > 0 {
            unsafe {
                drop(Vec::from_raw_parts(
                    self.preedit,
                    self.preedit_count,
                    self.preedit_count,
                ))
            };
            self.preedit = std::ptr::null_mut();
            self.preedit_count = 0;
        }
        for i in 0..self.candidate_count {
            unsafe {
                let cand = &mut *self.candidates.add(i);
                if !cand.text.is_null() {
                    drop(CString::from_raw(cand.text));
                    cand.text = std::ptr::null_mut();
                }
                if !cand.translation.is_null() {
                    drop(CString::from_raw(cand.translation));
                    cand.translation = std::ptr::null_mut();
                }
            }
        }
        if !self.candidates.is_null() && self.candidate_count > 0 {
            unsafe {
                drop(Vec::from_raw_parts(
                    self.candidates,
                    self.candidate_count,
                    self.candidate_count,
                ))
            };
            self.candidates = std::ptr::null_mut();
            self.candidate_count = 0;
        }
        if !self.sentence.is_null() {
            unsafe { drop(CString::from_raw(self.sentence)) };
            self.sentence = std::ptr::null_mut();
        }
        if !self.notice.is_null() {
            unsafe { drop(CString::from_raw(self.notice)) };
            self.notice = std::ptr::null_mut();
        }
    }
}

/// 从 Rust 侧的帧数据构造 C ABI 帧（字符串 `CString::into_raw` 交出所有权）。
pub struct FrameBuilder {
    pub preedit: Vec<(String, u8)>,
    pub cursor: usize,
    pub candidates: Vec<(String, Option<String>, u8, bool)>,
    pub highlight: usize,
    pub page: usize,
    pub page_count: usize,
    pub layout: u8,
    pub theme: u8,
    pub sentence: Option<String>,
    pub notice: Option<String>,
}

impl FrameBuilder {
    pub fn to_frame(&self) -> Frame {
        let preedit: Vec<PreeditSegment> = self
            .preedit
            .iter()
            .map(|(text, style)| PreeditSegment {
                text: cstring(text),
                style: *style,
            })
            .collect();
        let candidates: Vec<CandidateView> = self
            .candidates
            .iter()
            .map(|(text, translation, kind, highlighted)| CandidateView {
                text: cstring(text),
                translation: translation
                    .as_deref()
                    .map(cstring)
                    .unwrap_or(std::ptr::null_mut()),
                kind: *kind,
                highlighted: *highlighted,
            })
            .collect();
        let (preedit_ptr, preedit_count) = into_vec_parts(preedit);
        let (cand_ptr, cand_count) = into_vec_parts(candidates);
        Frame {
            preedit: preedit_ptr,
            preedit_count,
            cursor: self.cursor,
            candidates: cand_ptr,
            candidate_count: cand_count,
            highlight: self.highlight,
            page: self.page,
            page_count: self.page_count,
            layout: self.layout,
            theme: self.theme,
            sentence: self
                .sentence
                .as_deref()
                .map(cstring)
                .unwrap_or(std::ptr::null_mut()),
            notice: self
                .notice
                .as_deref()
                .map(cstring)
                .unwrap_or(std::ptr::null_mut()),
        }
    }
}

/// 把字符串变成 `*mut c_char`（`CString::into_raw`）。空字符串也分配（避免空指针歧义）。
fn cstring(s: &str) -> *mut c_char {
    CString::new(s.as_bytes()).unwrap_or_default().into_raw()
}

/// 把 `Vec` 转成 `(ptr, len)`，所有权交出。
fn into_vec_parts<T>(vec: Vec<T>) -> (*mut T, usize) {
    if vec.is_empty() {
        return (std::ptr::null_mut(), 0);
    }
    let mut vec = vec.into_iter().collect::<Vec<_>>();
    vec.shrink_to_fit();
    let ptr = vec.as_mut_ptr();
    let len = vec.len();
    std::mem::forget(vec);
    (ptr, len)
}
