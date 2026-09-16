//! 装配 Engine：Linux 壳里唯一知道具体 Translator / Learner 类型的地方，装的东西与 macOS 的 `host::init`、
//! Windows 的 `assembly::assemble` 一致。

use std::path::Path;
use std::time::Instant;

use qingjian_core::{EmojiTable, Engine, Language};
use qingjian_dictionary::{Dictionary, WordList};
use qingjian_learning::{FrequencyLearner, InputLog, UsageStats, VocabularyBook};
use qingjian_lm::BigramModel;
use qingjian_platform::extra_dictionaries;
use qingjian_platform::{Config, ConfigError, LogLevel};
use qingjian_translate::{Glossary, LayeredTranslator, LevelTable, PersonalGlossary};

use crate::dispatch::Dispatch;
use crate::error::InitError;
use crate::paths;

/// 装配好的进程级单例。
pub(crate) struct Host {
    pub(crate) engine: Engine,
    pub(crate) dispatch: Dispatch,
}

/// 随包释义表覆盖的语言（与 macOS 一致；有表才算）。
const GLOSSARY_LANGUAGES: [Language; 2] = [Language::English, Language::Japanese];

/// 初始化：读配置、装日志、装配 Engine、建分派器。
pub(crate) fn init() -> Result<Host, InitError> {
    load_env();
    let template = write_config_template();
    let config = load_config();
    init_logging(&config);
    match template {
        Some(Ok(true)) => tracing::info!("已写出配置模板"),
        Some(Err(error)) => tracing::warn!(%error, "写配置模板失败"),
        _ => {}
    }
    let engine = assemble_engine(&config)?;
    let dispatch = Dispatch::new(&config);
    let mut host = Host { engine, dispatch };
    host.engine.log_session(env!("CARGO_PKG_VERSION"), "linux");
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "青简 Linux 就绪");
    Ok(host)
}

/// 读密钥：工作目录 `.env`，再叠加用户目录 `.env`；不覆盖已有环境变量。
fn load_env() {
    let _ = dotenvy::dotenv();
    if let Some(env_file) = paths::user_dir().map(|dir| dir.join(".env")) {
        let _ = dotenvy::from_path(&env_file);
    }
}

/// 首次启动把带说明的配置模板写到 `~/.config/qingjian/config.toml`；已有文件返回 `Ok(false)`。
fn write_config_template() -> Option<Result<bool, ConfigError>> {
    let path = paths::config_file()?;
    if let Some(dir) = path.parent()
        && let Err(source) = std::fs::create_dir_all(dir)
    {
        return Some(Err(ConfigError::Write { path, source }));
    }
    Some(Config::write_template_if_missing(&path))
}

/// 文件不存在按默认值；解析失败记错误退回默认。
fn load_config() -> Config {
    match paths::config_file() {
        Some(path) => Config::load(&path).unwrap_or_else(|error| {
            tracing::error!(%error, path = %path.display(), "配置解析失败，用默认值");
            Config::default()
        }),
        None => Config::default(),
    }
}

