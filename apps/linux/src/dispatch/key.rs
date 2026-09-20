//! 按键怎么作用到 Engine / 高亮上。分流规则与 macOS 壳的 `handle_text` / `handle_command`、
//! Windows 壳的 `dispatch::key::input` 对齐。
//!
//! fcitx5 送来的是 xkb keysym（`sym`）+ keycode + modifiers。字母数字与标点的 keysym 就是 ASCII；
//! 功能键的 keysym 是 `XK_BackSpace` / `XK_Return` / `XK_Escape` / `XK_Tab` / 方向键等。

use qingjian_core::{Engine, QUESTION_PREFIX, shortcut};
use qingjian_platform::Modifiers;

use super::{Dispatch, KeyOutcome, KeyResult};
use crate::frame::Frame;
use crate::session::kind_of;

/// fcitx5 修饰键位（与 xkb 一致）。
pub const MOD_SHIFT: u32 = 1;
pub const MOD_CAPS: u32 = 2;
pub const MOD_CTRL: u32 = 4;
pub const MOD_ALT: u32 = 8;
pub const MOD_SUPER: u32 = 64;

/// 常用 xkb keysym（与 X11 的 keysymdef.h 一致）。
const XK_BACKSPACE: u32 = 0xFF08;
const XK_TAB: u32 = 0xFF09;
const XK_RETURN: u32 = 0xFF0D;
const XK_ESCAPE: u32 = 0xFF1B;
const XK_DELETE: u32 = 0xFFFF;
const XK_HOME: u32 = 0xFF50;
const XK_LEFT: u32 = 0xFF51;
const XK_UP: u32 = 0xFF52;
const XK_RIGHT: u32 = 0xFF53;
const XK_DOWN: u32 = 0xFF54;
const XK_PAGE_UP: u32 = 0xFF55;
const XK_PAGE_DOWN: u32 = 0xFF56;
const XK_END: u32 = 0xFF57;
const XK_INSERT: u32 = 0xFF63;

/// 一次按键的输入。
pub struct KeyInput {
    /// xkb keysym。
    pub sym: u32,
    /// keycode（物理键，暂未用）。
    pub code: u32,
    /// 修饰键位掩码（见 `MOD_*`）。
    pub modifiers: u32,
    /// 该键产生的字符（已按修饰键处理过）；功能键为 `None`。
    pub character: Option<char>,
    /// 是否按键释放（key up）。
    pub is_release: bool,
    /// 当前应用标识（桌面进程名）。
    pub app: Option<String>,
}

/// 把一次按键作用到 Engine / 高亮上。
pub fn apply(dispatch: &mut Dispatch, engine: &mut Engine, input: &KeyInput) -> KeyOutcome {
    let composing = !engine.composition().is_empty();
    let _caps = input.modifiers & MOD_CAPS != 0;
    let shift = input.modifiers & MOD_SHIFT != 0;
    let ctrl = input.modifiers & MOD_CTRL != 0;
    let alt = input.modifiers & MOD_ALT != 0;
    let super_ = input.modifiers & MOD_SUPER != 0;
    let command_like = ctrl || alt || super_;

    // emacs 风格编辑键（仅组句中）：Ctrl-A/E 行首尾、Ctrl-W 删前一个音节、
    // Ctrl-U 删到行首、Alt-F/B 按音节跳光标。不组句时交给应用。
    if composing && let Some(outcome) = apply_emacs(dispatch, engine, input, ctrl, alt) {
        return outcome;
    }

    // 命令键组合（Ctrl/Alt/Super）除方向键外一律交给应用
    if command_like && !is_navigation(input.sym) {
        return KeyOutcome::passthrough();
    }

    // 功能键
    if let Some(outcome) = apply_function_key(dispatch, engine, input, composing) {
        return outcome;
    }

    let Some(c) = input.character.filter(|c| !c.is_control()) else {
        return KeyOutcome::passthrough();
    };

    // 组句中修饰键 + 数字是快捷键（上屏译词 / 删候选）；MVP 先只接数字选词，译词 / 删候选后续接
    // 缓冲区为空时敲 ? 先进问字模式
    if !composing && c == QUESTION_PREFIX {
        engine.set_english_mode(false);
        engine.push(c);
        dispatch.refresh(engine);
        return KeyOutcome::consumed(dispatch.current_frame(engine));
    }

    // MVP 阶段先默认中文模式：不靠 Caps Lock 切中英（fcitx5 的 KeyState::CapsLock 行为与 macOS 不同，
    // 后续接 fcitx5 的中英切换机制或配置开关）。字母一律进拼音。
    let english = false;
    let question = composing && engine.question_mode();
    // 英文模式下问字：Caps 让字母以大写送来，按小写收进问题
    let c = if question && english && c.is_ascii_uppercase() {
        c.to_ascii_lowercase()
    } else {
        c
    };

    // 只有一个 ? 时敲了字母以外的键：还原成问号上屏
    if question && !c.is_ascii_lowercase() && engine.bare_question() {
        let mark = restore_bare_question(dispatch, engine, english);
        if c == ' ' {
            return KeyOutcome::committed(mark, dispatch.current_frame(engine));
        }
        // 还原后按非组句状态继续处理这个键
        let prefix = mark;
        let outcome = apply(dispatch, engine, &non_question_input(input));
        return with_prefix(Some(prefix), outcome, c);
    }

    engine.set_english_mode(false);

    let effect = if english && !question {
        apply_english(dispatch, engine, c, shift)
    } else {
        apply_chinese(dispatch, engine, c, shift)
    };
    with_prefix(None, effect, c)
}

