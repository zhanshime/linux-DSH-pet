# dsh-pet Tauri 外壳（Fedora / Wayland 原生）

把 `dsh-pet` 插件的桌面渲染端（`runtime/electron-helper`）装进 Tauri v2，不依赖 Electron / DSH，
在 Fedora GNOME（Wayland 会话）里作为独立桌宠运行。

## 为什么是"全屏透明覆盖层"

Wayland 的 `xdg-shell` **不允许客户端决定窗口位置**，GNOME 还会把托管窗口夹回屏幕内
（实测：请求 1203,489 的 924×744 窗口被 Mutter 放到 996,336）。Electron 版"窗口 = 宠物 + 外扩
边距、跟着宠物跑"的模型在 Wayland 上无法复刻，所以这里改成：

```
窗口 = 主屏工作区（1920×1048，Mutter 会把它放在工作区原点 (0,32)）
  └─ 页面坐标 = 工作区坐标（一一对应，无需偏移补偿）
       └─ 宠物在层内移动：桥接层把 .pet-sprite 摆到 boxX/boxY
       └─ 透明区穿透：wl_surface.set_input_region 只框住宠物包围盒
```

这不只是妥协：浏览器端本来就在用"视口坐标"这套模型，改完之后坐标反而更直接。

## 关键技术点

