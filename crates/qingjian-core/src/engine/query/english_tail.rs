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
        let longest = scope.len() - MIN_ENGLISH_TAIL_HEAD_LETTERS;
        (MIN_ENGLISH_TAIL_LETTERS..=longest).rev().find_map(|len| {
            let head_len = scope.len() - len;
            let tail = &scope[head_len..];
            let tail_lower = tail.to_ascii_lowercase();
            let words = lists
                .iter()
                .find(|words| words.get(&tail_lower).is_some())?;
            let word = words.get(&tail_lower)?;
            let acronym = word.bytes().any(|b| b.is_ascii_uppercase());
            let known = personal.is_some_and(|words| words.get(&tail_lower).is_some());
            if len == MIN_ENGLISH_TAIL_LETTERS && !acronym && !known {
                return None;
            }
            if parser::is_fully_segmentable(&tail_lower) && len < MIN_PINYIN_LIKE_TAIL_LETTERS {
                return None;
            }
            if shuangpin {
                let decoded = self.decode(&scope[..head_len])?;
                if !decoded.is_complete() || decoded.segmentation().is_none() {
                    return None;
                }
            } else if !parser::is_fully_segmentable(&scope[..head_len]) {
                return None;
            }
            Some(EnglishTail {
                head_len,
                word_end: scope.len(),
                word: word.to_owned(),
                // 双拼前缀已经按音节边界解码，尾部是否也能解码不应阻止英文尾段。
                competes: !shuangpin && full.is_some(),
                log_prob: english_log_prob(words.frequency(tail)),
            })
        })
    }

    /// 在双拼原始键串中寻找一个英文词。英文词可以位于句中，前后都必须是完整双拼音节。
    fn split_shuangpin_english_tail(
        &self,
        scope: &str,
        lists: &[&qingjian_dictionary::WordList],
    ) -> Option<EnglishTail> {
        if lists.iter().any(|words| words.get(scope).is_some()) {
            return None;
        }
        let personal = self.learner.user_english();
        let min_head = MIN_ENGLISH_TAIL_HEAD_LETTERS;
        let min_word = MIN_SHUANGPIN_ENGLISH_TAIL_LETTERS;
        let max_start = scope.len().saturating_sub(min_word);
        for start in min_head..=max_start {
            if !scope.is_char_boundary(start) {
                continue;
            }
            let Some(prefix) = self.decode(&scope[..start]) else {
                continue;
            };
            if !prefix.is_complete() || prefix.segmentation().is_none() {
                continue;
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
                    if !suffix.is_complete() || suffix.segmentation().is_none() {
                        continue;
                    }
                }
                return Some(EnglishTail {
                    head_len: start,
                    word_end: end,
                    word: word.to_owned(),
                    // 尾段也能解码成完整双拼时（`meds` → me+ds → 么东，与英文 meds 竞争），
                    // 要和拼音整句比分，不能直接采用英文读法。
                    competes: self.decode(typed).is_some_and(|d| d.is_complete()),
                    log_prob: english_log_prob(words.frequency(typed)),
                });
            }
        }
        None
    }

    /// 整段也能读成拼音时两种读法比分：头段整句的得分加英文词的 log 概率、扣掉切到英文的代价，高过整段按拼音读的整句就按英文读。
    /// 拼音读法末尾的单字母也读（`huoz` → 或者，不是 或 + 丢掉 z），两边覆盖同样多的字母才公平。
    /// `wodedatabase`：我的 + database 赢过 我的大塔巴瑟；`womenqubeijing`：我们去北京 赢过 我们去 + Beijing；
    /// `taida`：太大 赢过 他 + Ida；`huoz`：或者 赢过 和 + Oz。
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
            self.convert_sentence_with(&segmentations.first()?.patterns(), true, whole)
        };
        let (Some(head), Some(plain)) = (
            convert(&scope[..tail.head_len], false),
            convert(scope, true),
        ) else {
            return false;
        };
        if head.has_placeholder() {
            return false;
        }
        head.score + tail.log_prob - ENGLISH_SWITCH_PENALTY > plain.score
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
            let conversion = self.convert_sentence(&head.patterns(), typos)?;
            if conversion.has_placeholder() {
                return None;
            }
            let typed = &keys[tail.head_len..];
            let mut syllables = conversion.syllables;
            syllables.push(typed.to_owned());
            return Some(Candidate {
                text: format!("{}{}", conversion.text, tail.word),
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
                if !suffix.is_complete() {
                    return None;
                }
                if !suffix.pinyin().is_empty() {
                    if !combined.is_empty() {
                        combined.push('\'');
                    }
                    combined.push_str(suffix.pinyin());
                }
            }
            let prefix_len = prefix.segmentation()?.syllables.len();
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
