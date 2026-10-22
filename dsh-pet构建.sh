#!/usr/bin/env bash
# 构建 dsh-pet Flatpak，并把打好的 bundle 复制到桌面
#
#   ./dsh-pet构建.sh               # 打包 + --user 安装 + 导出 .flatpak 到桌面
#   ./dsh-pet构建.sh --rebuild     # 先重新编译 Rust 再打包
#   ./dsh-pet构建.sh --no-desktop  # 只构建，不往桌面复制
#
# 依赖：flatpak、flatpak-builder、org.gnome.Platform//50、org.gnome.Sdk//50
#       （首次运行会自动从 flathub 安装缺失的 runtime/SDK，需联网）
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
APP_ID=com.pc2005.dshpet.desktop
FLATPAK_DIR="$HERE/tauri/flatpak"
MANIFEST="$FLATPAK_DIR/$APP_ID.yml"
BUILD_DIR="$FLATPAK_DIR/build"
REPO_DIR="$FLATPAK_DIR/repo"
BUNDLE="$FLATPAK_DIR/$APP_ID.flatpak"
BIN="$HERE/tauri/src-tauri/target/release/dsh-pet-desktop"
CARGO_HOME_DIR="${CARGO_HOME:-${HOME}/.cargo}"

REBUILD=0
TO_DESKTOP=1
for arg in "$@"; do
  case "$arg" in
    --rebuild) REBUILD=1 ;;
    --no-desktop) TO_DESKTOP=0 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "未知参数：$arg" >&2; exit 2 ;;
  esac
done

command -v flatpak >/dev/null || { echo "缺少 flatpak" >&2; exit 1; }
command -v flatpak-builder >/dev/null || { echo "缺少 flatpak-builder" >&2; exit 1; }

# 1) 二进制
if [[ $REBUILD -eq 1 || ! -x "$BIN" ]]; then
  echo "==> 编译 Tauri 外壳"
  ( cd "$HERE/tauri/src-tauri" && CARGO_HOME="$CARGO_HOME_DIR" cargo build --release )
fi
[[ -x "$BIN" ]] || { echo "未找到 $BIN" >&2; exit 1; }

# 2) runtime / SDK（按精确 ref 判断，已装则秒过）
ensure_ref() {
  local ref=$1
  if flatpak info "$ref" >/dev/null 2>&1; then
    return 0
  fi
  echo "==> 安装 $ref"
  flatpak install --user -y --noninteractive flathub "$ref"
}
ensure_ref "org.gnome.Platform//50"
ensure_ref "org.gnome.Sdk//50"

# 3) 构建 + 安装（--user）
echo "==> flatpak-builder"
flatpak-builder \
  --user \
  --install \
  --force-clean \
  --disable-rofiles-fuse \
  --repo="$REPO_DIR" \
  "$BUILD_DIR" \
  "$MANIFEST"

# 4) 导出单文件 bundle
echo "==> 导出 bundle"
flatpak build-bundle "$REPO_DIR" "$BUNDLE" "$APP_ID" \
  --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo
ls -lh "$BUNDLE"

# 5) 复制到桌面
if [[ $TO_DESKTOP -eq 1 ]]; then
  DESKTOP_DIR=""
  for d in "$HOME/桌面" "$HOME/Desktop"; do
    if [[ -d "$d" ]]; then DESKTOP_DIR="$d"; break; fi
  done
  if [[ -z "$DESKTOP_DIR" ]]; then
    DESKTOP_DIR="$HOME/桌面"
    mkdir -p "$DESKTOP_DIR"
  fi
  cp -f "$BUNDLE" "$DESKTOP_DIR/dsh-pet.flatpak"
  echo "==> 已复制到桌面：$DESKTOP_DIR/dsh-pet.flatpak"
fi

echo
echo "完成。启动："
echo "  flatpak run $APP_ID"
echo "  # 或在应用菜单里搜 “dsh-pet”"
echo "  # 桌面上的 dsh-pet.flatpak 可直接双击安装，或：flatpak install --user ~/桌面/dsh-pet.flatpak"
