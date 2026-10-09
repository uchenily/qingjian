use super::*;

/// 「拼音头 + 英文尾」的切法：`woxiangxuehaorust` 切成头 `woxiangxuehao` 与尾 rust。
/// 见 [`Engine::split_english_tail`]。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EnglishTail {
    /// 英文词前的双拼 / 拼音占作用域开头多少字节。
    pub head_len: usize,

    /// 英文词在原始输入中的结束位置；旧的英文尾段这里等于整个作用域长度。
    pub word_end: usize,

    /// 英文词，按词表里的写法（`api` → API）。
    pub word: String,

    /// 整段字母也能读成拼音（`database` → da ta ba se，`woxiangxuehaorust` → … ru s… t… 简拼）：两种读法要比分，
    /// 见 [`Engine::mixed_beats_plain`]；整段根本切不成拼音的（`wodeid` 的 i、`woyongvim` 的 v）直接按英文读。
    pub competes: bool,

    /// 这个英文词的 log 概率（按词表词频估），比分用。
    pub log_prob: f64,
}

/// wordfreq 的 Zipf 频率（词表里 ×1000 存）换成 log 概率：Zipf z 是每十亿词里 10^z 次，即 log P = (z − 9)·ln 10。
/// 没有词频的（个人表里的词、`kubectl` 这类技术词）按 Zipf 3（百万分之一）算。
pub(crate) fn english_log_prob(frequency: Option<u32>) -> f64 {
    let zipf = frequency
        .map(|f| f64::from(f) / 1000.0)
        .filter(|z| *z > 0.0)
        .unwrap_or(ENGLISH_ZIPF_FLOOR)
        .max(ENGLISH_ZIPF_FLOOR);
    (zipf - 9.0) * std::f64::consts::LN_10
}

impl Engine {
    /// 整段输入的末尾是不是一个英文词：`woxiangxuehaorust` → 我想学好 + rust。
    ///
    /// 尾段要在英文词表里（个人表或随包表），头段要能切成每个音节都完整的拼音。
    /// 尾段自己就是完整拼音的（`database`、`fan`）要至少 [`MIN_PINYIN_LIKE_TAIL_LETTERS`] 个字母；
    /// 两个字母的尾段只认缩写词（ID / TV / OK）和个人表里的词：`to` / `it` 这类太容易撞上简拼。
    /// 同时满足的取最长的尾段（`wodedatabase` 取 database 不取 base）。双拼按完整双拼音节边界切头段；带 `'` 的输入不切；
    /// 整段本身是英文词（`agent`）、拼音不像话时纠错能纠通（`shiide` → 是的）或有英文补全（`releas` → release）的也不切，
    /// 那几条路本来就排第一。
    /// 切出来只说明「可以这么读」，与拼音读法谁排前面看 `competes` 与比分。
    pub(crate) fn split_english_tail(&self, scope: &str) -> Option<EnglishTail> {
        if scope.len() < MIN_ENGLISH_TAIL_HEAD_LETTERS + MIN_ENGLISH_TAIL_LETTERS
            || !scope.bytes().all(|b| b.is_ascii_alphabetic())
        {
            return None;
        }
        let shuangpin = self.shuangpin.is_some();
        let lists = self.english_lists();
        if lists.is_empty() {
            return None;
        }
        if shuangpin {
            return self.split_shuangpin_english_tail(scope, &lists);
        }
        if lists.iter().any(|words| words.get(scope).is_some()) {
            return None;
        }
        let full = (!shuangpin).then(|| parser::segment(scope).ok()).flatten();
        let unlikely = correction::unlikely_pinyin(full.as_ref().and_then(|s| s.first()), "")
            || correction::trailing_single_letter(full.as_ref().and_then(|s| s.first()));
        if !shuangpin && (full.is_none() || unlikely) {
            // 拼写纠错能把整段纠成通顺的拼音（`shiide` → 是的，`yingagi` → 应该）：那是敲错，不是英文
            if self.active_correction(scope).is_some() {
                return None;
            }
            if scope.len() >= MIN_COMPLETION_LETTERS
                && lists
                    .iter()
                    .any(|words| !words.complete(scope, 1).is_empty())
            {
                return None;
            }
        }
        let personal = self.learner.user_english();
        // 全拼：英文词可在句首 / 句中 / 句末，前后拼音段都要能完整切分（空段算合法）。
        // 双拼走 split_shuangpin_english_tail，这里只处理全拼。
        if !shuangpin {
            return self.split_fullpinyin_english_tail(scope, &lists, personal, full.is_some());
        }
        None
    }