| 问题 | 做法 |
|---|---|
| 窗口定位 | 不做。窗口固定铺满工作区；宠物在层内移动（`web/tauri-bridge.js` 里改 `.pet-sprite` 的 left/top） |
| 点击穿透 | C shim 直接对 GTK 的 `wl_surface` 下发 input region（GTK3 Wayland **没有**实现 `gdk_window_input_shape_combine_region`，调了等于没调） |
| region 被覆盖 | GTK 在 map/commit 时会用默认 shape（整窗 `-10,-10,1940,1068`）覆盖，主进程每 400ms 强制重发一次 |
| Alpha 视频 | VP9 alpha 由 GStreamer 解码，WebKitGTK 的 GPU 合成路径能正确保留透明；软件合成路径会把视频矩形画成黑底 |
| 显式同步崩溃 | WebKitGTK + NVIDIA + Wayland 的已知 bug（[WebKit #280210](https://bugs.webkit.org/show_bug.cgi?id=280210)）。启动时自动设置 `__NV_DISABLE_EXPLICIT_SYNC=1`，保留 DMABUF/GPU 合成 |
| 无置顶 | GNOME 没有 keep-above 协议（也不实现 `wlr-layer-shell`）。窗口是普通窗口：聚焦别的应用后桌宠会被盖住，这是 Wayland 下的既定取舍 |

## 构建

```bash
cd dsh-pet/tauri/src-tauri
CARGO_HOME=/var/home/zhan/yingyong/.cargo-home cargo build --release
```

依赖：`gtk3-devel`、`webkit2gtk4.1-devel`、`wayland-devel`、`gcc`、`pkg-config`。
`build.rs` 会用 pkg-config 的 cflags 编译 `src/wayland_shim.c`，并链接 `gdk-3` / `wayland-client`。

## 运行

```bash
./run.sh                  # 桌宠
./run.sh --selftest       # 只验证 VP9-alpha 解码/合成，打印 JSON 后退出
```

安装到桌面菜单（复制二进制 + 素材 + .desktop）：

```bash
./install.sh              # 装到 ~/.local/{share/dsh-pet,share/applications,share/icons}
```

## Flatpak

```bash
cd dsh-pet
./dsh-pet构建.sh                  # 打包 + --user 安装 + 把 bundle 复制到桌面
flatpak run com.pc2005.dshpet.desktop
```

细节（权限为什么这么配、完全源码构建怎么改）见 [flatpak/README.md](flatpak/README.md)。

## 环境变量

| 变量 | 作用 |
|---|---|
| `DSH_PET_ASSETS` | 素材目录（默认 `../assets`） |
| `DSH_HOME` | 用户数据根（默认 `~/.dsh`，读 `dsh-pet/main-config.jsonc`） |
| `DSH_PET_KEEP_WEB=1` | 不把 `display:"web"` 的宠物提升为 `both` |
| `DSH_PET_XWAYLAND=1` | 强制走 XWayland 后端（XShape + XQueryPointer 轮询），排查用 |
| `DSH_PET_VERBOSE=1` | 打开详细日志（穿透翻转、页面 console.log 等） |
| `DSH_PET_DEBUG_MARKS=1` | 页面四角画调试标记（验证窗口原点/尺寸） |
| `DSH_PET_TEST_GEOM=1280x720` | 启动 6 秒后模拟一次分辨率变化（随后自动还原），验证自适应链路 |
| `DSH_PET_PANEL_INSET` | 顶栏/任务栏占掉的逻辑像素，默认 `32`（GNOME 默认顶栏）；`0` = 不扣 |
| `DSH_PET_PANEL_EDGE` | 边距在哪条边：`top`（默认）/`bottom`/`left`/`right` |
| `DSH_PET_WHISPER_SEC` | 本地碎碎念周期（秒），默认 `120`；`0` = 关闭自动、只保留菜单手动触发 |
| `DSH_PET_WHISPER_LINES` | 自定义文案文件（一行一句），覆盖内置 12 句 |
| `DSH_PET_REPAINT_PUMP=1` | 30fps 整窗重绘（软件合成路径下压残影的兜底） |
| `DSH_PET_CLEAR_HOOK=1` | 每帧把窗口缓冲清成全透明（同上） |
| `DSH_PET_FORCE_REPAINT=1` | 每帧强制 WebKit 整页重绘（同上） |

## 分辨率 / 缩放变化（纯 Wayland）

不依赖 X11：窗口始终铺满**整块显示器**，可视区用 `DSH_PET_PANEL_INSET` 扣掉面板占的那条边。

- 为什么窗口要铺满整屏：Mutter 会把「放不进工作区」的窗口夹到工作区原点，于是页面 `(0,0)`
  正好落在工作区左上角；再把面板那条边从 `VIEW` 里扣掉，**页面坐标 = 工作区坐标**，
  完全不需要知道窗口在屏幕上的真实坐标（Wayland 也没法查）。
- 顶栏高度拿不到 strut（`xdg-shell` 没有工作区概念，GNOME 也不实现 `wlr-layer-shell`），
  所以按配置来：默认 `32`（GNOME 默认顶栏），顶栏自动隐藏/全屏使用时设
  `DSH_PET_PANEL_INSET=0`；`DSH_PET_PANEL_EDGE` 指定在哪条边（`top`/`bottom`/`left`/`right`）。
- 主进程每秒比对一次 GDK 的主屏几何；分辨率/缩放变化时先把新几何 `pet_displays` 推给渲染端
  （`applyDeskGeometry` 重挂 `VIEW/AREAS` 并 `relayout()`），再把窗口改成新的整屏尺寸；
  渲染端随后收到 resize，按新边界重新定位（固定角落回新角落，拖过的按比例落点重算）。

自测（不动系统分辨率）：

```bash
DSH_PET_VERBOSE=1 DSH_PET_TEST_GEOM=1280x720 ./run.sh
# [dsh-pet] 覆盖层窗口: 1920×1080（整屏）；可视区 1920×1048（inset=32 top）
# [dsh-pet] 测试：模拟工作区变化 -> 1280×720
# [dsh-pet] 桌面几何变化: 1280×720 -> 1920×1080（可视区 1920×1048，scale 1）
```

## 本地碎碎念 / 表情包

壳里没有 DSH 模型，碎碎念改为**纯本地**：宿主按周期往 `/state` 写
`pets.<id>.say = { counter, data: { text, image } }`，渲染端轮询到 counter 变化就播说话动画 + 气泡
（`applySayLeaf → showWhisper`）。

- 文案：内置 12 句，可用 `DSH_PET_WHISPER_LINES` 指向自定义文件覆盖；
- 配图：从 `assets/memes/*.png` 与 `$DSH_HOME/dsh-pet/<素材根>-memes/*.png` 里随机挑一张
  （不与上一张重复），渲染端拼成 `/pic/memes/<素材根>/<名>.png`；
- 周期：`DSH_PET_WHISPER_SEC`，默认 120 秒；
- 手动：右键菜单 →「碎碎念」会 `POST /whisper` 立即产出一句，不受周期限制；
- 关闭：`DSH_PET_WHISPER_SEC=0` 只停自动，菜单仍可用；`whisperEnabled=false` 则自动与配图都停。

## 排查

- **宠物整块黑底 / 拖动有拖影**：说明走到了 WebKit 软件合成路径（常见于 GL 初始化失败）。
  先确认 `__NV_DISABLE_EXPLICIT_SYNC=1` 是否生效、`GDK_GL` 是否被外部设置为 `disable`；
  临时兜底可加 `DSH_PET_REPAINT_PUMP=1 DSH_PET_CLEAR_HOOK=1`（费 CPU，且仍可能有轻微残影）。
- **启动即退出，日志有 `Error 71 (协议错误)`**：显式同步握手失败，确认 `__NV_DISABLE_EXPLICIT_SYNC=1`；
  或设 `WEBKIT_DISABLE_DMABUF_RENDERER=1`（能跑但性能差、透明可能异常）。
- **空白处点不动**：input region 被 GTK 覆盖。看 `DSH_PET_VERBOSE=1` 日志里 region 是否在按宠物矩形重发；
  也可以 `WAYLAND_DEBUG=1 ./run.sh 2>&1 | grep -E "set_input_region|region.*add"` 直接看协议。
- **完全看不到宠物**：别的窗口在前台（无置顶），按 Super+D 回桌面；或用 `DSH_PET_DEBUG_MARKS=1` 确认覆盖层在屏上。

## 文件

```
web/                     渲染端（复用插件 runtime/electron-helper）+ tauri-bridge.js 桥接
  tauri-bridge.js        petBridge → Tauri 命令；覆盖层模式下的 sprite 定位
src-tauri/src/main.rs    窗口、命令、覆盖层/输入区域/重绘策略
src-tauri/src/wl.rs      点击穿透（Wayland input region）
src-tauri/src/wayland_shim.c  GTK 的 wl_surface 拿不到输入形状 API，用 C 补
src-tauri/src/http.rs    本机 HTTP 宿主（config/state/动画/表情包/字体）
src-tauri/src/config.rs  JSONC 合并（内置默认 + 用户 main-config.jsonc）
src-tauri/src/pointer.rs XWayland 兜底：X11 指针轮询翻转穿透
tools/                   调试工具（xinspect/xdo/xtestwin/gdkway）
```

## 已知限制

- 没有置顶：桌宠只在桌面（或它所在的层）可见，其他窗口聚焦后会在它上面。
- 只覆盖主屏工作区：多屏时宠物限制在主屏。
- 顶栏区域不可达：工作区从 (0,32) 起，宠物不会跑到顶栏下面（这也是好事）。
- 无 Wayland 输入注入工具，穿透/拖拽只能用真实鼠标验证。
