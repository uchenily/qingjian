//! 当前输入会话的 UI 状态：候选排布、高亮、页码、preedit。
//!
//! 与 macOS 壳的 `host::Session` 同构：候选的分页与云端词位置由 Core 的 [`CandidateLayout`] 定，
//! 这里只管高亮与页码。Engine 的缓冲区是全进程一份，会话状态跟着它走。

use qingjian_core::{Candidate, CandidateLayout, Cell, MarkedSegment};

/// preedit 段的样式。
pub const STYLE_TYPED: u8 = 0;
pub const STYLE_CORRECTED: u8 = 1;
pub const STYLE_REST: u8 = 2;

/// 候选来源类型的 C ABI 编码（与 `frame::CandidateView::kind` 对齐）。
pub const KIND_CHINESE: u8 = 0;
pub const KIND_SENTENCE: u8 = 1;
pub const KIND_ENGLISH: u8 = 2;
pub const KIND_CLOUD: u8 = 3;
pub const KIND_SHORTCUT: u8 = 4;
pub const KIND_EMOJI: u8 = 5;
pub const KIND_CUSTOM: u8 = 6;

/// 当前输入会话的 UI 状态。
#[derive(Debug, Default)]
pub struct Session {
    /// 上一次查询的候选排布（本地候选 + 云端词）。
    layout: CandidateLayout,

    /// 高亮的格子下标（在整个排布里的绝对位置）。
    highlighted: usize,

    /// 当前页。
    page: usize,

    /// 这轮查询里用户用方向键 / 翻页键动过高亮。
    navigated: bool,

    /// 候选窗口顶部显示的拼音行（分段 + 光标）。
    preedit: Vec<(String, u8)>,

    /// 拼音行光标。
    cursor: usize,
}

impl Session {
    /// 新一轮查询：候选换掉，选中第一个真实候选。
    pub fn reset(
        &mut self,
        segments: &[MarkedSegment],
        cursor: usize,
        candidates: Vec<Candidate>,
        page_size: usize,
        slots: usize,
    ) {
        self.preedit = segments
            .iter()
            .filter(|s| !s.text.is_empty())
            .map(|s| (s.text.clone(), style_of(s.kind)))
            .collect();
        self.cursor = cursor;
        self.layout = CandidateLayout::new(candidates, page_size, slots);
        self.highlighted = (0..self.layout.len())
            .find(|&i| self.layout.candidate(i).is_some())
            .unwrap_or(0);
        self.page = self.highlighted / self.layout.page_size();
        self.navigated = false;
    }

    /// 查询失败时退回显示原始字母。
    pub fn reset_plain(&mut self, text: &str, cursor: usize, page_size: usize, slots: usize) {
        self.preedit = if text.is_empty() {
            Vec::new()
        } else {
            vec![(text.to_owned(), STYLE_TYPED)]
        };
        self.cursor = cursor;
        self.layout = CandidateLayout::new(Vec::new(), page_size, slots);
        self.highlighted = 0;
        self.page = 0;
        self.navigated = false;
    }

    /// 第 `index` 格的候选。
    pub fn candidate(&self, index: usize) -> Option<Candidate> {
        self.layout.candidate(index).cloned()
    }

    /// 当前页第 `offset` 格在整个排布里的下标；越界返回 `None`。
    #[allow(dead_code)]
    pub fn index_on_page(&self, offset: usize) -> Option<usize> {
        let index = self.page * self.layout.page_size() + offset;
        (offset < self.layout.page_size() && index < self.layout.len()).then_some(index)
    }