    /// 全拼下在整段输入中寻找英文词。英文词可在句首 / 句中 / 句末，前后拼音段都要能完整切分（空段算合法）。
    /// 取最长的英文词（`wodedatabase` 取 database 不取 base）；整段本身是英文词的已在调用方排除。
    fn split_fullpinyin_english_tail(
        &self,
        scope: &str,
        lists: &[&qingjian_dictionary::WordList],
        personal: Option<&qingjian_dictionary::WordList>,
        full_segmentable: bool,
    ) -> Option<EnglishTail> {
        let min_word = MIN_ENGLISH_TAIL_LETTERS;
        let max_start = scope.len().saturating_sub(min_word);
        let mut best: Option<EnglishTail> = None;
        for start in 0..=max_start {
            for end in ((start + min_word)..=scope.len()).rev() {
                let typed = &scope[start..end];
                let typed_lower = typed.to_ascii_lowercase();
                let Some(words) = lists.iter().find(|words| words.get(&typed_lower).is_some())
                else {
                    continue;
                };
                let word = words.get(&typed_lower)?;
                let acronym = word.bytes().any(|b| b.is_ascii_uppercase());
                let known = personal.is_some_and(|known| known.get(&typed_lower).is_some());
                let len = end - start;
                if len == min_word && !acronym && !known {
                    continue;
                }
                if parser::is_fully_segmentable(&typed_lower) && len < MIN_PINYIN_LIKE_TAIL_LETTERS {
                    continue;
                }
                // 前后拼音段都要能完整切分（空段算合法）
                if start > 0 && !parser::is_fully_segmentable(&scope[..start]) {
                    continue;
                }
                if end < scope.len() && !parser::is_fully_segmentable(&scope[end..]) {
                    continue;
                }
                let tail = EnglishTail {
                    head_len: start,
                    word_end: end,
                    // 用户输入全小写时用词表原形（`id` → ID、`github` → GitHub）；
                    // 用户输入含大写时尊重用户输入（`Rust` → Rust）。
                    word: if typed.bytes().all(|b| b.is_ascii_lowercase()) {
                        word.to_owned()
                    } else {
                        typed.to_owned()
                    },
                    // 英文词本身也能切成完整拼音时（`database` → da ta ba se），两种读法要比分。
                    competes: full_segmentable,
                    log_prob: english_log_prob(words.frequency(&typed_lower)),
                };
                // 取最长的英文词；同长取靠后的（句末优先，与原行为一致）
                if best.as_ref().is_none_or(|b| end - start > b.word_end - b.head_len) {
                    best = Some(tail);
                }
                break;
            }
        }
        best
    }

