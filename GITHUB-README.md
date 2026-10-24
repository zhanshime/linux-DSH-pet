# dsh-pet Desktop 🐾

> 把 [dsh-pet](https://github.com/PC2005-cloud/dsh-pet) 桌宠装进 Tauri 外壳：Fedora / GNOME **Wayland 原生透明覆盖层**，可点击穿透、可拖拽、带右键菜单，并打包成 Flatpak。

<p align="center">
  <img src="tauri/flatpak/com.pc2005.dshpet.desktop.png" width="140" alt="dsh-pet icon">
</p>

<p align="center">
  <img alt="license" src="https://img.shields.io/badge/license-MIT-orange">
  <img alt="platform" src="https://img.shields.io/badge/platform-Linux%20%2F%20Wayland-8A2BE2">
  <img alt="tauri" src="https://img.shields.io/badge/built%20with-Tauri%202-24C8DB">
  <img alt="flatpak" src="https://img.shields.io/badge/package-Flatpak-4A90D9">
</p>

---

## 这是什么

`dsh-pet` 原本是 DeepSeek Harness 的网页插件：桌宠跑在浏览器里，靠 Electron 宿主提供“透明窗口 + 点击穿透 + 跟随移动”。

**dsh-pet Desktop** 把同一套渲染端（`sprite.js` / 右键菜单 / 气泡 / 对话）搬进一个独立的 Tauri v2 进程：

- 不需要 Electron，也不要求本机跑着 DSH；
- 在 **GNOME Wayland 会话**里以“工作区大小的透明覆盖层”运行，点击穿透、拖拽、菜单全部保留；
- 用一个极简本机 HTTP 宿主替代原来的 Node 宿主半边（配置合并、动画/表情包/字体服务、状态推送）；
- 带 Flatpak 打包脚本，`flatpak install` 后从应用菜单直接启动。

一句话：**给桌宠做一个能独立运行的 Linux 桌面壳。**

## 特性

- **Wayland 原生**：透明窗口 + GPU 合成，不依赖 XWayland 渲染；纯 Wayland 几何（不使用 `_NET_WORKAREA`）。
- **点击穿透**：宠物以外全部透传（`wl_surface.set_input_region`，C shim 直连 Wayland；GTK3 没实现这个 API）。可点区域取渲染端的 `.pet-hit` 命中盒（约视频框的 30%），透明边角也能穿透。
- **自动漫游**：待机动画结束掷骰决定「走一段 / 转身 / 小动作」，权重可调；`fixedEnabled: true` 可让它老实待在角落。
- **完整交互**：拖拽、右键菜单、对话气泡、余额/工作状态联动、碎碎念 + 表情包配图。
- **分辨率/缩放自适应**：改分辨率、换缩放、插拔屏后自动重排（窗口跟随工作区、渲染端热更新几何）。
- **Flatpak**：`org.gnome.Platform//50` 运行时，无 X11 权限；构建脚本一键打包并把 `.flatpak` 放到桌面。
- **零模型依赖的碎碎念**：内置 12 句本地文案 + 27 张表情包随机配图，周期可配；右键菜单可手动触发。
- **自检**：`--selftest` 用 canvas 采样验证 VP9 alpha 解码/合成，排查黑底/透明问题。
- **xdg_toplevel_tag_v1**：给窗口打 `dsh-pet` 这个稳定 tag（+ 中文描述），供合成器/窗口规则在应用重启后识别同一个窗口（GNOME 51 / Mutter 51 支持）。协议本身不提供置顶请求。

## 截图

> 建议放一张“宠物在桌面上 + 右键菜单展开”的截图：
> `<img src="docs/screenshot.png" width="720">`
>
> （仓库里的 `assets/logo.png` 是图标，截图需要你补一张。）

## 快速开始

### 方式一：Flatpak（推荐）

```bash
# 首次需要运行时（约 1–2 GB）
flatpak install --user flathub org.gnome.Platform//50 org.gnome.Sdk//50

# 首次克隆没有二进制：先编译，再打包（也可以直接 bash ./dsh-pet构建.sh --rebuild）
cd tauri/src-tauri && cargo build --release && cd ../..

# 打包 + 安装 + 导出到桌面（检测到旧版会询问是否先卸载；--uninstall 强制、--no-uninstall 跳过）
bash ./dsh-pet构建.sh
```

桌面上会生成 `dsh-pet.flatpak`，可直接双击安装。卸载：

```bash
flatpak uninstall com.pc2005.dshpet.desktop
```

### 方式二：源码直接跑

```bash
# Fedora
sudo dnf install gtk3-devel webkit2gtk4.1-devel wayland-devel gcc pkg-config cargo

cd tauri/src-tauri
cargo build --release
cd ..

bash tauri/run.sh
```

`bash tauri/run.sh --selftest` 只验证 VP9 alpha，不启动桌宠。

> 用 GitHub 网页上传会把脚本的可执行位丢掉；`git clone` 后可以 `chmod +x dsh-pet构建.sh tauri/run.sh`，
> 或者像上面这样直接用 `bash` 运行。

## 托盘

装了 AppIndicator 扩展的 GNOME 上，顶栏右侧会出现 dsh-pet 图标：

| 操作 | 行为 |
| --- | --- |
| 左键点图标 | 隐藏 / 显示宠物（隐藏时暂停视频，省 CPU） |
| 右键菜单 | 隐藏宠物 / 打开 DSH 网站 / 碎碎念开关 / 重载宠物配置 / 退出 |

- 托盘用纯 D-Bus 的 StatusNotifierItem（`ksni`），不依赖 libayatana-appindicator。
- Flatpak 里必须允许访问 `org.kde.StatusNotifierWatcher`，清单已经加上 `--talk-name=org.kde.StatusNotifierWatcher`；
  自己改清单时漏了这条，托盘会静默不出现。
- 同一时间只允许一个实例（`$XDG_RUNTIME_DIR/dsh-pet-desktop.sock` 当锁），重复启动会直接退出，避免出现两只宠物、两个图标。

## 架构

```
┌──────────────────────────── Tauri (GTK3 + WebKitGTK) ────────────────────────────┐
│  覆盖层窗口 = 工作区（透明、无边框、跳过任务栏）                                  │
│                                                                                  │
│  web/  渲染端（复用 dsh-pet runtime/electron-helper）                            │
│    ├─ sprite.js / events.js / renderer.js   → 动画、菜单、气泡、拖拽             │
│    └─ tauri-bridge.js  petBridge → invoke(...) / 接收 pet_displays               │
│                                                                                  │
│  Rust 主进程                                                                     │
│    ├─ http.rs       127.0.0.1 极简宿主：/config /state /thumb /pic /font /whisper│
│    ├─ config.rs     JSONC 合并（内置默认 + ~/.dsh/dsh-pet/main-config.jsonc）    │
│    ├─ wl.rs + wayland_shim.c   input region、xdg_toplevel_tag_v1                │
│    └─ 几何监视（GDK 每秒比对，分辨率/缩放变化 → 推 pet_displays + 改窗口尺寸）   │
└──────────────────────────────────────────────────────────────────────────────────┘
                                   │
                    页面坐标 == 工作区坐标（窗口铺满工作区，Mutter 把它夹到工作区原点；
                    面板那一条边用 panelInset 扣掉，页面 y=0 即工作区顶边）
```

几个关键决定（都是踩坑后的结论）：

1. **窗口不移动，宠物在层内移动**。Wayland 的 `xdg-shell` 不允许客户端定位窗口，GNOME 还会把放不下的窗口夹到工作区原点；所以窗口固定铺满工作区，靠 `tauri-bridge.js` 改 `.pet-sprite` 的 `left/top`，再把“窗口本该在的位置”从输入区域里扣回来。
2. **点击穿透走 raw `wl_surface`**。GTK3 的 Wayland 后端没有实现 `gdk_window_input_shape_combine_region`；而且 GTK 会在 commit 时用默认整窗 shape 覆盖，所以 shim 下发后还要每 400ms 重发一次。
3. **不读 X11 工作区**。`_NET_WORKAREA` 只有 XWayland 侧有，纯 Wayland 拿不到 strut；改成 `DSH_PET_PANEL_INSET` 配置（默认 32px 顶栏）。
4. **窗口先小后大**：先以 1200×700 映射，再放大到工作区。Mutter 会把“创建即填满工作区”的窗口自动标记为最大化，而 GNOME 的窗口菜单对最大化窗口禁用「置顶 / 改变大小」；放大后窗口仍落在工作区原点（用调试角标实测对齐）。
5. **NVIDIA 显式同步**：WebKitGTK + Wayland + NVIDIA 会直接断连（[WebKit #280210](https://bugs.webkit.org/show_bug.cgi?id=280210)），启动时自动设 `__NV_DISABLE_EXPLICIT_SYNC=1`，保留 DMABUF/GPU 合成。

## 配置

| 变量 | 默认 | 说明 |
|---|---|---|
| `DSH_PET_ASSETS` | 可执行文件旁的 `assets/`（Flatpak 里是 `/app/share/dsh-pet/assets`） | 素材目录 |
| `DSH_HOME` | `~/.dsh` | 用户数据根，读 `dsh-pet/main-config.jsonc` |
| `DSH_PET_PANEL_INSET` | `32` | 顶栏/任务栏占掉的逻辑像素；`0` = 不扣 |
| `DSH_PET_PANEL_EDGE` | `top` | `top` / `bottom` / `left` / `right` |
| `DSH_PET_WHISPER_SEC` | `120` | 本地碎碎念周期（秒）；`0` = 只留菜单手动触发 |
| `DSH_PET_WHISPER_LINES` | 内置 12 句 | 自定义文案文件（一行一句） |
| `DSH_PET_VERBOSE` | 关 | 详细日志（穿透翻转、HTTP 请求、页面 console） |
| `DSH_PET_TEST_GEOM` | — | 如 `1280x720`，启动 6 秒后模拟一次分辨率变化并自动还原 |
| `DSH_PET_XWAYLAND` | — | `1` 强制走 XWayland 后端（排查用） |

宠物本身的配置仍是 dsh-pet 的 `main-config.jsonc`（尺寸、位置、动画池、表情包描述等）。

## 漫游与固定

- 默认会自己走：每段待机动画播完掷一次骰，`animationWeights` 默认 `idle 10 / move 5 / turn 5`（百分比），
  即每轮约 5% 走、5% 转身、其余原地小动作；
- `pets[].fixedEnabled: true` 让宠物只播原地待机，不自己走、不自己翻朝向（菜单点播、拖拽、事件动画不受影响）；
- **注意**：`animationWeights` 要写在配置**顶层**（与 `pets` 同级）。渲染端的 `flattenConfigPets` 会用顶层值覆盖
  每只宠物的同名项，写在 `pets[]` 里不会生效；
- 宠物走到屏幕边缘时，朝向不对的移动会被 `planMove` 判为越界而放弃，所以它常先转身再走——这是预期行为。

## 已知限制

- **没有置顶**。GNOME 不实现 `wlr-layer-shell`，也没有客户端可用的 keep-above 请求；本项目已适配 `xdg_toplevel_tag_manager_v1`，但 tag 只是窗口身份标识（给规则/扩展匹配用），**不能自己置顶**。要真置顶需要 GNOME Shell 扩展（`Meta.Window.get_tag()` 匹配后 `make_above()`）。
- **只覆盖主屏**：多屏时宠物限制在主屏。
- **顶栏高度靠配置**：纯 Wayland 拿不到 strut，`DSH_PET_PANEL_INSET` 默认 32（GNOME 顶栏），换主题/移动面板时需要改。
- **分数缩放未验证**：1x / 2x 实测正常，1.25/1.5 需要再验。
- **平台**：在 Fedora 44 + GNOME 51 + WebKitGTK 2.52 上实测；其他发行版/桌面未验证（X11 会话可用 `DSH_PET_XWAYLAND=1` 兜底）。

## 目录

```
.
├── tauri/                     Tauri 外壳
│   ├── web/                   渲染端（复用 dsh-pet，含 tauri-bridge.js）
│   ├── src-tauri/             Rust：窗口/几何/HTTP/config/穿透/tag
│   ├── flatpak/               Flatpak manifest + 元数据 + 图标
│   ├── tools/                 调试小工具（X11 探针、GDK monitor 探针）
│   ├── run.sh                 源码运行
│   └── README.md              实现细节与排错
├── assets/                    动画 / 表情包 / 字体（61 MB）
├── dsh-pet构建.sh             一键 Flatpak 打包（并复制到桌面）
└── LICENSE                    MIT
```

## 开发与调试

```bash
# 只看 VP9 alpha 解码/合成
bash tauri/run.sh --selftest

# 模拟一次分辨率变化（不动系统设置）
DSH_PET_VERBOSE=1 DSH_PET_TEST_GEOM=1280x720 bash tauri/run.sh

# 看 Wayland 协议层（穿透/窗口形状/tag）
WAYLAND_DEBUG=1 bash tauri/run.sh 2>&1 | grep -E "set_input_region|xdg_toplevel"

# 看本机宿主返回的配置/状态
PORT=$(...); curl -s http://127.0.0.1:$PORT/dsh-pet-7340/state
```

## 许可

MIT © 2026 PC2005-cloud。渲染端与素材来自同作者的 [dsh-pet](https://github.com/PC2005-cloud/dsh-pet)。

> 如果你要发布的是**含素材**的仓库：`assets/` 约 61 MB，GitHub 单文件限制 100 MB 没问题，但克隆会比较重；也可以只保留代码、把素材放到 Release 附件里，README 里注明下载方式。
