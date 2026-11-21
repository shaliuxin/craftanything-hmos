//! 鸿蒙专属 FFI 边界。首版目标：验证「ArkTS + C++ N-API + Rust + wgpu(gles) + egui」整条通路
//! 在 OHOS 上可跑通。完整 CraftApp（含 Engine / AiHub / 320 工具 / AI agent）是后续工作，原因见移植报告。
#![allow(clippy::needless_lifetimes)]
mod appearance;
mod bridge;
mod display_color;
mod form_factor;
mod input;
mod locale;
mod render;

use serde::Deserialize;
use std::{
    sync::{
        OnceLock,
        mpsc,
        atomic::{AtomicI8, Ordering},
    },
    time::{Duration, Instant},
};

/// 把 `log` crate 的记录转到 hilog：wgpu-hal 的 EGL 诊断（平台选择 / surface 创建 / swap 失败）
/// 默认没有 logger 就会被丢掉，设备上完全看不见。
#[cfg(target_env = "ohos")]
mod glog {
    use std::ffi::{CString, c_char};

    unsafe extern "C" {
        fn ca_hilog(level: i32, message: *const c_char);
    }

    struct Hilog;

    impl log::Log for Hilog {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }

        fn log(&self, record: &log::Record<'_>) {
            let level = match record.level() {
                log::Level::Error => 2,
                log::Level::Warn => 1,
                _ => 0,
            };
            let line = format!("[{}] {}: {}", record.level(), record.target(), record.args());
            if let Ok(text) = CString::new(line) {
                unsafe { ca_hilog(level, text.as_ptr()) };
            }
        }

        fn flush(&self) {}
    }

    static LOGGER: Hilog = Hilog;

    pub fn install() {
        if log::set_logger(&LOGGER).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Config {
    files: String,
    #[serde(default)]
    documents: String,
    #[serde(default = "default_scale")]
    scale: f32,
    #[serde(default)]
    language: String,
    #[serde(default, rename = "systemLocale")]
    system_locale: String,
    #[serde(default, rename = "displayP3")]
    display_p3: bool,
    #[serde(default, rename = "systemTheme")]
    system_theme: String,
    #[serde(default)]
    fullscreen: bool,
    #[serde(default, rename = "deviceType")]
    device_type: String,
    #[serde(default, rename = "widthVp")]
    width_vp: f32,
}
fn default_scale() -> f32 {
    1.
}

#[allow(dead_code)]
enum Message {
    Attach(usize, u32, u32),
    Resize(u32, u32),
    Detach,
    Frame,
    Active(bool),
    Input(input::Input),
    Close,
    Wake(Duration),
}

static SEND: OnceLock<mpsc::Sender<Message>> = OnceLock::new();
fn send(message: Message) {
    if let Some(tx) = SEND.get() {
        let _ = tx.send(message);
    }
}

static SYSTEM_THEME_TOKEN: AtomicI8 = AtomicI8::new(0);

unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len > isize::MAX as usize || (ptr.is_null() && len != 0) {
        return None;
    }
    if len == 0 {
        return Some(&[]);
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// # Safety
/// `ptr`/`len` 必须在调用期间指向可读内存；notify 回调在进程生命周期内保持有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ca_init(ptr: *const u8, len: usize, notify: bridge::Notify) -> bool {
    let Some(data) = (unsafe { bytes(ptr, len) }) else {
        return false;
    };
    let Ok(config) = serde_json::from_slice::<Config>(data) else {
        return false;
    };
    if !config.scale.is_finite() || config.scale <= 0. || config.scale > 8. {
        return false;
    }
    bridge::init(notify);
    #[cfg(target_env = "ohos")]
    glog::install();
    locale::install(&config.files, &config.system_locale, &config.language);
    if !config.system_theme.is_empty() {
        let dark = config.system_theme.eq_ignore_ascii_case("dark");
        SYSTEM_THEME_TOKEN.store(if dark { 1 } else { -1 }, Ordering::Release);
        appearance::set_dark(dark);
    }
    let (tx, rx) = mpsc::channel();
    if SEND.set(tx).is_err() {
        return true;
    }
    std::thread::Builder::new()
        .name("craftanything-native".into())
        .spawn(move || {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker(config, rx)))
                .is_err()
            {
                bridge::notify(0, "error", "Native host initialization failed", &[]);
            }
        })
        .is_ok()
}

