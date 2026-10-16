//! 「快捷键」页：翻页键、删候选的组合键。

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSPopUpButton;
use qingjian_platform::{Config, PAGE_KEY_OPTIONS};

use crate::preferences::controls::{
    GROUP_GAP, button, note, note_full, page_keys_label, row_popup, row_recorder, select,
};
use crate::preferences::key_recorder::KeyRecorder;
use crate::preferences::layout::{Layout, PAGE_PADDING, ROW_HEIGHT};
use crate::preferences::setting::Setting;
use crate::preferences::target::PreferencesTarget;

pub struct ShortcutsPage {
    /// 翻页键对。
    page_keys: Retained<NSPopUpButton>,

    /// 删除候选的修饰键。
    delete_candidate: Retained<KeyRecorder>,
}

impl ShortcutsPage {
    pub fn build(layout: &mut Layout, mtm: MainThreadMarker, target: &PreferencesTarget) -> Self {
        let page_key_titles: Vec<String> = PAGE_KEY_OPTIONS
            .iter()
            .map(|k| page_keys_label(k))
            .collect();
        let page_keys = row_popup(
            layout,
            mtm,
            "翻页键",
            &page_key_titles,
            Setting::PageKeys,
            target,
        );
        note(
            layout,
            mtm,
            "选「，  。」时组句中敲逗号句号是翻页，不再是上屏加标点。",
        );
        layout.space(GROUP_GAP);
        let delete_candidate = row_recorder(
            layout,
            mtm,
            "删除候选",
            Setting::DeleteCandidateKeys,
            true,
            target,
        );
        note(
            layout,
            mtm,
            "按住修饰键再按候选序号：自己造的词整个删掉；词库里的词清掉对它的学习记录，回到原来的排序。组句中要打感叹号先把词上屏。",
        );
        layout.space(GROUP_GAP);
        note_full(
            layout,
            mtm,
            "改快捷键：点一下右边的按钮，再按下新的组合键（要带修饰键 ⌃ ⌥ ⇧ ⌘），Esc 取消。避开 ⌃+数字（切换桌面）和 ⌘+数字 / ⌘T（应用常用键）。",
        );
        let reset = button(mtm, "恢复默认快捷键", Setting::ResetShortcuts, target);
        layout.place(&reset, PAGE_PADDING, 160.0, ROW_HEIGHT + 4.0);
        layout.next_row(ROW_HEIGHT + 4.0);
        layout.space(GROUP_GAP);
        note_full(
            layout,
            mtm,
            "组句中固定的键（不可改）：空格上屏首选，1–9 选词，回车原样上屏，Esc 清空；⌥⌫ 删一个音节，⌘⌫ 删到开头；\
             ⌥← / ⌥→ 按音节跳光标，⌘← / ⌘→ 到开头 / 末尾；上 / 下移动高亮，PageUp / PageDown 与 ⇧Tab 翻页；\
             Tab 翻页（没有云端整句补全了）；半角标点进入英文直输段。",
        );
        Self {
            page_keys,
            delete_candidate,
        }
    }

    pub fn sync(&self, config: &Config) {
        let (previous, next) = config.general.page_keys();
        let pair = format!("{previous}{next}");
        select(
            &self.page_keys,
            PAGE_KEY_OPTIONS.iter().position(|k| *k == pair),
        );
        let delete = config.shortcut.delete_keys();
        self.delete_candidate.show(&delete.key(), &delete.label());
    }
}
