//! 按键怎么作用到 Engine / 高亮上。分流规则与 macOS 壳的 `handle_text` / `handle_command` 对齐。

use qingjian_core::{CandidateKind, QUESTION_PREFIX, shortcut};
use qingjian_platform::protocol::KeyEvent;

use super::{Effect, codes, with_prefix};
use crate::dispatch::Router;

impl Router {
    /// 功能键靠键码，其余靠字符。组句中修饰键 + 数字是快捷键；带 Ctrl / Alt / Win 而没配到快捷键的键归应用。
    /// 表达式模式里 Shift + 数字打的是 `^ * ( )`，不当快捷键。
    pub(crate) fn apply_key(&mut self, event: &KeyEvent) -> Effect {
        if self.composing()
            && !self.engine.expression_mode()
            && let Some(digit) = codes::digit_key(event.virtual_key)
            && let Some(effect) = self.apply_digit_shortcut(digit, event.modifiers.chord())
        {
            return effect;
        }
        // emacs 风格编辑键（仅组句中）：Ctrl-A/E 行首尾、Ctrl-W 删前一个音节、
        // Ctrl-U 删到行首、Alt-F/B 按音节跳光标。不组句时交给应用。
        if self.composing()
            && let Some(effect) = self.apply_emacs(event)
        {
            return effect;
        }
        if event.modifiers.has_command_key() {
            return Effect::Passthrough;
        }
        // 持久英文模式彻底直通：不组句、不转全角、不弹候选窗，按键原样归应用。
        // 切到英文模式时已 commit_pending，不会还在组句；防御性地把残留落定再放行。
        if event.modifiers.english_mode {
            if self.composing() {
                let pending = self.engine.take_raw();
                if !pending.is_empty() {
                    return Effect::Changed(Some(pending));
                }
            }
            return Effect::Passthrough;
        }
        let Some(c) = event.character.filter(|c| !c.is_control()) else {
            return self.apply_function_key(event);
        };
        // Caps 亮着无论中英模式都直接出大写英文。
        let caps = event.modifiers.caps;
        let english = caps;
        // 缓冲区为空时敲 `?` 先进问字模式
        if !self.composing() && c == QUESTION_PREFIX {
            self.engine.set_english_mode(false);
            self.engine.push(c);
            return Effect::Changed(None);
        }
        let question = self.composing() && self.engine.question_mode();
        // Caps 亮时问字：字母以大写送来，按小写收进问题。
        let c = if question && english && c.is_ascii_uppercase() {
            c.to_ascii_lowercase()
        } else {
            c
        };
        // 只有一个 `?` 时敲了字母以外的键：还原成问号上屏；空格只是「把这个 ? 上屏」，其他键按没在组句重新分派。
        if question && !c.is_ascii_lowercase() && self.engine.bare_question() {
            let mark = self.restore_bare_question(english);
            if c == ' ' {
                return Effect::Changed(Some(mark));
            }
            return with_prefix(Some(mark), self.apply_key(event), c);
        }
        self.engine.set_english_mode(false);
        let effect = if english && !question {
            self.apply_english(c, event)
        } else {
            self.apply_chinese(c, event)
        };
        with_prefix(None, effect, c)
    }

    /// 缓冲区里只有一个 `?`：清掉，还原成问号（按当前模式的全角设置转）。
    fn restore_bare_question(&mut self, english: bool) -> String {
        self.engine.clear();
        if self.full_width_for(english)
            && let Some(mark) = self.engine.punctuate(QUESTION_PREFIX)
        {
            return mark.to_owned();
        }
        self.engine.note_passthrough(QUESTION_PREFIX);
        QUESTION_PREFIX.to_string()
    }