/// # Safety
/// 指针/长度对必须可读直到返回。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ca_input(ptr: *const u8, len: usize) {
    let Some(data) = (unsafe { bytes(ptr, len) }) else {
        return;
    };
    let Ok(value) = serde_json::from_slice(data) else {
        return;
    };
    send(Message::Input(value));
}

/// # Safety
/// `ptr`/`len` 必须可读直到返回。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ca_reply(id: u64, ok: bool, text: *const u8, text_len: usize, data: *const u8, len: usize) {
    let Some(text) = (unsafe { bytes(text, text_len) }) else {
        bridge::reply(id, Err("Invalid reply text buffer".into()));
        return;
    };
    let Some(data) = (unsafe { bytes(data, len) }) else {
        bridge::reply(id, Err("Invalid reply data buffer".into()));
        return;
    };
    let text = String::from_utf8_lossy(text).into_owned();
    bridge::reply(id, if ok { Ok((text, data.to_vec())) } else { Err(text) });
}

/// # Safety
/// 指针/长度对必须可读直到返回。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ca_open(_text: *const u8, _n: usize, _data: *const u8, _len: usize) {
    bridge::notify(0, "info", "open not wired in this port (see report)", &[]);
}

/// # Safety
/// 指针/长度对必须可读直到返回；`fd` 是 ArkTS 侧 dup 出来的文件描述符，骨架版未消费。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ca_drop_file(_name: *const u8, _n: usize, _fd: i32, _x: f32, _y: f32) {
    bridge::notify(0, "info", "drop not wired in this port (see report)", &[]);
}

/// # Safety
/// 指针/长度对必须可读直到返回。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ca_drop(_name: *const u8, _n: usize, _data: *const u8, _len: usize, _x: f32, _y: f32) {
    bridge::notify(0, "info", "drop not wired in this port (see report)", &[]);
}

/// # Safety
/// `ptr`/`len` 必须可读直到返回。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ca_command(ptr: *const u8, len: usize) {
    let Some(data) = (unsafe { bytes(ptr, len) }) else {
        return;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    let Some(id) = v.get("id").and_then(|v| v.as_str()) else {
        return;
    };
    bridge::notify(0, "command", &format!("not wired: {id}"), &[]);
}

