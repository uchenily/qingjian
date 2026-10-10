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

/// 多张英文词表里取词频最高的（随包表的 wordfreq 远高于个人表的选择次数），
/// 给 `english_log_prob` 估英文词的 log 概率用。
fn best_frequency(lists: &[&qingjian_dictionary::WordList], code: &str) -> Option<u32> {
    lists.iter().filter_map(|words| words.frequency(code)).max()
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
        // 取最长的英文词（`wodedatabase` 取 database 不取 base）；同长取靠后的（句末优先，与原行为一致）。
        let min_word = MIN_ENGLISH_TAIL_LETTERS;
        let max_start = scope.len().saturating_sub(min_word);
        let mut best: Option<EnglishTail> = None;
        for start in 0..=max_start {
            if !scope.is_char_boundary(start) {
                continue;
            }
            // 前拼音段要能完整切分（空段算合法）
            if start > 0 && !parser::is_fully_segmentable(&scope[..start]) {
                continue;
            }
            for end in ((start + min_word)..=scope.len()).rev() {
                if !scope.is_char_boundary(end) {
                    continue;
                }
                let typed = &scope[start..end];
                let typed_lower = typed.to_ascii_lowercase();
                let Some(words) =
                    lists.iter().find(|words| words.get(&typed_lower).is_some())
                else {
                    continue;
                };
                let Some(word) = words.get(&typed_lower) else {
                    continue;
                };
                let acronym = word.bytes().any(|b| b.is_ascii_uppercase());
                let known =
                    personal.is_some_and(|known| known.get(&typed_lower).is_some());
                let len = end - start;
                if len == min_word && !acronym && !known {
                    continue;
                }
                if parser::is_fully_segmentable(&typed_lower)
                    && len < MIN_PINYIN_LIKE_TAIL_LETTERS
                {
                    continue;
                }
                // 后拼音段要能完整切分（空段算合法）
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
                    competes: full.is_some(),
                    log_prob: english_log_prob(best_frequency(&lists, &typed_lower)),
                };
                if best.as_ref().is_none_or(|b| end - start > b.word_end - b.head_len) {
                    best = Some(tail);
                }
                break;
            }
        }
        best
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
        let min_word = MIN_SHUANGPIN_ENGLISH_TAIL_LETTERS;
        let max_start = scope.len().saturating_sub(min_word);
        let mut best: Option<EnglishTail> = None;
        for start in 0..=max_start {
            if !scope.is_char_boundary(start) {
                continue;
            }
            // 英文词前的拼音头段：空（句首英文词）或全部解成完整音节、没有解不动的尾巴。
            // 末尾落单的声母键（`f` 可能要打 `fa`）不算合法头段结尾——会误切 `fwihhc` 这种。
            if start > 0 {
                let Some(prefix) = self.decode(&scope[..start]) else {
                    continue;
                };
                if !prefix.is_complete() || prefix.segmentation().is_none() {
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
                let Some(word) = words.get(&typed_lower) else {
                    continue;
                };
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
                    let Some(suffix) = self.decode(&scope[end..]) else {
                        continue;
                    };
                    if !suffix.is_complete() || suffix.segmentation().is_none() {
                        continue;
                    }
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
                    // 尾段也能解码成完整双拼时（`meds` → me+ds → 么东，与英文 meds 竞争），
                    // 要和拼音整句比分，不能直接采用英文读法。
                    competes: self.decode(typed).is_some_and(|d| d.is_complete()),
                    log_prob: english_log_prob(best_frequency(lists, &typed_lower)),
                };
                // 取最长的英文词（`wodedatabase` 取 database 不取 DE）；同长取靠后的（句末优先）。
                if best.as_ref().is_none_or(|b| end - start > b.word_end - b.head_len) {
                    best = Some(tail);
                }
                break;
            }
        }
        best
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
            self.convert_sentence_static(&segmentations.first()?.patterns(), whole)
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
        // 句中英文词有后拼音段，后段分数也要算进去（`zvjibfdeyrzejquibungiidc` 的后段
        // 是「的原则就是不能吃到」，不算的话头段「最基」单独分高会误判英文赢）。
        let suffix_score = if tail.word_end < scope.len() {
            match convert(&scope[tail.word_end..], false) {
                Some(suffix) if !suffix.has_placeholder() => suffix.score,
                _ => return false,
            }
        } else {
            0.0
        };
        // 用户选过这个英文词（`nice` 选多次）时加分，与拼音整句里的用户加分对等：
        // 不加的话 `mixed_beats_plain` 只看静态分数，用户选再多次英文切法也赢不了拼音。
        let english_weight = self.learner.weight(&tail.word);
        let english_bonus = ranking::weight_bonus(english_weight);
        // 用户选过这个英文词时不扣切换代价：冷启动时 penalty 拦住误切（`womenqubeijing` 不出 我们去Beijing），
        // 但用户教过 `nice` 就该让它赢，penalty 3 nat 会压过 weight_bonus（封顶 ~2.4）。
        let penalty = if english_weight > 0 { 0.0 } else { ENGLISH_SWITCH_PENALTY };
        head.score + suffix_score + tail.log_prob + english_bonus - penalty > plain.score
    }

    /// 拼音段转成的汉字加上英文词（及后拼音段）：`woxiangxuehaorust` → 我想学好 + rust、
    /// `luayuyjhcxtma` → lua + 语言好学吗、`wozddebugvege` → 我在 + debug + 这个。
    /// 候选的音节是各拼音段的全拼音节加上敲的英文词字母（上屏按它们消耗拼音）。有占位音节的不出。
    pub(super) fn mixed_sentence(
        &self,
        tail: &EnglishTail,
        typos: bool,
    ) -> Option<Candidate> {
        let keys = self.composition.scope();
        if self.shuangpin.is_none() {
            // 全拼：前拼音段 + 英文词 [+ 后拼音段]。后拼音段也按完整切分转换。
            let head_pinyin = &keys[..tail.head_len];
            let suffix_pinyin = &keys[tail.word_end..];
            let head_seg = if head_pinyin.is_empty() {
                None
            } else {
                parser::segment(head_pinyin).ok()?.into_iter().next()
            };
            let suffix_seg = if suffix_pinyin.is_empty() {
                None
            } else {
                parser::segment(suffix_pinyin).ok()?.into_iter().next()
            };
            let head_text = if let Some(seg) = &head_seg {
                let conv = self.convert_sentence(&seg.patterns(), typos)?;
                if conv.has_placeholder() {
                    return None;
                }
                conv.text
            } else {
                String::new()
            };
            let suffix_text = if let Some(seg) = &suffix_seg {
                let conv = self.convert_sentence(&seg.patterns(), typos)?;
                if conv.has_placeholder() {
                    return None;
                }
                conv.text
            } else {
                String::new()
            };
            let mut syllables = Vec::new();
            if let Some(seg) = &head_seg {
                syllables.extend(seg.syllables.iter().map(|s| s.text.clone()));
            }
            syllables.push(keys[tail.head_len..tail.word_end].to_owned());
            if let Some(seg) = &suffix_seg {
                syllables.extend(seg.syllables.iter().map(|s| s.text.clone()));
            }
            return Some(Candidate {
                text: format!("{}{}{}", head_text, tail.word, suffix_text),
                kind: CandidateKind::Sentence,
                syllables,
                reading: None,
                translation: None,
            });
        }
        // 双拼：前拼音段 + 英文词 [+ 后拼音段]，各段按双拼解码后的全拼分别转换。
        let prefix = self.decode(&keys[..tail.head_len]);
        let suffix = self.decode(&keys[tail.word_end..]);
        // 头段为空（句首英文词）或完整双拼音节；后段同理。
        let head_text = if tail.head_len == 0 {
            String::new()
        } else {
            let prefix = prefix?;
            if !prefix.is_complete() || prefix.segmentation().is_none() {
                return None;
            }
            let seg = prefix.segmentation()?;
            let conv = self.convert_sentence(&seg.patterns(), typos)?;
            if conv.has_placeholder() {
                return None;
            }
            conv.text
        };
        let suffix_text = if tail.word_end == keys.len() {
            String::new()
        } else {
            let suffix = suffix?;
            if !suffix.is_complete() || suffix.segmentation().is_none() {
                return None;
            }
            let seg = suffix.segmentation()?;
            let conv = self.convert_sentence(&seg.patterns(), typos)?;
            if conv.has_placeholder() {
                return None;
            }
            conv.text
        };
        let mut syllables = Vec::new();
        if tail.head_len > 0 {
            let prefix = self.decode(&keys[..tail.head_len])?;
            syllables.extend(prefix.segmentation()?.syllables.iter().map(|s| s.text.clone()));
        }
        syllables.push(keys[tail.head_len..tail.word_end].to_owned());
        if tail.word_end < keys.len() {
            let suffix = self.decode(&keys[tail.word_end..])?;
            syllables.extend(suffix.segmentation()?.syllables.iter().map(|s| s.text.clone()));
        }
        Some(Candidate {
            text: format!("{}{}{}", head_text, tail.word, suffix_text),
            kind: CandidateKind::Sentence,
            syllables,
            reading: None,
            translation: None,
        })
    }
}
