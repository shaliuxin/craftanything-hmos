//! 系统语言：把鸿蒙的 locale 写回环境变量，上游 `sys-locale` 就能读到。
//!
//! 字体注入留作后续工作（首版骨架版不做，界面用 egui 内置字体显示英文与 CJK 基本字符）。
//! 完整 CraftApp 的字体注入（`CRAFT_FONTS_DIR` / `CRAFTANYTHING_FONT_DIRS`）是后续工作的一部分。

/// 系统 locale 的 BCP-47 形式（`zh_CN` → `zh-CN`）。`locale` 缺失时退回 `language`。
fn system_tag(locale: &str, language: &str) -> Option<String> {
    let tag = [locale, language].into_iter().map(str::trim).find(|tag| !tag.is_empty())?;
    Some(tag.chars().map(|c| if c == '_' { '-' } else { c }).collect())
}

/// 在 `ca_init` 里、任何 UI 与字体读取之前调用一次。
pub fn install(_files_dir: &str, locale: &str, language: &str) {
    let tag = system_tag(locale, language);
    if let Some(tag) = &tag {
        // sys-locale 按 LANGUAGE → LC_ALL → LC_MESSAGES → LANG 取值：先清掉可能残留的高优先级变量。
        for key in ["LANGUAGE", "LC_ALL", "LC_MESSAGES"] {
            std::env::remove_var(key);
        }
        std::env::set_var("LANG", tag);
    }
    log::info!("locale: tag={tag:?} language={language:?}");
}