    /// emacs 风格编辑键（仅组句中调用）。返回 `Some` 表示已处理，`None` 表示不是 emacs 键、交给后续逻辑。
    ///
    /// - Ctrl-A：光标到行首
    /// - Ctrl-E：光标到行尾
    /// - Ctrl-W：删掉光标前一个音节
    /// - Ctrl-U：删掉光标前的全部拼音
    /// - Alt-F：光标右跳一个音节
    /// - Alt-B：光标左跳一个音节
    ///
    /// 按 virtual_key 认字母（Ctrl/Alt 会把 character 变成控制字符）。
    fn apply_emacs(&mut self, event: &KeyEvent) -> Option<Effect> {
        let vk = event.virtual_key;
        // 字母键码 0x41..0x5a（A..Z）
        if !(0x41..=0x5A).contains(&vk) {
            return None;
        }
        let lower = vk + 32; // a..z
        if event.modifiers.ctrl && !event.modifiers.alt {
            return Some(match lower {
                0x61 => {
                    self.engine.move_cursor_home();
                    Effect::Changed(None)
                }
                0x65 => {
                    self.engine.move_cursor_end();
                    Effect::Changed(None)
                }
                0x77 => {
                    self.engine.delete_syllable_backward();
                    Effect::Changed(None)
                }
                0x75 => {
                    self.engine.delete_to_start();
                    Effect::Changed(None)
                }
                _ => return None,
            });
        }
        if event.modifiers.alt && !event.modifiers.ctrl {
            return Some(match lower {
                0x66 => {
                    self.engine.move_cursor_syllable_right();
                    Effect::Changed(None)
                }
                0x62 => {
                    self.engine.move_cursor_syllable_left();
                    Effect::Changed(None)
                }
                _ => return None,
            });
        }
        None
    }

    /// 退格 / Esc / 回车 / Tab / 方向键；没在组句时都交还应用。
    fn apply_function_key(&mut self, event: &KeyEvent) -> Effect {
        if !self.composing() {
            // 回车交给应用：文本流里是一个段落边界（macOS 壳同样记）
            if event.virtual_key == codes::RETURN {
                self.engine.note_passthrough('\n');
            }
            return Effect::Passthrough;
        }
        // 只有一个 `?` 时按了回车：回车就是「把这个 ? 上屏」，吞掉，否则聊天框会连消息一起发出去；
        // 退格 / Esc 照常删掉它。其他功能键 macOS 壳还原后交给应用，Windows 放行同步、上屏异步，
        // 先动光标再插问号会插错位置，所以还原后一并吞掉。
        if self.engine.bare_question() && !matches!(event.virtual_key, codes::BACK | codes::ESCAPE)
        {
            let english = event.modifiers.caps;
            return Effect::Changed(Some(self.restore_bare_question(english)));
        }
        match event.virtual_key {
            codes::BACK => {
                self.engine.backspace();
                Effect::Changed(None)
            }
            codes::ESCAPE => {
                self.engine.clear();
                Effect::Changed(None)
            }
            codes::RETURN => Effect::Changed(Some(self.engine.take_raw())),
            codes::TAB if self.engine.english_mode() => {
                Effect::Changed(Some(self.commit_highlighted()))
            }
            // 中文模式 Tab：有整句补全就接受，否则交还应用（缩进 / 跳焦点）。
            codes::TAB => match self.sentence.take() {
                Some(sentence) => Effect::Changed(Some(self.engine.accept_prediction(&sentence))),
                None => Effect::Passthrough,
            },
            codes::DOWN => {
                self.move_highlight(1);
                Effect::Navigated
            }
            codes::UP => {
                self.move_highlight(-1);
                Effect::Navigated
            }
            codes::NEXT => {
                self.page(1);
                Effect::Navigated
            }
            codes::PRIOR => {
                self.page(-1);
                Effect::Navigated
            }
            codes::LEFT => {
                self.engine.move_cursor_left();
                Effect::Changed(None)
            }
            codes::RIGHT => {
                self.engine.move_cursor_right();
                Effect::Changed(None)
            }
            codes::HOME => {
                self.engine.move_cursor_home();
                Effect::Changed(None)
            }
            codes::END => {
                self.engine.move_cursor_end();
                Effect::Changed(None)
            }
            _ => Effect::Passthrough,
        }
    }

    /// 中文模式：小写字母进拼音；组句中的大写字母也进缓冲区（中英混输，`woxiangxueRust` → 我想学Rust）；
    /// 没在组句时的大写字母是临时打英文，直接放行。没在组句时的其他字符走全角标点（与 macOS 壳一致，
    /// 组句中的标点仍进英文直输段）。
    fn apply_chinese(&mut self, c: char, event: &KeyEvent) -> Effect {
        if c.is_ascii_uppercase() {
            if self.composing() {
                // 组句中：大写字母进缓冲区，交给 engine 做中英混输切分
                self.engine.push(c);
                return Effect::Changed(None);
            }
            // 没在组句：临时打英文，直接放行
            self.engine.note_passthrough(c);
            return Effect::Passthrough;
        }
        if c.is_ascii_lowercase() {
            self.engine.push(c);
            return Effect::Changed(None);
        }
        if !self.composing() {
            return self.apply_punctuation(c, event);
        }
        self.apply_printable(c, event)
    }

