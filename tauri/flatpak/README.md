# Flatpak 打包

把 `dsh-pet` Tauri 外壳打成 Flatpak（应用 ID：`com.pc2005.dshpet.desktop`），
运行时用 `org.gnome.Platform//50`（自带 GTK3、WebKitGTK 4.1、GStreamer 的 VP9/matroska 插件）。

## 一键构建

```bash
cd dsh-pet
./dsh-pet构建.sh               # 打包 + --user 安装 + 导出 bundle 并复制到桌面（~/桌面/dsh-pet.flatpak）
./dsh-pet构建.sh --rebuild     # 先 cargo build --release 再打包
./dsh-pet构建.sh --no-desktop  # 只构建，不往桌面复制
```

脚本在 `dsh-pet/` 根目录，内部会自动切到 `tauri/flatpak/` 用 manifest 构建。

启动：

```bash
flatpak run com.pc2005.dshpet.desktop
# 或应用菜单里搜 “dsh-pet”
```

首次构建会自动 `flatpak install --user flathub org.gnome.Platform//50 org.gnome.Sdk//50`
（约 1–2 GB，需要联网；装过就跳过）。

## 文件

| 文件 | 作用 |
|---|---|
| `com.pc2005.dshpet.desktop.yml` | manifest：运行时、权限（finish-args）、安装命令、素材来源 |
| `com.pc2005.dshpet.desktop.desktop` | 桌面入口 |
| `com.pc2005.dshpet.desktop.metainfo.xml` | AppStream 元数据（菜单/软件中心展示） |
| `com.pc2005.dshpet.desktop.png` | 256×256 图标（取自 `assets/logo.png`） |
| `../../dsh-pet构建.sh` | 构建脚本（在 `dsh-pet/` 根目录，构建完把 bundle 复制到桌面） |
| `repo/` | 本地 OSTree 仓库（`flatpak build-bundle` 用） |
| `*.flatpak` | 导出的单文件 bundle（同时复制一份到 `~/桌面/dsh-pet.flatpak`） |
| `build/` | flatpak-builder 的临时构建目录（可删，`--force-clean` 会重建） |

## 权限（finish-args）为什么这么配

| 参数 | 原因 |
|---|---|
| `--socket=wayland` | 渲染走 Wayland（纯 Wayland 实现，不读 X11 工作区） |
| `--socket=fallback-x11` | 仅在“机器没有 Wayland”的会话里兜底；Wayland 会话中不生效 |
| `--share=ipc` | WebKit/GTK 的共享内存 |
| `--device=dri` | WebKitGTK 走 GPU 合成；软件合成会丢 VP9 alpha（黑底/残影） |
| `--share=network` | 渲染端素材/状态走本机 `127.0.0.1` HTTP 宿主；没有网络权限时 loopback 起不来 |
| `--filesystem=~/.dsh:ro` | 读取用户自己的 `~/.dsh/dsh-pet/main-config.jsonc`（尺寸/位置/动画池）。沙箱内 `$HOME` 仍是真实家目录，程序按 `$HOME/.dsh` 就能读到 |
| `--env=DSH_PET_ASSETS=/app/share/dsh-pet/assets` | 素材装在 share 下，显式告诉程序 |
| `--env=__NV_DISABLE_EXPLICIT_SYNC=1` | 规避 WebKitGTK + NVIDIA + Wayland 的显式同步崩溃（[WebKit #280210](https://bugs.webkit.org/show_bug.cgi?id=280210)） |

## 检查与调试

```bash
flatpak run --command=sh com.pc2005.dshpet.desktop -c 'echo $HOME; ls -l $HOME/.dsh/dsh-pet'
flatpak run com.pc2005.dshpet.desktop                    # 前台跑，直接看 [dsh-pet] 日志
DSH_PET_VERBOSE=1 flatpak run com.pc2005.dshpet.desktop  # 详细日志（需在 finish-args 允许的环境里）
flatpak info com.pc2005.dshpet.desktop
flatpak uninstall com.pc2005.dshpet.desktop
```

> 注意：同一个桌宠同时只能有一个实例的覆盖层在屏上。调试前先 `pkill -x dsh-pet-desktop`。

## 想完全从源码构建？

当前 manifest 走的是"宿主编译 + 打包二进制"，因为 Rust/Tauri 在 flatpak-builder 里编译需要：

1. 加 SDK 扩展 `org.freedesktop.Sdk.Extension.rust-stable`（约 300 MB）；
2. `cargo vendor` 把依赖放进 `sources`（构建沙箱无网络，或给 flatpak-builder 开 `--share=network`）；
3. 在 SDK 里装 `webkit2gtk4.1-devel` 等头文件（`org.gnome.Sdk` 已带 GTK3/WebKit 开发文件）。

如果要做可复现的 CI 构建，建议：在 `flatpak/` 下放 `vendor/`（`cargo vendor vendor/`）+
`.cargo/config.toml` 指向 vendored 源，然后把模块换成
`buildsystem: simple` 的 `cargo build --release --offline --locked`，其余安装步骤不变。