    /// 在双拼原始键串中寻找一个英文词。英文词可以位于句首、句中或句末，前后都必须是完整双拼音节（句首 / 句末的英文词另一侧为空）。
    fn split_shuangpin_english_tail(
        &self,
        scope: &str,
        lists: &[&qingjian_dictionary::WordList],
    ) -> Option<EnglishTail> {
        if lists.iter().any(|words| words.get(scope).is_some()) {
            return None;
        }
        let personal = self.learner.user_english();
        let min_word = MIN_SHUANGPIN_ENGLISH_TAIL_LETTERS;
        let max_start = scope.len().saturating_sub(min_word);
        for start in 0..=max_start {
            if !scope.is_char_boundary(start) {
                continue;
            }
            // 英文词前的拼音头段：空（句首）或全部解成音节、没有解不动的尾巴。末尾单声母键（`z`=声母 z）算合法头段结尾。
            if start > 0 {
                let Some(prefix) = self.decode(&scope[..start]) else {
                    continue;
                };
                if !prefix.tail().is_empty() || prefix.segmentation().is_none() {
                    continue;
                }
            }
            for end in ((start + min_word)..=scope.len()).rev() {
                if !scope.is_char_boundary(end) {
                    continue;
                }
                let typed = &scope[start..end];
                let typed_lower = typed.to_ascii_lowercase();
                let Some(words) = lists.iter().find(|words| words.get(&typed_lower).is_some())
                else {
                    continue;
                };
                let word = words.get(&typed_lower)?;
                let acronym = word.bytes().any(|b| b.is_ascii_uppercase());
                let known = personal.is_some_and(|known| known.get(&typed_lower).is_some());
                if end - start < min_word && !acronym && !known {
                    continue;
                }
                if parser::is_fully_segmentable(&typed_lower)
                    && end - start < MIN_PINYIN_LIKE_TAIL_LETTERS
                {
                    continue;
                }
                if end < scope.len() {
                    let suffix = self.decode(&scope[end..])?;
                    if !suffix.tail().is_empty() || suffix.segmentation().is_none() {
                        continue;
                    }
                }
                return Some(EnglishTail {
                    head_len: start,
                    word_end: end,
                    // 用户输入全小写时用词表原形（`id` → ID、`github` → GitHub）；
                    // 用户输入含大写时尊重用户输入（`Rust` → Rust）。
                    word: if typed.bytes().all(|b| b.is_ascii_lowercase()) {
                        word.to_owned()
                    } else {
                        typed.to_owned()
                    },
                    // 尾段也能解码成完整双拼时（`meds` → me+ds → 么东，与英文 meds 竞争），
                    // 要和拼音整句比分，不能直接采用英文读法。
                    competes: self.decode(typed).is_some_and(|d| d.is_complete()),
                    log_prob: english_log_prob(words.frequency(&typed_lower)),
                });
            }
        }
        None
    }

    /// 整段也能读成拼音时两种读法比分：前拼音段 + 英文词 + 后拼音段的得分加英文词的 log 概率、扣掉切到英文的代价，
    /// 高过整段按拼音读的整句就按英文读。拼音读法末尾的单字母也读（`huoz` → 或者，不是 或 + 丢掉 z），两边覆盖同样多的字母才公平。
    /// `wodedatabase`：我的 + database 赢过 我的大塔巴瑟；`womenqubeijing`：我们去北京 赢过 我们去 + Beijing；
    /// `taida`：太大 赢过 他 + Ida；`huoz`：或者 赢过 和 + Oz。句首 / 句中英文词（`luayuyjhcxtma`、`wozdebugzhege`）同理。
    pub(super) fn mixed_beats_plain(&self, scope: &str, tail: &EnglishTail) -> bool {
        // 双拼下 scope 是原始键串，parser::segment 切不动；先 decode 成全拼再切。
        let decode = |text: &str| -> Option<String> {
            if self.shuangpin.is_some() {
                self.decode(text).map(|d| d.pinyin().to_owned())
            } else {
                Some(text.to_owned())
            }
        };
        let convert = |text: &str, whole: bool| {
            let pinyin = decode(text)?;
            let segmentations = parser::segment(&pinyin).ok()?;
            self.convert_sentence_static(&segmentations.first()?.patterns(), whole)
        };
        // 英文词把整段切成前拼音段 + 英文词 + 后拼音段。英文读法的得分 = 前段分 + 后段分 + 英文词 log 概率 − 切换代价。
        // 空段（句首 / 句末）不计分。整段按拼音读的整句作为对手。
        let head_score = if tail.head_len == 0 {
            Some(0.0)
        } else {
            convert(&scope[..tail.head_len], false).map(|c| c.score)
        };
        let suffix = &scope[tail.word_end..];
        let suffix_score = if suffix.is_empty() {
            Some(0.0)
        } else if self.shuangpin.is_some() {
            convert(suffix, false).map(|c| c.score)
        } else {
            convert(suffix, false).map(|c| c.score)
        };
        let (Some(head), Some(suffix_s), Some(plain)) =
            (head_score, suffix_score, convert(scope, true))
        else {
            return false;
        };
        head + suffix_s + tail.log_prob - ENGLISH_SWITCH_PENALTY > plain.score
    }

