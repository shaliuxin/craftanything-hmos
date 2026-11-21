# Craft Anything · HarmonyOS 移植

> 上游：[storytold/craft-anything](https://github.com/storytold/craft-anything) **v0.6.1**
> 目标平台：**HarmonyOS (OHOS)**，双 ABI（`arm64-v8a` 真机 + `x86_64` 手机/PC 模拟器）
> 完整移植报告见 [移植报告.md](./移植报告.md)

**Craft Anything** 是一个纯 Rust（约 25.3 万行、13 个 crate）的「软件逆向与再创作」引擎：把任意软件 / 文件丢进去，用 320+ 工具剖析它「怎么工作」，再用 AI agent 重搭一个能跑的副本。本仓库把它移植到 HarmonyOS 上运行，**完整保留上游 UI 与功能**（详见下方「功能状态」）。

## 功能状态（真机实测）

| 能力 | 状态 |
|---|---|
| 完整 CraftApp UI（首次启动向导 / 320 工具界面 / 项目管理 / 设置 / AI 对话界面） | ✅ 已渲染（通过 eframe stub 兼容层，零 winit 依赖） |
| 文件分析（格式 / 架构 / 段 / 导入 / 导出 / 依赖库 / 字符串 / 熵） | ✅ `craft-anything-formats`（60+ 格式） |
| 反汇编内核（x86 / ARM） | ✅ `craft-anything-disasm` |
| JS 分析（oxc AST） | ✅ `craft-anything-js` |
| 综合分析器 | ✅ `craft-anything-analysis` |
| AI 对话界面 | ✅ UI 已渲染（实际调用需外部后端 / API key） |
| 中文渲染 | ✅ Noto Sans SC 内嵌，全字重正常（含粗体 / 标题） |
| 文件选择器（OHOS DocumentViewPicker） | ✅ ArkTS 侧通路就绪 |
| AI agent 实际执行（Claude Code / Ghidra 等） | ✅ 外部进程桥已接通（spawn 转给 PC 执行，见「AI agent 外部执行桥」） |
| 自动更新 | 已门控关闭（鸿蒙无自更新机制） |

**技术突破**：用 3 个最小 stub crate（`eframe` / `rfd` / `arboard`）替代上游对桌面窗口系统、原生文件对话框、跨进程剪贴板的依赖，使完整的 `craft-anything-ui-egui`（约 25 万行 Rust，含 40+ UI 模块、i18n、320 工具界面）在纯 `wgpu + egui` 管线上编译运行。

## 架构

绕开 eframe / winit（OHOS 没有），由 Rust 直接持有原生窗口：

```
ArkTS 外壳（UIAbility + XComponent 原生表面）
   │  N-API（libcraftanything.so，C++ 编）
C++ N-API 桥（bridge.cpp：Surface/Touch/Key/Axis 回调 + 文件 picker + IME）
   │  FFI（extern "C" ca_*）
Rust staticlib（libcraftanything_hmos.a）
   │  wgpu(GLES) 后端
egui 0.37（vendored 版）
   → OHOS NativeWindow 直绘，不经 eframe
```

## 目录结构

```
craftanything-hmos/
├─ AppScope/                    bundleName / versionName / 应用图标
├─ entry/
│  ├─ src/main/ets/             EntryAbility.ets / pages/Index.ets / SystemTheme.ets
│  ├─ src/main/cpp/             bridge.cpp（ca_* NAPI）/ CMakeLists.txt
│  └─ build-profile.json5       abiFilters: arm64-v8a, x86_64
├─ native/rust/                 适配层 staticlib（lib.rs / bridge / input / render / …）
├─ vendor/                      eframe·rfd·arboard stub + vendored egui 0.37 + platform-ohos（进程桥）
├─ tools/exec-bridge/           PC 端外部进程执行服务（ca-exec-bridge）
├─ third_party/wgpu-hal-30.0.1  GLES 修复版 wgpu-hal
├─ scripts/                     见下「构建」
├─ build-profile.json5          签名配置【占位符】，真实值见 signing/
└─ 移植报告.md                   完整移植报告
```

## 构建前置条件

1. **DevEco Studio**（含 OHOS native SDK / `clang.exe` / `llvm-ar.exe` / `hdc`）。
2. **Rust 工具链**：安装 `aarch64-unknown-linux-ohos` 与 `x86_64-unknown-linux-ohos` 两个 target。
3. **Node.js + Java**：`hvigorw`（HAP 打包）需要。
4. **上游源码同级目录 `_upstream`（关键）**：本仓库 `native/rust/Cargo.toml` 用相对路径
   `../../../_upstream/crates/*` 引用上游 8 个 crate，因此构建时本仓库的**兄弟目录**里必须有
   上游 craft-anything 源码（v0.6.1）。目录应长这样：

   ```
   工作区/
   ├─ craftanything-hmos/   # 本仓库
   └─ _upstream/            # github.com/storytold/craft-anything @ v0.6.1
   ```

## 构建步骤

> 说明：`native/rust/.cargo/config.toml` 已配置 OHOS 交叉链接（`clang.exe` + `--target` + `--sysroot`）。
> **务必从 `native/rust/` 目录执行 cargo**（cargo 从当前 CWD 而非 manifest 路径发现 `.cargo/config.toml`）。

```bash
# 1) 构建 Rust 适配层 staticlib（默认双 ABI：aarch64 + x86_64）
#    只跑真机可设 CRAFTANYTHING_NATIVE_TARGETS=aarch64-unknown-linux-ohos
bash scripts/build_native.sh

# 2) 打包 HAP（需先完成签名，见下）
#    用 DevEco「自动签名」生成的是调试 profile，故用 buildMode=debug
node <DevEco>/tools/hvigor/bin/hvigorw.js assembleHap --mode module \
  -p product=default -p buildMode=debug --no-daemon

# 产物
#   entry/build/default/outputs/default/entry-default-signed.hap

# 3)（可选）校验 HAP 的 ABI / 无 web 残留
python scripts/verify_native.py entry/build/default/outputs/default/entry-default-signed.hap
```

`scripts/` 一览：

| 脚本 | 用途 |
|---|---|
| `build_native.sh` | 交叉编译 Rust staticlib（按 `CRAFTANYTHING_NATIVE_TARGETS` 驱动；自动调用 `setup_exec_bridge.sh`） |
| `setup_exec_bridge.sh` | 把上游 workspace 的 platform 依赖重定向到 `vendor/platform-ohos`（可重复执行） |
| `run_on_device.sh` | 本机开发循环：构建 → 打包 → 装到已连设备 → 启动 |
| `verify_native.py` | 校验 HAP 的 ABI、确认无 web 残留 |
| `make_craftanything_signing.sh` | 生成 `signing/` 下本地 CA 链（仅验证打包链路） |
| `make_icon.py` / `encrypt_pwd.js` | 图标生成 / DevEco 口令 hex 工具 |

## 签名与安装

- **签名配置**：仓库里的 `build-profile.json5` 是**占位符**（不含真实证书 / 口令）。
  真实配置放在被 `.gitignore` 屏蔽的 `signing/build-profile.local.json5`；`run_on_device.sh`
  打包前会自动把它还原回 `build-profile.json5`，本地构建无需手动操作。
- **生成签名（二选一）**：
  - A) DevEco Studio → File → Project Structure → Signing Configs → 勾选**自动签名**，
    用 bundleName `cn.shaliuxin.craftanything` 生成绑定本机设备 UDID 的**调试** Profile
    （回写后请本地备份，勿提交）。
  - B) 纯命令行：`bash scripts/make_craftanything_signing.sh`（本地 CA 链，仅验证打包链路，
    真机安装会报 `9568257`）。