/// emacs 风格编辑键（仅组句中调用）。返回 `Some` 表示已处理，`None` 表示不是 emacs 键、交给后续逻辑。
///
/// - Ctrl-A：光标到行首（`move_cursor_home`）
/// - Ctrl-E：光标到行尾（`move_cursor_end`）
/// - Ctrl-W：删掉光标前一个音节（`delete_syllable_backward`）
/// - Ctrl-U：删掉光标前的全部拼音（`delete_to_start`）
/// - Alt-F：光标右跳一个音节（`move_cursor_syllable_right`）
/// - Alt-B：光标左跳一个音节（`move_cursor_syllable_left`）
///
/// 按 xkb keysym（字母的 sym 就是 ASCII 小写）+ 修饰键判断，不靠 `character`（Ctrl/Alt 会把字符变成控制字符）。
const EMACS_A: u32 = 0x61;
const EMACS_E: u32 = 0x65;
const EMACS_W: u32 = 0x77;
const EMACS_U: u32 = 0x75;
const EMACS_F: u32 = 0x66;
const EMACS_B: u32 = 0x62;

fn apply_emacs(
    dispatch: &mut Dispatch,
    engine: &mut Engine,
    input: &KeyInput,
    ctrl: bool,
    alt: bool,
) -> Option<KeyOutcome> {
    // Ctrl 组合：sym 是小写字母（0x61..0x7a）或大写字母（0x41..0x5a）
    if ctrl && !alt {
        let lower = if (b'A' as u32..=b'Z' as u32).contains(&input.sym) {
            input.sym + 32
        } else {
            input.sym
        };
        let outcome = match lower {
            EMACS_A => {
                engine.move_cursor_home();
                KeyOutcome::consumed(frame_after(dispatch, engine))
            }
            EMACS_E => {
                engine.move_cursor_end();
                KeyOutcome::consumed(frame_after(dispatch, engine))
            }
            EMACS_W => {
                engine.delete_syllable_backward();
                KeyOutcome::consumed(frame_after(dispatch, engine))
            }
            EMACS_U => {
                engine.delete_to_start();
                KeyOutcome::consumed(frame_after(dispatch, engine))
            }
            _ => return None,
        };
        return Some(outcome);
    }
    // Alt 组合：sym 是小写 / 大写字母
    if alt && !ctrl {
        let lower = if (b'A' as u32..=b'Z' as u32).contains(&input.sym) {
            input.sym + 32
        } else {
            input.sym
        };
        let outcome = match lower {
            EMACS_F => {
                engine.move_cursor_syllable_right();
                KeyOutcome::consumed(frame_after(dispatch, engine))
            }
            EMACS_B => {
                engine.move_cursor_syllable_left();
                KeyOutcome::consumed(frame_after(dispatch, engine))
            }
            _ => return None,
        };
        return Some(outcome);
    }
    None
}