/// # Safety
/// `window` 必须是存活的 OHNativeWindow，且已获取一个原生引用；该引用所有权转移给 worker 及其 wgpu surface。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ca_attach(window: usize, w: u32, h: u32) {
    if window != 0 {
        send(Message::Attach(window, w, h));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ca_resize(w: u32, h: u32) {
    send(Message::Resize(w, h));
}

#[unsafe(no_mangle)]
pub extern "C" fn ca_detach() {
    bridge::cancel();
    send(Message::Detach);
}

#[unsafe(no_mangle)]
pub extern "C" fn ca_frame() {
    send(Message::Frame);
}

#[unsafe(no_mangle)]
pub extern "C" fn ca_active(active: bool) {
    send(Message::Active(active));
}

#[unsafe(no_mangle)]
pub extern "C" fn ca_close() {
    send(Message::Close);
}

// ===================== 仅 OHOS 宿主逻辑 =====================

#[cfg(target_env = "ohos")]
unsafe extern "C" {
    fn ca_request_vsync();
}

/// 内嵌中文简体（Noto Sans SC），追加到 proportional / monospace 的 fallback 链，
/// 让 UI 里的中文不再显示为方块（egui 默认字体只有拉丁字形）。
#[cfg(target_env = "ohos")]
fn install_cjk_fonts(ctx: &egui::Context) {
    const CJK: &[u8] = include_bytes!("../assets/NotoSansSC-VF.ttf");
    const NAME: &str = "NotoSansSC";
    // 原版 ui-egui 主题字体（拉丁/代码），字节直接复用上游 pub static（与 theme::install_fonts 对齐）。
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("DMSans".to_owned(), std::sync::Arc::new(FontData::from_static(craft_anything_ui_egui::theme::DM_SANS_REGULAR)));
    fonts
        .font_data
        .insert("DMSans-Medium".to_owned(), std::sync::Arc::new(FontData::from_static(craft_anything_ui_egui::theme::DM_SANS_MEDIUM)));
    fonts
        .font_data
        .insert("DMSans-Bold".to_owned(), std::sync::Arc::new(FontData::from_static(craft_anything_ui_egui::theme::DM_SANS_BOLD)));
    fonts
        .font_data
        .insert("JetBrainsMono".to_owned(), std::sync::Arc::new(FontData::from_static(craft_anything_ui_egui::theme::JETBRAINS_MONO)));
    // 鸿蒙无 CRAFT_FONTS_DIR 提供的 CJK craft-fonts，用 NotoSansSC 兜底中/日/韩字形。
    fonts.font_data.insert(NAME.to_owned(), std::sync::Arc::new(FontData::from_static(CJK)));

    // 抓取 egui 默认链（含自带 emoji / 图标 / 拉丁兜底），在其前面插入主题字体 + NotoSansSC。
    let base_prop: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let base_mono: Vec<String> = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let push = |v: &mut Vec<String>, n: &str| {
        if !v.iter().any(|x| x == n) {
            v.push(n.to_string());
        }
    };

    // Proportional：DMSans(拉丁) → NotoSansSC(CJK) → 原默认(emoji/图标…)
    let mut proportional = Vec::new();
    push(&mut proportional, "DMSans");
    push(&mut proportional, NAME);
    for n in &base_prop {
        push(&mut proportional, n);
    }
    // Monospace：JetBrainsMono(代码) → NotoSansSC(CJK) → 原默认
    let mut monospace = Vec::new();
    push(&mut monospace, "JetBrainsMono");
    push(&mut monospace, NAME);
    for n in &base_mono {
        push(&mut monospace, n);
    }
    // 主题 heading/label 用的「命名 family」（theme::medium/semibold → FontFamily::Name）：
    // 对应 DMSans 字重 → CJK(NotoSansSC) → 完整 Proportional 链。
    // 这是修掉粗体/标题中文方块的「关键」——这些 family 之前完全没有 CJK 回退。
    let mut medium = vec!["DMSans-Medium".to_string()];
    for n in &proportional {
        push(&mut medium, n);
    }
    let mut semibold = vec!["DMSans-Bold".to_string()];
    for n in &proportional {
        push(&mut semibold, n);
    }

    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Monospace, monospace);
    fonts.families.insert(FontFamily::Name("medium".into()), medium);
    fonts.families.insert(FontFamily::Name("semibold".into()), semibold);
    ctx.set_fonts(fonts);
}

