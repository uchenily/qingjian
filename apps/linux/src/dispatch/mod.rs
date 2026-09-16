//! 按键分派：把 fcitx5 送来的按键翻译成 Engine 的调用，把 Engine 返回的候选交给候选窗口。
//!
//! 与 macOS 壳的 `handle_text` / `handle_command`、Windows 壳的 `dispatch::key` 对齐。
//! **这里不允许出现排序、词库或翻译逻辑。** 会话状态（候选、高亮、页码）在 [`Session`]。

pub(crate) mod key;
mod result;

use std::time::{Duration, Instant};

use qingjian_core::{Engine, QUESTION_PREFIX, shortcut};
use qingjian_platform::{
    AppsConfig, Config, DEFAULT_PAGE_KEYS, LayoutMode, Modifiers, PreeditMode, ThemeMode,
};

pub use key::KeyInput;
pub use result::{KeyOutcome, KeyResult};

use crate::frame::{Frame, FrameBuilder};
use crate::session::{Session, kind_of};

/// 学习数据落盘间隔（与 macOS / Windows 壳一致）。
const LEARNING_FLUSH_INTERVAL: Duration = Duration::from_secs(60);

/// 云端词在第一页末尾占的格数（与 macOS / Windows 一致）。
const CLOUD_SLOTS: usize = 2;

/// 分派器：持有会话状态与从配置来的常量。
pub struct Dispatch {
    session: Session,
    page_size: usize,
    page_keys: (char, char),
    layout: LayoutMode,
    theme: ThemeMode,
    /// 组句中的拼音显示位置（行内 / 窗口 / 两处）。MVP 阶段统一画在候选窗，保留字段供后续按配置分。
    #[allow(dead_code)]
    preedit_mode: PreeditMode,
    /// 英文模式要不要给候选（终端 / 编辑器类应用关掉）。MVP 暂不用，接中英切换后启用。
    #[allow(dead_code)]
    english_candidates: bool,
    /// 按应用关掉英文候选的配置。MVP 暂不用。
    #[allow(dead_code)]
    apps: AppsConfig,
    /// 整句补全（preedit 右侧、Tab 上屏）；缓冲变化时清空。
    sentence: Option<String>,
    /// 删候选后的屏幕提示，随下一帧下发、下一次按键清。
    notice: Option<String>,
    /// 上次把学习数据落盘的时间。
    last_flush: Instant,
    /// 当前应用标识。
    app: Option<String>,
}

impl Dispatch {
    pub fn new(config: &Config) -> Self {
        Self {
            session: Session::default(),
            page_size: config.general.page_size(),
            page_keys: config.general.page_keys(),
            layout: config.general.layout,
            theme: config.general.theme,
            preedit_mode: config.general.preedit,
            english_candidates: config.general.english_candidates,
            apps: config.apps.clone(),
            sentence: None,
            notice: None,
            last_flush: Instant::now(),
            app: None,
        }
    }

    /// 处理一次按键，返回处置结果、要显示的帧与要上屏的文本。
    pub fn key_event(&mut self, engine: &mut Engine, input: &KeyInput) -> KeyOutcome {
        // 提示在显示：敲任何键先收掉，键照常处理
        self.notice = None;
        if input.is_release {
            return KeyOutcome::passthrough();
        }
        // 记下应用标识，变了才告诉 Engine
        if self.app != input.app {
            self.app = input.app.clone();
            engine.set_application(input.app.clone());
        }
        let outcome = key::apply(self, engine, input);
        self.maybe_flush(engine);
        outcome
    }

    /// 焦点离开：缓冲原样上屏、收起候选窗、落盘。
    pub fn focus_out(&mut self, engine: &mut Engine) -> KeyOutcome {
        let commit = if !engine.composition().is_empty() {
            engine.take_raw()
        } else {
            String::new()
        };
        self.session = Session::default();
        self.sentence = None;
        self.notice = None;
        engine.break_chain();
        engine.flush_learning();
        self.last_flush = Instant::now();
        KeyOutcome::committed(commit, Frame::empty())
    }

    /// 重置：清空缓冲、收起候选窗（不落盘）。
    pub fn reset(&mut self, engine: &mut Engine) -> KeyOutcome {
        let commit = if !engine.composition().is_empty() {
            engine.take_raw()
        } else {
            String::new()
        };
        engine.clear();
        self.session = Session::default();
        self.sentence = None;
        self.notice = None;
        KeyOutcome::committed(commit, Frame::empty())
    }

    /// 轮询异步结果（云联想、释义兜底）。有更新返回新帧。
    pub fn poll(&mut self, engine: &mut Engine) -> Option<Frame> {
        let mut changed = false;
        // 云联想结果
        if let Some(prediction) = engine.poll_prediction() {
            changed = self.apply_prediction(engine, prediction) || changed;
        }
        // 释义兜底结果（不改变帧，只是后台写个人释义表）
        engine.poll_glosses();
        if changed {
            Some(self.build_frame(engine))
        } else {
            None
        }
    }