    /// 当前模式开着全角就让 Core 转（数字后的 `.` 保持半角）；转不了的原样交给应用并告知 Core。
    fn apply_punctuation(&mut self, c: char, event: &KeyEvent) -> Effect {
        let english = event.modifiers.caps;
        if self.full_width_for(english)
            && let Some(text) = self.engine.punctuate(c)
        {
            return Effect::Changed(Some(text.to_owned()));
        }
        self.engine.note_passthrough(c);
        Effect::Passthrough
    }

    /// 英文模式。开着候选：字母进缓冲区，空格 / 标点先把字母原样上屏（动过高亮的空格才选词）；
    /// 关着候选：字母由我们插入（大小写按 Shift）。其他键按英文模式那份全角设置转，转不了的交给应用。
    fn apply_english(&mut self, c: char, event: &KeyEvent) -> Effect {
        let composing = self.composing();
        let raw = composing.then(|| self.engine.take_raw());
        let effect = if c.is_ascii_alphabetic() {
            self.engine.note_passthrough(c);
            Effect::Changed(Some(c.to_string()))
        } else {
            self.apply_punctuation(c, event)
        };
        with_prefix(raw, effect, c)
    }

    /// 组句中的可打印键：数字选当前页第 N 个，翻页键翻页，空格上屏高亮，其余进英文直输段。
    /// 表达式模式（`v1+2`）里数字和运算符进算式；问字模式敲的还可能是码点（`u4e00`、`u+1f600`），数字与 `+` 进缓冲区；
    /// 微软 / 搜狗双拼的 `;` 是 ing 键，末尾有落单声母时进缓冲区。
    fn apply_printable(&mut self, c: char, event: &KeyEvent) -> Effect {
        let expression = self.engine.expression_mode();
        if (expression && shortcut::is_expression_char(c))
            || (self.engine.unicode_entry() && (c.is_ascii_digit() || c == '+'))
            || (c == ';' && self.engine.takes_semicolon())
        {
            self.engine.push(c);
            return Effect::Changed(None);
        }
        if let Some(digit) = codes::digit(event)
            && self.candidate_count() > 0
        {
            let page_size = self.config.page_size;
            let page = self.highlight / page_size;
            return Effect::Changed(self.commit_index(page * page_size + digit - 1));
        }
        if let Some(step) = codes::page_key(event, self.config.page_keys) {
            self.page(step);
            return Effect::Navigated;
        }
        if c == ' ' {
            return Effect::Changed(Some(self.commit_highlighted()));
        }
        // 中文候选后敲标点：先提交候选，再把标点作为文本流的一部分处理，避免整个缓冲区退化成英文直输。
        // 这样 `nihc,zdjm` / `veuiufme?` 都只需在最后按一次空格。
        if c.is_ascii_punctuation()
            && c != '\''
            && !(c == ';' && self.engine.takes_semicolon())
            && self
                .layout_candidate(self.highlight)
                .is_some_and(|candidate| {
                    matches!(
                        candidate.kind,
                        CandidateKind::Chinese | CandidateKind::Sentence
                    )
                })
        {
            let mut text = self.commit_highlighted();
            if self.full_width_for(event.modifiers.caps)
                && let Some(converted) = self.engine.punctuate(c)
            {
                text.push_str(converted);
            } else {
                self.engine.note_passthrough(c);
                text.push(c);
            }
            return Effect::Changed(Some(text));
        }
        // 表达式 / 问字模式下的其他字符不进缓冲区（与 macOS 壳一致）：先把高亮候选上屏，再按没在组句处理这个键。
        if c != '\'' && (expression || self.engine.question_mode()) {
            let committed = self.commit_highlighted();
            let effect = self.apply_punctuation(c, event);
            return with_prefix(Some(committed), effect, c);
        }
        self.engine.push(c);
        Effect::Changed(None)
    }

    /// 上屏高亮候选；没有候选时缓冲原样上屏。
    fn commit_highlighted(&mut self) -> String {
        match self.commit_index(self.highlight) {
            Some(text) => text,
            None => self.engine.take_raw(),
        }
    }

    fn composing(&self) -> bool {
        !self.engine.composition().is_empty()
    }
}