    /// 头段拼音转成的汉字加上英文尾段：候选的音节是头段的全拼音节加上敲的尾段字母（上屏按它们消耗拼音）。
    /// 头段有占位音节的不出。
    pub(super) fn mixed_sentence(
        &self,
        head: &Segmentation,
        tail: &EnglishTail,
        typos: bool,
    ) -> Option<Candidate> {
        let keys = self.composition.scope();
        if self.shuangpin.is_none() {
            // 前拼音段 + 英文词 + 后拼音段。句首英文词 head_len=0（前段空），句末 word_end=keys.len()（后段空）。
            let head_text = if tail.head_len == 0 {
                String::new()
            } else {
                let conversion = self.convert_sentence(&head.patterns(), typos)?;
                if conversion.has_placeholder() {
                    return None;
                }
                conversion.text
            };
            let suffix_keys = &keys[tail.word_end..];
            let suffix_text = if suffix_keys.is_empty() {
                String::new()
            } else {
                let suffix_seg = parser::segment(suffix_keys).ok()?.into_iter().next()?;
                let conversion = self.convert_sentence(&suffix_seg.patterns(), typos)?;
                if conversion.has_placeholder() {
                    return None;
                }
                conversion.text
            };
            let typed = &keys[tail.head_len..tail.word_end];
            let mut syllables = Vec::new();
            if tail.head_len > 0 {
                let head_conversion = self.convert_sentence(&head.patterns(), typos)?;
                syllables.extend(head_conversion.syllables);
            }
            syllables.push(typed.to_owned());
            if !suffix_keys.is_empty() {
                let suffix_seg = parser::segment(suffix_keys).ok()?.into_iter().next()?;
                let suffix_conversion = self.convert_sentence(&suffix_seg.patterns(), typos)?;
                syllables.extend(suffix_conversion.syllables);
            }
            return Some(Candidate {
                text: format!("{}{}{}", head_text, tail.word, suffix_text),
                kind: CandidateKind::Sentence,
                syllables,
                reading: None,
                translation: None,
            });
        }
        let (combined, prefix_len) = if self.shuangpin.is_some() {
            let prefix = self.decode(&keys[..tail.head_len])?;
            let suffix = self.decode(&keys[tail.word_end..]);
            let mut combined = prefix.pinyin().to_owned();
            if let Some(suffix) = suffix {
                if !suffix.tail().is_empty() {
                    return None;
                }
                if !suffix.pinyin().is_empty() {
                    if !combined.is_empty() {
                        combined.push('\'');
                    }
                    combined.push_str(suffix.pinyin());
                }
            }
            // 句首英文词（head_len=0）：prefix 为空，prefix_len=0，英文词插在最前
            let prefix_len = if tail.head_len == 0 {
                0
            } else {
                prefix.segmentation()?.syllables.len()
            };
            (combined, prefix_len)
        } else {
            (keys[..tail.word_end].to_owned(), head.syllables.len())
        };
        let segmentation = parser::segment(&combined).ok()?.into_iter().next()?;
        let conversion = self.convert_sentence(&segmentation.patterns(), typos)?;
        if conversion.has_placeholder() {
            return None;
        }
        let prefix_text = if prefix_len == 0 {
            String::new()
        } else {
            let prefix = parser::segment(&combined).ok()?.into_iter().next()?;
            self.convert_sentence(
                &prefix.syllables[..prefix_len]
                    .iter()
                    .map(|s| s.pattern())
                    .collect::<Vec<_>>(),
                typos,
            )?
            .text
        };
        let suffix_text = &conversion.text[prefix_text.len()..];
        let typed = &keys[tail.head_len..tail.word_end];
        let mut syllables = conversion.syllables;
        syllables.insert(prefix_len, typed.to_owned());
        Some(Candidate {
            text: format!("{}{}{}", prefix_text, tail.word, suffix_text),
            kind: CandidateKind::Sentence,
            syllables,
            reading: None,
            translation: None,
        })
    }
}