/// 生成一个最小可解析的 64-bit ELF 文件（含 .text/.data/.rodata 段 + 一个导入符号），
/// 用于「示例分析」按钮，无需外部文件即可演示分析引擎。
fn make_sample_elf() -> Vec<u8> {
    use std::io::Write;
    let mut b = Vec::with_capacity(512);
    // ELF magic
    b.extend_from_slice(b"\x7fELF");
    // 64-bit, little-endian, ELFv1
    b.push(2); // EI_CLASS = ELFCLASS64
    b.push(1); // EI_DATA  = ELFDATA2LSB
    b.push(1); // EI_VERSION
    b.push(0); // EI_OSABI = System V
    b.extend_from_slice(&[0u8; 8]); // padding
    // Type = ET_EXEC (2), Machine = EM_AARCH64 (183)
    b.extend_from_slice(&2u16.to_le_bytes()); // e_type
    b.extend_from_slice(&183u16.to_le_bytes()); // e_machine
    b.extend_from_slice(&1u32.to_le_bytes()); // e_version
    b.extend_from_slice(&0x1000u64.to_le_bytes()); // e_entry
    b.extend_from_slice(&64u64.to_le_bytes()); // e_phoff (after e_ident + header)
    b.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
    b.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    b.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    b.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
    b.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
    b.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
    b.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
    b.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx
    // One program header: LOAD segment
    b.extend_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
    b.extend_from_slice(&5u32.to_le_bytes()); // p_flags = R+X
    b.extend_from_slice(&0u64.to_le_bytes()); // p_offset
    b.extend_from_slice(&0x400000u64.to_le_bytes()); // p_vaddr
    b.extend_from_slice(&0x400000u64.to_le_bytes()); // p_paddr
    b.extend_from_slice(&0x1000u64.to_le_bytes()); // p_filesz
    b.extend_from_slice(&0x1000u64.to_le_bytes()); // p_memsz
    b.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align
    // Pad to 512 bytes
    while b.len() < 512 {
        b.push(0);
    }
    // Add some "import-like" data at offset 0x1000
    let mut imp = b"\x00\x00\x00\x02".to_vec(); // NUL + "main"
    let _ = imp.write_all(b"main\0printf\0__stack_chk_fail\0");
    b.extend_from_slice(&imp);
    b
}

/// 文件分析结果（用上游纯 Rust crate 计算）。
#[allow(dead_code)]
#[derive(Default)]
struct AnalysisResult {
    file_name: String,
    file_size: u64,
    format_name: String,
    arch_name: String,
    kind: String,
    entry_point: String,
    sections: Vec<(String, String, u64)>, // (name, rwx, size)
    libraries: Vec<String>,
    imports: Vec<String>,
    exports: Vec<String>,
    top_strings: Vec<(String, u64)>, // (text, offset)
    entropy: f64,
    error: Option<String>,
}

/// 对任意文件字节做快速分析（纯 Rust，无子进程、无 C 依赖）。
fn analyze_bytes(data: &[u8], name: &str) -> AnalysisResult {
    let mut result = AnalysisResult {
        file_name: name.to_string(),
        file_size: data.len() as u64,
        ..Default::default()
    };
    // 1) 格式 + 架构 + sections + imports + exports
    if let Ok(binary) = craft_anything_formats::binary::parse_binary(data) {
        result.format_name = format!("{:?}", binary.format);
        result.arch_name = binary.arch_name.clone();
        result.kind = binary.kind.to_string();
        result.entry_point = binary.entry.map(|e| format!("{e:#x}")).unwrap_or_else(|| "-".into());
        result.sections = binary
            .sections
            .iter()
            .take(32)
            .map(|s| {
                let rwx = format!(
                    "{}{}{}",
                    if s.read { 'r' } else { '-' },
                    if s.write { 'w' } else { '-' },
                    if s.exec { 'x' } else { '-' }
                );
                (s.name.clone(), rwx, s.size)
            })
            .collect();
        result.libraries = binary.libraries.clone();
        result.imports = binary.imports.iter().take(40).map(|i| i.name.clone()).collect();
        result.exports = binary.exports.iter().take(40).map(|e| e.name.clone()).collect();
    } else {
        // 不是已知二进制格式 — 可能是文本/压缩包/数据库等
        result.format_name = "未知/非标准二进制".into();
    }
    // 2) 高熵检测
    result.entropy = craft_anything_formats::entropy(data) as f64;
    // 3) 前 20 条字符串（长度 ≥ 6）
    result.top_strings = craft_anything_formats::strings::extract(data, 0, 6, 20)
        .into_iter()
        .take(20)
        .map(|s| (s.text, s.offset))
        .collect();
    result
}