/// 功能键处理：退格 / Esc / 回车 / Tab / 方向键 / 翻页 / Home / End / Delete。
fn apply_function_key(
    dispatch: &mut Dispatch,
    engine: &mut Engine,
    input: &KeyInput,
    composing: bool,
) -> Option<KeyOutcome> {
    let sym = input.sym;
    // 字母数字与标点的 keysym 不是这些
    if !is_function_key(sym) {
        return None;
    }
    if !composing {
        // 回车交给应用
        if sym == XK_RETURN {
            engine.note_passthrough('\n');
        }
        return Some(KeyOutcome::passthrough());
    }
    // 只有一个 ? 时按了回车：吞掉（「把这个 ? 上屏」）
    if engine.bare_question() && !matches!(sym, XK_BACKSPACE | XK_ESCAPE) {
        let english = input.modifiers & MOD_CAPS != 0;
        let mark = restore_bare_question(dispatch, engine, english);
        return Some(KeyOutcome::committed(mark, dispatch.current_frame(engine)));
    }
    let outcome = match sym {
        XK_BACKSPACE => {
            engine.backspace();
            KeyOutcome::consumed(frame_after(dispatch, engine))
        }
        XK_ESCAPE => {
            engine.clear();
            KeyOutcome::consumed(frame_after(dispatch, engine))
        }
        XK_RETURN => {
            let raw = engine.take_raw();
            KeyOutcome::committed(raw, frame_after(dispatch, engine))
        }
        XK_TAB => {
            if engine.english_mode() {
                let text = commit_highlighted(dispatch, engine);
                KeyOutcome::committed(text, frame_after(dispatch, engine))
            } else {
                // 中文模式 Tab：有整句补全就接受，否则交还应用
                match dispatch.take_sentence() {
                    Some(sentence) => {
                        let text = engine.accept_prediction(&sentence);
                        KeyOutcome::committed(text, frame_after(dispatch, engine))
                    }
                    None => KeyOutcome::passthrough(),
                }
            }
        }
        XK_DOWN => {
            dispatch.session().move_highlight(1);
            KeyOutcome::consumed(dispatch.current_frame(engine))
        }
        XK_UP => {
            dispatch.session().move_highlight(-1);
            KeyOutcome::consumed(dispatch.current_frame(engine))
        }
        XK_PAGE_DOWN | XK_INSERT => {
            dispatch.session().turn_page(1);
            engine.note_page_turn();
            KeyOutcome::consumed(dispatch.current_frame(engine))
        }
        XK_PAGE_UP => {
            dispatch.session().turn_page(-1);
            engine.note_page_turn();
            KeyOutcome::consumed(dispatch.current_frame(engine))
        }
        XK_LEFT => {
            engine.move_cursor_left();
            KeyOutcome::consumed(frame_after(dispatch, engine))
        }
        XK_RIGHT => {
            engine.move_cursor_right();
            KeyOutcome::consumed(frame_after(dispatch, engine))
        }
        XK_HOME => {
            engine.move_cursor_home();
            KeyOutcome::consumed(frame_after(dispatch, engine))
        }
        XK_END => {
            engine.move_cursor_end();
            KeyOutcome::consumed(frame_after(dispatch, engine))
        }
        XK_DELETE => {
            engine.delete_forward();
            KeyOutcome::consumed(frame_after(dispatch, engine))
        }
        _ => KeyOutcome::passthrough(),
    };
    Some(outcome)
}

/// 中文模式：小写字母进拼音；Shift 大写字母是临时打英文；其他字符走全角标点或进缓冲区。
fn apply_chinese(dispatch: &mut Dispatch, engine: &mut Engine, c: char, shift: bool) -> KeyOutcome {
    if c.is_ascii_uppercase() {
        // Shift 大写字母：临时打英文，先把拼音原样上屏
        let raw = (!engine.composition().is_empty())
            .then(|| engine.take_raw())
            .filter(|s| !s.is_empty());
        engine.note_passthrough(c);
        return with_prefix(raw, KeyOutcome::passthrough(), c);
    }
    if c.is_ascii_lowercase() {
        engine.push(c);
        dispatch.refresh(engine);
        return KeyOutcome::consumed(dispatch.current_frame(engine));
    }
    let composing = !engine.composition().is_empty();
    if !composing {
        return apply_punctuation(engine, c, shift);
    }
    apply_printable(dispatch, engine, c, shift)
}