    /// 取当前应该显示的帧（不触发计算）。
    pub fn current_frame(&self, engine: &Engine) -> Frame {
        if engine.composition().is_empty() {
            return Frame::empty();
        }
        self.build_frame(engine)
    }

    /// 到点把学习数据落盘。
    fn maybe_flush(&mut self, engine: &mut Engine) {
        if self.last_flush.elapsed() >= LEARNING_FLUSH_INTERVAL {
            engine.flush_learning();
            self.last_flush = Instant::now();
        }
    }

    /// 按当前缓冲区重新查候选、更新 preedit，回到第一页。
    pub(crate) fn refresh(&mut self, engine: &mut Engine) {
        let mut cursor = engine.composition().cursor();
        match engine.query() {
            Ok(mut query) => {
                engine.annotate(&mut query.candidates);
                cursor = query.marked_cursor();
                let segments = query.marked_segments();
                self.session.reset(
                    &segments,
                    cursor,
                    query.candidates.items,
                    self.page_size,
                    CLOUD_SLOTS,
                );
            }
            Err(_) => {
                let marked = engine.composition().text().to_owned();
                self.session
                    .reset_plain(&marked, cursor, self.page_size, CLOUD_SLOTS);
            }
        }
    }

    /// 应用一次联想结果：云端词补进第一页末尾、整句补全进 preedit 右侧。返回帧是否变了。
    fn apply_prediction(
        &mut self,
        engine: &mut Engine,
        prediction: qingjian_core::Prediction,
    ) -> bool {
        let mut changed = false;
        if !engine.composition().is_empty()
            && self.session.cloud_slots_untouched()
            && !prediction.words.is_empty()
        {
            let mut words = qingjian_core::CandidateList {
                items: prediction
                    .words
                    .into_iter()
                    .map(|w| w.into_candidate())
                    .collect(),
            };
            engine.annotate(&mut words);
            let filled = self.session.set_cloud(words.items);
            tracing::debug!(filled, "云端词已补进候选");
            changed = filled > 0;
        }
        if let Some(sentence) = prediction.sentence
            && self.sentence.as_deref() != Some(&sentence)
        {
            self.sentence = Some(sentence);
            changed = true;
        }
        changed
    }

    /// 由会话状态构造一帧。
    fn build_frame(&self, _engine: &Engine) -> Frame {
        let preedit: Vec<(String, u8)> = self.session.preedit().to_vec();
        let candidates: Vec<(String, Option<String>, u8, bool)> = self
            .session
            .page_candidates()
            .into_iter()
            .map(|(cand, highlighted)| {
                let translation = cand
                    .translation
                    .as_ref()
                    .and_then(|t| t.senses().first())
                    .map(|s| s.text.clone());
                (cand.text, translation, kind_of(cand.kind), highlighted)
            })
            .collect();
        let builder = FrameBuilder {
            preedit,
            cursor: self.session.cursor(),
            candidates,
            highlight: self.session.highlighted() % self.session.page_size().max(1),
            page: self.session.page(),
            page_count: self.session.pages(),
            layout: layout_code(self.layout),
            theme: theme_code(self.theme),
            sentence: self.sentence.clone(),
            notice: self.notice.clone(),
        };
        builder.to_frame()
    }

    /// 当前应用里英文模式要不要给候选。
    #[allow(dead_code)]
    pub(crate) fn english_candidates_in(&self) -> bool {
        self.english_candidates
            && self
                .app
                .as_deref()
                .map(|app| !self.apps.english_candidates_off(app))
                .unwrap_or(true)
    }

    pub(crate) fn page_keys(&self) -> (char, char) {
        self.page_keys
    }

    pub(crate) fn page_size(&self) -> usize {
        self.page_size
    }

    pub(crate) fn session(&mut self) -> &mut Session {
        &mut self.session
    }

    pub(crate) fn take_sentence(&mut self) -> Option<String> {
        self.sentence.take()
    }

    #[allow(dead_code)]
    pub(crate) fn set_notice(&mut self, notice: String) {
        self.notice = Some(notice);
    }
}

/// 排布码。
fn layout_code(layout: LayoutMode) -> u8 {
    match layout {
        LayoutMode::Vertical => 0,
        LayoutMode::Horizontal => 1,
    }
}

/// 外观码。
fn theme_code(theme: ThemeMode) -> u8 {
    match theme {
        ThemeMode::System => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    }
}

// 引用占位，避免未使用警告（shortcut / Modifiers / DEFAULT_PAGE_KEYS 在 key 模块用）
#[allow(dead_code)]
fn _ensure_used() {
    let _ = shortcut::is_expression_char('1');
    let _ = Modifiers::default();
    let _ = DEFAULT_PAGE_KEYS;
    let _ = QUESTION_PREFIX;
}
