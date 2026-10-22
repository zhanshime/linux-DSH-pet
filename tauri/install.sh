#!/usr/bin/env bash
# 把 dsh-pet Tauri 外壳安装到当前用户的桌面（无需 root）
#
#   ~/.local/share/dsh-pet/            二进制 + 素材
#   ~/.local/share/applications/       dsh-pet.desktop（应用菜单里可搜到）
#   ~/.local/share/icons/hicolor/...   logo
#
# 卸载：rm -rf ~/.local/share/dsh-pet ~/.local/share/applications/dsh-pet.desktop \
#              ~/.local/share/icons/hicolor/256x256/apps/dsh-pet.png
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BIN="$HERE/src-tauri/target/release/dsh-pet-desktop"
ASSETS="$HERE/../assets"
PREFIX="${PREFIX:-$HOME/.local}"

if [[ ! -x "$BIN" ]]; then
  echo "未找到构建产物：$BIN" >&2
  echo "请先构建：cd $HERE/src-tauri && CARGO_HOME=$HERE/../../.cargo-home cargo build --release" >&2
  exit 1
fi

APP_DIR="$PREFIX/share/dsh-pet"
mkdir -p "$APP_DIR" "$PREFIX/share/applications" "$PREFIX/share/icons/hicolor/256x256/apps"

install -m 0755 "$BIN" "$APP_DIR/dsh-pet-desktop"
rm -rf "$APP_DIR/assets"
cp -a "$ASSETS" "$APP_DIR/assets"

if [[ -f "$ASSETS/logo.png" ]]; then
  install -m 0644 "$ASSETS/logo.png" "$PREFIX/share/icons/hicolor/256x256/apps/dsh-pet.png"
fi

cat > "$PREFIX/share/applications/dsh-pet.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=dsh-pet
Name[zh_CN]=dsh-pet 桌宠
Comment=Desktop pet (dsh-pet renderer on Tauri)
Comment[zh_CN]=桌宠（dsh-pet 渲染端 + Tauri 外壳）
Exec=$APP_DIR/dsh-pet-desktop
Icon=dsh-pet
Terminal=false
Categories=Utility;
StartupNotify=false
StartupWMClass=dsh-pet
EOF

update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true

echo "已安装到 $APP_DIR"
echo "应用菜单搜索 “dsh-pet” 即可启动；也可以直接运行 $APP_DIR/dsh-pet-desktop"