- **安装 / 启动**（设备已连、USB 调试开启）：

  ```bash
  hdc -t <device> install -r -d entry/build/default/outputs/default/entry-default-signed.hap
  hdc -t <device> shell aa start -b cn.shaliuxin.craftanything -a EntryAbility
  ```

> 打包注意：DevEco「自动签名」生成的是**调试 profile**，只能配 `buildMode=debug`；
> 用 `--buildMode=release` 会在 `SignHap` 报 `Error 00308018`（release 需正式 release profile）。

## 包名 / 版本铁律

- **bundleName**：`cn.shaliuxin.craftanything`（本项目专属，不借用其它 craft 工程的 profile）
- **versionName**：`0.6.1`（跟随上游）；**versionCode**：`601`
- **应用名**：`Craft Anything`（上游产品名，不加后缀）

## AI agent 外部执行桥

鸿蒙应用沙箱没有「按路径 exec 任意程序」的公开 API（官方 `childProcessManager` 只能启动
应用自身声明的入口），所以 agent 的实际执行代理到 PC：

```
UI → AiHub → platform::Process::spawn（OHOS 分支）
   → TCP 127.0.0.1:17890（hdc rport 反向转发）
   → ca-exec-bridge（PC）→ Command::new("claude" …)
   → started / stdout / stderr / exit 回传 UI
```

使用前需在 PC 上准备：

```bash
# 1) 编译并启动执行桥（默认只监听 127.0.0.1:17890）
cd tools/exec-bridge && cargo build --release && ./target/release/ca-exec-bridge.exe

# 2) PC 的 PATH 中需有真实 agent CLI（claude / codex / …）
#    以及上游 helper：cargo build --release -p craft-anything-cli（在 _upstream/ 下）

# 3) 设备上建立反向端口转发
hdc -t <device> rport tcp:17890 tcp:17890
```

然后在应用「设置 → AI → 代理」里选择 agent 并**关闭 ACP**（ACP 需要双向 stdin，
当前桥只支持一次性 stdin 的普通 CLI 路径）。

已验证：`started → stdout → exit` 协议本地跑通；真机 UI 能检测并添加 Claude Code（显示「已就绪」）；
触发构建后的错误信息确实由 PC bridge 回传。完整真实 agent 对话需在准备好上述两个 PC 程序后复验。

## 已知限制

- **ACP 与终端捕获未代理**：ACP 需要可写的 `ChildStdin`（OHOS 侧无法虚拟）；`portable-pty`
  终端捕获工具链也未接桥，相关工具不可用。
- **自动更新**：鸿蒙无对应自更新机制，已门控关闭。
- **首帧略慢**：CJK 字形图集首次构建稍慢，启动遮罩会多停留片刻（正常，非缺陷）。

## License

移植适配层遵循上游 [LICENSE](./LICENSE)（MIT / Apache-2.0 双许可，见 `LICENSE-MIT` / `LICENSE-APACHE-2.0`）。