/// 级别按 `[general] log_level`（`RUST_LOG` 可覆盖），同时写 stderr 与按天滚动的文件（留 7 天）。
fn init_logging(config: &Config) {
    use tracing_subscriber::fmt::writer::MakeWriterExt;
    let level = if config.general.log_level == LogLevel::Debug {
        "debug"
    } else {
        "info"
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    match paths::log_dir() {
        Some(dir) => {
            let appender = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("qingjian")
                .filename_suffix("log")
                .max_log_files(7)
                .build(&dir)
                .expect("构建滚动日志文件");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            std::mem::forget(guard);
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(writer.and(std::io::stderr))
                .init();
        }
        None => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
    }
}

/// 装配 Engine：词库、释义表、学习数据、英文词表、emoji、语言模型、附加词库。
fn assemble_engine(config: &Config) -> Result<Engine, InitError> {
    let started = Instant::now();
    let dict = paths::resource("data/generated/dict.qj")
        .or_else(|| paths::resource("assets/lexicon/dict.tsv"))
        .or_else(|| paths::resource("assets/sample/dict.tsv"))
        .ok_or(InitError::NoDataDir)?;
    let dictionary = Dictionary::from_path(&dict).map_err(InitError::from_dictionary)?;
    let learner = match paths::user_dir() {
        Some(dir) => load_learner(&dir),
        None => FrequencyLearner::default(),
    };
    tracing::info!(
        entries = dictionary.len(),
        learned = learner.len(),
        dictionary_ms = started.elapsed().as_millis(),
        "词库与学习数据已加载"
    );
    let learning_language = config
        .general
        .learning_language
        .parse::<Language>()
        .unwrap_or(Language::English);
    let mut engine = Engine::new(dictionary).with_learner(Box::new(learner));
    if let Some(glossary) = glossary_file(learning_language) {
        match load_glossary(learning_language, &glossary, paths::user_dir().as_deref()) {
            Ok(translator) => engine = engine.with_translator(Box::new(translator)),
            Err(error) => tracing::warn!(%error, "释义表加载失败"),
        }
    }
    if let Some(dir) = paths::user_dir() {
        engine = engine
            .with_usage_meter(Box::new(UsageStats::open(dir.join("usage.tsv"))))
            .with_vocabulary_tracker(Box::new(load_vocabulary(&dir)));
        if config.general.input_log {
            let path = dir.join("input-log.jsonl");
            tracing::info!(path = %path.display(), "输入日志开着");
            engine = engine.with_input_logger(Box::new(InputLog::open(path)));
        }
    }
    engine.set_extra_dictionaries(extra_dictionaries::load(
        paths::bundled_dicts_dir().as_deref(),
        paths::user_dicts_dir().as_deref(),
        &config.dictionaries,
    ));
    if let Some(path) = paths::resource("data/generated/glossary-zh.qj")
        .or_else(|| paths::resource("assets/glossary/glossary-zh.tsv"))
    {
        match Glossary::from_path(Language::Chinese, &path) {
            Ok(glossary) => {
                tracing::info!(glosses = glossary.len(), "英→中释义表已加载");
                engine = engine.with_english_translator(Box::new(glossary));
            }
            Err(error) => tracing::warn!(%error, "英→中释义表加载失败"),
        }
    }
    if let Some(path) = paths::resource("data/generated/english.tsv")
        .or_else(|| paths::resource("assets/lexicon/english.tsv"))
        .or_else(|| paths::resource("assets/sample/english.tsv"))
    {
        match WordList::from_path(&path) {
            Ok(words) => {
                tracing::info!(words = words.len(), "英文词表已加载");
                engine = engine.with_english(words);
            }
            Err(error) => tracing::warn!(%error, "英文词表加载失败"),
        }
    }
    if let Some(table) = load_emoji() {
        tracing::info!(words = table.len(), "emoji 表已加载");
        engine = engine.with_emoji(table);
    }
    if let Some(model) = load_language_model() {
        tracing::info!(
            words = model.word_count(),
            bigrams = model.bigram_count(),
            load_ms = started.elapsed().as_millis(),
            "语言模型已加载"
        );
        engine = engine.with_language_model(Box::new(model));
    }
    engine.set_fuzzy(config.fuzzy);
    engine.set_shuangpin(config.general.shuangpin());
    engine.set_zhuyin_mode(config.general.zhuyin);
    engine.set_mode_keys(config.shortcut.mode);
    Ok(engine)
}

/// 某语言的释义表：打包过的优先，否则随 git 的 TSV。
fn glossary_file(language: Language) -> Option<std::path::PathBuf> {
    let code = language.code();
    paths::resource(&format!("data/generated/glossary-{code}.qj"))
        .or_else(|| paths::resource(&format!("assets/glossary/glossary-{code}.tsv")))
}

/// 随包释义表叠上个人释义表。
fn load_glossary(
    language: Language,
    path: &Path,
    user_dir: Option<&Path>,
) -> Result<LayeredTranslator, InitError> {
    let bundled =
        Glossary::from_path(language, path).map_err(|e| InitError::Glossary(e.to_string()))?;
    let personal = match user_dir {
        Some(dir) => PersonalGlossary::open(
            language,
            dir.join(format!("user-glossary-{}.tsv", language.code())),
        ),
        None => PersonalGlossary::in_memory(language),
    };
    Ok(LayeredTranslator::new(bundled, personal))
}

/// 读不了就退回只在内存里学，不拿空表覆盖用户文件。
fn load_learner(dir: &Path) -> FrequencyLearner {
    let path = dir.join("user.tsv");
    match FrequencyLearner::from_path(&path) {
        Ok(learner) => learner,
        Err(error) => {
            tracing::error!(path = %path.display(), %error, "学习数据读取失败，本次只在内存里学习");
            FrequencyLearner::default()
        }
    }
}

/// 词汇记录（`user-vocab.tsv`），有等级表就按级统计。
fn load_vocabulary(user_dir: &Path) -> VocabularyBook {
    let mut vocabulary = VocabularyBook::open(user_dir.join("user-vocab.tsv"));
    for language in GLOSSARY_LANGUAGES {
        let Some(path) = paths::resource(&format!("assets/levels/levels-{}.tsv", language.code()))
        else {
            continue;
        };
        match LevelTable::from_path(&path) {
            Ok(table) => vocabulary = vocabulary.with_levels(language, table),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "词汇等级表读不了，不分级")
            }
        }
    }
    vocabulary
}

/// 几张 emoji 表合成一张；坏的跳过。
fn load_emoji() -> Option<EmojiTable> {
    let mut merged: Option<EmojiTable> = None;
    for name in ["emoji-zh.tsv", "emoji-en.tsv"] {
        let Some(path) = paths::resource(&format!("assets/emoji/{name}")) else {
            continue;
        };
        match EmojiTable::from_path(&path) {
            Ok(table) => match &mut merged {
                Some(all) => all.merge(table),
                None => merged = Some(table),
            },
            Err(error) => tracing::warn!(%error, path = %path.display(), "emoji 表加载失败，跳过"),
        }
    }
    merged
}

/// 语言模型：打包过的 `lm.qj` 优先，否则 TSV 两件套；都没有为 `None`。
fn load_language_model() -> Option<BigramModel> {
    if let Some(packed) = paths::resource("data/generated/lm.qj") {
        match BigramModel::from_path(&packed) {
            Ok(model) => return Some(model),
            Err(error) => tracing::warn!(%error, "lm.qj 加载失败，尝试 TSV"),
        }
    }
    let unigram = paths::resource("data/generated/lm-unigram.tsv")?;
    let bigram = paths::resource("data/generated/lm-bigram.tsv")?;
    match BigramModel::from_paths(&unigram, &bigram) {
        Ok(model) => Some(model),
        Err(error) => {
            tracing::warn!(%error, "语言模型 TSV 加载失败，退化为一元词频");
            None
        }
    }
}