    /// 当前页的格子（带序号与候选）。
    #[allow(dead_code)]
    pub fn page_cells(&self) -> Vec<Cell<'_>> {
        self.layout.page(self.page)
    }

    /// 当前页的候选（带译文，供帧生成用）。跳过没有候选的格子（云端词未到的占位）。
    pub fn page_candidates(&self) -> Vec<(Candidate, bool)> {
        let page_size = self.layout.page_size();
        let start = self.page * page_size;
        let end = (start + page_size).min(self.layout.len());
        (start..end)
            .filter_map(|i| {
                self.layout
                    .candidate(i)
                    .cloned()
                    .map(|cand| (cand, i == self.highlighted))
            })
            .collect()
    }

    /// 高亮上下移动，越过页边自动翻页。返回是否有变化。
    pub fn move_highlight(&mut self, delta: isize) -> bool {
        let len = self.layout.len();
        if len == 0 {
            return false;
        }
        let current = self.highlighted as isize;
        let mut next = (current + delta).clamp(0, len as isize - 1) as usize;
        while self.layout.candidate(next).is_none() {
            let candidate = next as isize + delta.signum();
            if candidate < 0 || candidate >= len as isize || delta == 0 {
                return false;
            }
            next = candidate as usize;
        }
        if next == self.highlighted {
            return false;
        }
        self.highlighted = next;
        self.page = next / self.layout.page_size();
        self.navigated = true;
        true
    }

    /// 翻页并选中新页第一个真实候选，跳过没有候选的页。
    pub fn turn_page(&mut self, delta: isize) -> bool {
        let pages = self.layout.pages().max(1);
        let current = self.page as isize;
        let mut next = (current + delta).clamp(0, pages as isize - 1) as usize;
        loop {
            if next == self.page {
                return false;
            }
            let start = next * self.layout.page_size();
            if let Some(index) = (start..(start + self.layout.page_size()).min(self.layout.len()))
                .find(|&i| self.layout.candidate(i).is_some())
            {
                self.page = next;
                self.highlighted = index;
                break;
            }
            let following = next as isize + delta.signum();
            if following < 0 || following >= pages as isize || delta == 0 {
                return false;
            }
            next = following as usize;
        }
        self.navigated = true;
        true
    }

    pub fn pages(&self) -> usize {
        self.layout.pages()
    }

    pub fn page(&self) -> usize {
        self.page
    }

    pub fn highlighted(&self) -> usize {
        self.highlighted
    }

    pub fn navigated(&self) -> bool {
        self.navigated
    }

    pub fn preedit(&self) -> &[(String, u8)] {
        &self.preedit
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn layout_len(&self) -> usize {
        self.layout.len()
    }

    pub fn page_size(&self) -> usize {
        self.layout.page_size()
    }

    /// 云端词补进第一页末尾几格（前面的本地候选不动）。返回填了几格。
    pub fn set_cloud(&mut self, words: Vec<qingjian_core::Candidate>) -> usize {
        self.layout.set_cloud(words)
    }

    /// 本地候选（不含云端词）。
    #[allow(dead_code)]
    pub fn local(&self) -> &[qingjian_core::Candidate] {
        self.layout.local()
    }

    /// 排布总容量（本地 + 云端格）。
    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        self.layout.capacity()
    }

    /// 云端词到了还能不能补进第一页：用户还在第一页，且高亮没落在会被云端词顶掉的那几格上。
    pub fn cloud_slots_untouched(&self) -> bool {
        let layout = &self.layout;
        let page = self.page;
        let highlighted = self.highlighted;
        let local_empty = layout.local().is_empty();
        let page_size = layout.page_size();
        let capacity = layout.capacity();
        // 没有本地候选（问字模式）时整页都是云端的，谈不上挪走谁，永远能补
        if local_empty {
            return true;
        }
        if page != 0 {
            return false;
        }
        // 云端词填的是第一页末尾 (page_size - slots) .. page_size 这几格
        let slots = page_size.saturating_sub(capacity);
        let cloud_start = page_size - slots;
        highlighted < cloud_start
    }
}

/// Core 的 `MarkedKind` 转成 C ABI 样式码。
fn style_of(kind: qingjian_core::MarkedKind) -> u8 {
    match kind {
        qingjian_core::MarkedKind::Typed => STYLE_TYPED,
        qingjian_core::MarkedKind::Corrected => STYLE_CORRECTED,
        qingjian_core::MarkedKind::Rest => STYLE_REST,
    }
}

/// Core 的 `CandidateKind` 转成 C ABI 类型码。
pub fn kind_of(kind: qingjian_core::CandidateKind) -> u8 {
    use qingjian_core::CandidateKind;
    match kind {
        CandidateKind::Chinese => KIND_CHINESE,
        CandidateKind::Sentence => KIND_SENTENCE,
        CandidateKind::English => KIND_ENGLISH,
        CandidateKind::Cloud => KIND_CLOUD,
        CandidateKind::Shortcut => KIND_SHORTCUT,
        CandidateKind::Emoji => KIND_EMOJI,
        CandidateKind::Custom(_) => KIND_CUSTOM,
    }
}