/// 当前模式开着全角就让 Core 转；转不了的原样交给应用并告知 Core。
fn apply_punctuation(engine: &mut Engine, c: char, _shift: bool) -> KeyOutcome {
    if let Some(text) = engine.punctuate(c) {
        return KeyOutcome::committed(text.to_owned(), Frame::empty());
    }
    engine.note_passthrough(c);
    KeyOutcome::passthrough()
}

/// 英文模式。开着候选：字母进缓冲区，空格 / 标点先把字母原样上屏；关着候选：字母由我们插入。
fn apply_english(dispatch: &mut Dispatch, engine: &mut Engine, c: char, shift: bool) -> KeyOutcome {
    let composing = !engine.composition().is_empty();
    let raw = composing
        .then(|| engine.take_raw())
        .filter(|s| !s.is_empty());
    let effect = if c.is_ascii_alphabetic() {
        let letter = if shift {
            c.to_ascii_uppercase()
        } else {
            c.to_ascii_lowercase()
        };
        engine.note_passthrough(letter);
        KeyOutcome::committed(letter.to_string(), Frame::empty())
    } else {
        apply_punctuation(engine, c, shift)
    };
    with_prefix(raw, effect, c)
}

/// 组句中的可打印键：数字选当前页第 N 个，翻页键翻页，空格上屏高亮，其余进缓冲区或英文直输段。
fn apply_printable(
    dispatch: &mut Dispatch,
    engine: &mut Engine,
    c: char,
    _shift: bool,
) -> KeyOutcome {
    let expression = engine.expression_mode();
    if (expression && shortcut::is_expression_char(c))
        || (engine.unicode_entry() && (c.is_ascii_digit() || c == '+'))
        || (c == ';' && engine.takes_semicolon())
    {
        engine.push(c);
        dispatch.refresh(engine);
        return KeyOutcome::consumed(dispatch.current_frame(engine));
    }
    // 数字选词
    if let Some(digit) = digit(c)
        && dispatch.session().layout_len() > 0
    {
        let page_size = dispatch.page_size();
        let page = dispatch.session().page();
        let index = page * page_size + digit - 1;
        let text = commit_index(dispatch, engine, index);
        return KeyOutcome::committed(text, frame_after(dispatch, engine));
    }
    // 翻页键
    let (page_prev, page_next) = dispatch.page_keys();
    if c == page_prev {
        dispatch.session().turn_page(-1);
        engine.note_page_turn();
        return KeyOutcome::consumed(dispatch.current_frame(engine));
    }
    if c == page_next {
        dispatch.session().turn_page(1);
        engine.note_page_turn();
        return KeyOutcome::consumed(dispatch.current_frame(engine));
    }
    if c == ' ' {
        let mut text = commit_highlighted(dispatch, engine);
        append_punctuation_suffix(engine, &mut text);
        return KeyOutcome::committed(text, frame_after(dispatch, engine));
    }
    // 中文候选后敲标点：先提交候选，再把标点作为文本流的一部分处理，避免整个缓冲区退化成英文直输。
    // 这样 `nihc,zdjm` / `veuiufme?` 都只需在最后按一次空格。
    let highlighted = dispatch.session().highlighted();
    if c.is_ascii_punctuation()
        && c != '\''
        && !(c == ';' && engine.takes_semicolon())
        && dispatch
            .session()
            .candidate(highlighted)
            .is_some_and(|candidate| {
                matches!(
                    candidate.kind,
                    qingjian_core::CandidateKind::Chinese | qingjian_core::CandidateKind::Sentence
                )
            })
    {
        let mut text = commit_highlighted(dispatch, engine);
        if let Some(converted) = engine.punctuate(c) {
            text.push_str(converted);
        } else {
            engine.note_passthrough(c);
            text.push(c);
        }
        return KeyOutcome::committed(text, frame_after(dispatch, engine));
    }
    // 表达式 / 问字模式下的其他字符：先把高亮候选上屏，再按非组句处理
    if c != '\'' && (expression || engine.question_mode()) {
        let committed = commit_highlighted(dispatch, engine);
        let effect = apply_punctuation(engine, c, false);
        return with_prefix(Some(committed), effect, c);
    }
    // 其他可打印字符（含 `'`、半角标点进英文直输段）
    engine.push(c);
    dispatch.refresh(engine);
    KeyOutcome::consumed(dispatch.current_frame(engine))
}

