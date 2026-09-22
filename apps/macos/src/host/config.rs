//! 配置热加载：config.toml 改了就整份重新套用到 Engine 与窗口；激活期间的定时事务。

use super::init::load_glossary;
use super::*;

impl Host {
    /// 把当前配置推给 Engine 与界面：模糊音 / 模式键 / 翻页 / 外观直接设；学习语言变了换释义表；
    /// 最后刷新菜单勾选与设置窗口。
    pub fn apply_config(&mut self, force: bool) {
        let config = self.settings.config().clone();
        self.engine.set_fuzzy(config.fuzzy);
        self.engine
            .set_full_width_punctuation(config.general.full_width_punctuation);
        if let Err(error) = self
            .engine
            .set_custom_phrases(config.custom_phrases.clone())
        {
            tracing::warn!(%error, "自定义短语配置未应用");
        }
        self.engine.set_mode_keys(config.shortcut.mode);
        self.engine.set_chinese_first(config.general.chinese_first);
        self.engine.set_shuangpin(config.general.shuangpin());
        logging::set_level(config.general.log_level);
        self.translation_keys = config.shortcut.translation_keys();
        self.delete_keys = config.shortcut.delete_keys();
        self.page_size = config.general.page_size();
        self.page_keys = config.general.page_keys();
        self.preedit_mode = config.general.preedit;
        self.window.set_theme(config.general.theme);
        self.window.set_layout(config.general.layout);
        self.apply_learning_language(&config.general.learning_language);
        if self.input_log_enabled != Some(config.general.input_log) {
            self.input_log_enabled = Some(config.general.input_log);
            self.open_input_log(config.general.input_log);
        }
        if force || config.dictionaries != self.applied_dictionaries {
            self.reload_dictionaries();
        }
        if self.applied_model.as_ref() != Some(&config.model) {
            if config.model.enabled {
                self.load_local_model();
            } else {
                self.unload_local_model();
            }
            self.applied_model = Some(config.model.clone());
        }
        self.indicator.update();
        self.menu.sync(&config, self.settings.error());
        self.dictionary_list = self.dictionary_infos();
        self.preferences
            .sync(&config, self.settings.error(), &self.dictionary_list);
    }

    /// 学习语言变了就换释义表；文件缺失或坏了保持原样，只记日志。
    pub(super) fn apply_learning_language(&mut self, code: &str) {
        let Ok(language) = code.parse::<Language>() else {
            tracing::warn!(code, "不认识的学习语言，保持不变");
            return;
        };
        if language == self.learning_language {
            return;
        }
        match load_glossary(language) {
            Ok(glossary) => {
                tracing::info!(
                    language = language.code(),
                    glosses = glossary.len(),
                    "释义表已切换"
                );
                self.engine.set_translator(Box::new(glossary));
                self.learning_language = language;
            }
            Err(error) => tracing::warn!(%error, "释义表加载失败，学习语言不变"),
        }
    }

    /// 配置文件被手改过就热加载；激活输入法时和监视定时器都会调。
    pub fn reload_config_if_changed(&mut self) {
        if self.settings.reload_if_changed() {
            self.apply_config(false);
        }
    }

    /// 激活期间的定时器每秒调一次：看配置文件，再看学习数据要不要落盘。
    /// 学习数据原本只在停用输入法时保存，进程被 launchd 杀掉就丢一整段；现在最多丢 [`LEARNING_FLUSH_INTERVAL`] 这么久。
    /// 没有新数据时 flush 是空操作（各表按 dirty 位判断），不会每分钟碰一次磁盘。
    pub fn tick(&mut self) {
        self.reload_config_if_changed();
        if self.last_flush.elapsed() >= LEARNING_FLUSH_INTERVAL {
            self.engine.flush_learning();
            self.last_flush = std::time::Instant::now();
        }
    }
}