#[cfg(target_env = "ohos")]
fn worker(config: Config, rx: mpsc::Receiver<Message>) {
    bridge::notify(0, "stage", "Rust worker entered", &[]);
    let ctx = egui::Context::default();
    let form = form_factor::Form::new(&config.device_type, config.width_vp);
    bridge::notify(
        0,
        "diagnostic",
        &format!(
            "form factor {:?}, {:.0} vp, {:?}",
            form.device_type, form.width_vp, form.width_class
        ),
        &[],
    );

    let mut render = match render::Render::new(config.display_p3) {
        Ok(render) => render,
        Err(error) => {
            bridge::notify(0, "error", &error, &[]);
            return;
        }
    };
    let mut active = true;
    let mut visible = false;
    let mut deadline: Option<Instant> = None;
    let started = Instant::now();
    let mut events: Vec<egui::Event> = Vec::new();
    let mut input_state = input::State::default();
    let mut close = false;
    let mut ime_shown = false;

    // 完整 CraftApp（上游 ui-egui crate，320 工具界面）
    let settings = craft_anything_engine::settings::Settings::default();
    let session = craft_anything_engine::Session::new(settings);
    let engine = std::sync::Arc::new(craft_anything_engine::Engine::new(session));
    let hub = std::sync::Arc::new(craft_anything_ai::AiHub::new(engine.clone()));
    let mut app = craft_anything_ui_egui::CraftApp::new(engine, hub);
    craft_anything_ui_egui::CraftApp::setup_context(&ctx);
    // CJK 字体必须在 setup_context 之后注入（setup 会重置 FontDefinitions）
    install_cjk_fonts(&ctx);
    let mut frame = eframe::Frame::new(ctx.clone());

    ctx.set_request_repaint_callback(|info| send(Message::Wake(info.delay)));
    bridge::notify(0, "ready", "Native host initialized (full CraftApp)", &[]);

    loop {
        let wait = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        let message = match wait {
            Some(wait) => match rx.recv_timeout(wait) {
                Ok(m) => m,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    deadline = None;
                    if active && visible && render.attached() {
                        unsafe { ca_request_vsync() };
                    }
                    continue;
                }
                Err(_) => break,
            },
            None => match rx.recv() {
                Ok(m) => m,
                Err(_) => break,
            },
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<(), String> {
                match message {
                    Message::Attach(ptr, w, h) => {
                        visible = w > 0 && h > 0;
                        ime_shown = false;
                        render.bind(render::Window(ptr), w, h)?;
                        bridge::resume();
                        deadline = Some(Instant::now());
                        bridge::notify(0, "ready", "Native egui / wgpu GLES", &[]);
                    }
                    Message::Resize(w, h) => {
                        visible = w > 0 && h > 0;
                        render.resize(w, h)?;
                        deadline = Some(Instant::now());
                    }
                    Message::Detach => {
                        visible = false;
                        deadline = None;
                        render.detach();
                        input_state.cancel(&mut events);
                        if ime_shown {
                            ime_shown = false;
                            bridge::notify(0, "ime", "hide", &[]);
                        }
                    }
                    Message::Active(value) => {
                        active = value;
                        if active {
                            bridge::resume();
                            deadline = Some(Instant::now());
                        } else {
                            deadline = None;
                            input_state.cancel(&mut events);
                        }
                    }
                    Message::Input(value) => {
                        if value.kind == "system_theme" {
                            let dark = value.text.eq_ignore_ascii_case("dark");
                            appearance::set_dark(dark);
                            deadline = Some(Instant::now());
                            return Ok(());
                        }
                        input_state.push(value, config.scale, &mut events);
                        deadline = Some(Instant::now());
                    }
                    Message::Close => {
                        bridge::resume();
                        close = true;
                        deadline = Some(Instant::now());
                    }
                    Message::Wake(delay) => {
                        if active && visible {
                            if let Some(next) = Instant::now().checked_add(delay) {
                                deadline = Some(deadline.map_or(next, |d| d.min(next)));
                            }
                        }
                    }
                    Message::Frame => {
                        if !active || !visible {
                            return Ok(());
                        }
                        if !render.attached() {
                            return Ok(());
                        }
                        let size = egui::vec2(
                            render.width as f32 / config.scale,
                            render.height as f32 / config.scale,
                        );
                        let mut raw = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            time: Some(started.elapsed().as_secs_f64()),
                            events: std::mem::take(&mut events),
                            system_theme: appearance::current(),
                            ..Default::default()
                        };
                        let viewport = raw.viewports.entry(egui::ViewportId::ROOT).or_default();
                        viewport.native_pixels_per_point = Some(config.scale);
                        viewport.inner_rect = raw.screen_rect;
                        viewport.focused = Some(true);
                        let validating_close = close;
                        if close {
                            viewport.events.push(egui::ViewportEvent::Close);
                            close = false;
                        }
                        let output = ctx.run_ui(raw, |ui| {
                            // 完整 CraftApp UI（上游 320 工具界面）
                            eframe::App::ui(&mut app, ui, &mut frame);
                        });

                        let ime_wanted = output.platform_output.ime.is_some();
                        if ime_wanted != ime_shown {
                            ime_shown = ime_wanted;
                            bridge::notify(0, "ime", if ime_wanted { "show" } else { "hide" }, &[]);
                        }
                        let mut commands = Vec::new();
                        let mut repaint_delay: Option<Duration> = None;
                        if let Some(v) = output.viewport_output.get(&egui::ViewportId::ROOT) {
                            commands.extend(v.commands.clone());
                            repaint_delay = Some(v.repaint_delay);
                        }
                        match close_action(validating_close, &commands) {
                            CloseAction::Confirm => bridge::notify(0, "close", "", &[]),
                            CloseAction::Request => {
                                close = true;
                                deadline = Some(Instant::now());
                            }
                            CloseAction::Cancel | CloseAction::None => {}
                        }
                        let painted = render.paint(&ctx, output)?;
                        if first_frame_marker(painted) {
                            bridge::notify(0, "first_frame", "Native UI presented", &[]);
                        }
                        if painted && active && visible && render.attached() {
                            deadline = match repaint_delay {
                                Some(delay) if delay.is_zero() => Some(Instant::now()),
                                Some(delay) if delay < Duration::from_secs(3600) => {
                                    Some(Instant::now() + delay)
                                }
                                _ => None,
                            };
                        }
                    }
                }
                Ok(())
            },
        ));
        let _ = result;
    }
}

#[cfg(target_env = "ohos")]
#[allow(dead_code)]
fn first_frame_marker(painted: bool) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static FIRST: AtomicBool = AtomicBool::new(false);
    if painted && !FIRST.swap(true, Ordering::AcqRel) {
        true
    } else {
        false
    }
}

#[cfg(target_env = "ohos")]
#[derive(Debug, PartialEq, Eq)]
enum CloseAction {
    None,
    Request,
    Cancel,
    Confirm,
}

#[cfg(target_env = "ohos")]
fn close_action(validating: bool, commands: &[egui::ViewportCommand]) -> CloseAction {
    if commands.contains(&egui::ViewportCommand::CancelClose) {
        CloseAction::Cancel
    } else if validating {
        CloseAction::Confirm
    } else if commands.contains(&egui::ViewportCommand::Close) {
        CloseAction::Request
    } else {
        CloseAction::None
    }
}

#[cfg(not(target_env = "ohos"))]
fn worker(_config: Config, _rx: mpsc::Receiver<Message>) {
    bridge::notify(0, "error", "Native host requires an OHOS target", &[]);
}