/// 上屏高亮候选；没有候选时缓冲原样上屏。
fn commit_highlighted(dispatch: &mut Dispatch, engine: &mut Engine) -> String {
    let index = dispatch.session().highlighted();
    commit_index(dispatch, engine, index)
}

/// 选词后若缓冲区只剩标点，一次空格同时把标点转为中文标点并上屏。
/// 不能对任意剩余内容这么做，否则会破坏「选前缀、继续输入」的行为。
fn append_punctuation_suffix(engine: &mut Engine, text: &mut String) {
    let pending = engine.composition().text().to_owned();
    if pending.is_empty() || !pending.chars().all(|c| c.is_ascii_punctuation()) {
        return;
    }
    let raw = engine.take_raw();
    for c in raw.chars() {
        if let Some(converted) = engine.punctuate(c) {
            text.push_str(converted);
        } else {
            text.push(c);
        }
    }
}

/// 上屏第 `index` 个候选；没有候选时上屏拼音本身。
fn commit_index(dispatch: &mut Dispatch, engine: &mut Engine, index: usize) -> String {
    let candidate = dispatch.session().candidate(index);
    match candidate {
        Some(candidate) => {
            let text = engine.commit(&candidate);
            tracing::debug!(%text, "commit");
            dispatch.refresh(engine);
            text
        }
        None => {
            if index < dispatch.session().layout_len() {
                String::new()
            } else {
                engine.take_raw()
            }
        }
    }
}

/// 缓冲区里只有一个 ?：清掉，还原成问号上屏。
fn restore_bare_question(dispatch: &mut Dispatch, engine: &mut Engine, english: bool) -> String {
    let mark = engine.restore_bare_question(english);
    dispatch.refresh(engine);
    mark.unwrap_or_else(|| QUESTION_PREFIX.to_string())
}

/// 刷新后取帧（组句中才有帧）。
fn frame_after(dispatch: &mut Dispatch, engine: &mut Engine) -> Frame {
    if engine.composition().is_empty() {
        Frame::empty()
    } else {
        dispatch.refresh(engine);
        dispatch.current_frame(engine)
    }
}

/// 把先行上屏的文本接到本次结果前面。
fn with_prefix(prefix: Option<String>, effect: KeyOutcome, _c: char) -> KeyOutcome {
    let Some(mut prefix) = prefix else {
        return effect;
    };
    prefix.push_str(&effect.commit);
    KeyOutcome {
        outcome: if prefix.is_empty() {
            effect.outcome
        } else {
            KeyResult::Committed
        },
        frame: effect.frame,
        commit: prefix,
    }
}

/// 字符是不是数字 1–9。
fn digit(c: char) -> Option<usize> {
    ('1'..='9')
        .contains(&c)
        .then_some((c as u8 - b'1' + 1) as usize)
}

/// keysym 是不是功能键。
fn is_function_key(sym: u32) -> bool {
    matches!(
        sym,
        XK_BACKSPACE
            | XK_TAB
            | XK_RETURN
            | XK_ESCAPE
            | XK_DELETE
            | XK_HOME
            | XK_LEFT
            | XK_UP
            | XK_RIGHT
            | XK_DOWN
            | XK_PAGE_UP
            | XK_PAGE_DOWN
            | XK_END
            | XK_INSERT
    )
}

/// keysym 是不是导航键（Ctrl/Alt/Super 组合下仍由输入法处理的方向键等）。
fn is_navigation(sym: u32) -> bool {
    matches!(
        sym,
        XK_LEFT | XK_UP | XK_RIGHT | XK_DOWN | XK_HOME | XK_END | XK_PAGE_UP | XK_PAGE_DOWN
    )
}

/// 把一个输入复制成「不在问字模式」的版本（用于还原 ? 后重新分派）。
fn non_question_input(input: &KeyInput) -> KeyInput {
    KeyInput {
        sym: input.sym,
        code: input.code,
        modifiers: input.modifiers,
        character: input.character,
        is_release: input.is_release,
        app: input.app.clone(),
    }
}

// 占位：Modifiers 在配置里用，这里引用避免未使用警告
#[allow(dead_code)]
fn _ensure_modifiers() {
    let _ = Modifiers::default();
    let _ = kind_of(qingjian_core::CandidateKind::Chinese);
}
