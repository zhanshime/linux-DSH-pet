#!/usr/bin/env bash
# 构建 dsh-pet Flatpak，并把打好的 bundle 复制到桌面
#
#   ./dsh-pet构建.sh               # 打包 + --user 安装 + 导出 .flatpak 到桌面
#   ./dsh-pet构建.sh --rebuild     # 先重新编译 Rust 再打包
#   ./dsh-pet构建.sh --no-desktop  # 只构建，不往桌面复制
#   ./dsh-pet构建.sh --uninstall   # 构建前先卸载已安装的旧版本（不询问）
#   ./dsh-pet构建.sh --no-uninstall# 保留旧版本、不询问
#
# 默认：检测到已安装时会询问“是否先卸载旧版本”（y/N）；非交互环境默认保留。
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
# 0=有安装时询问；1=不卸载；2=卸载
UNINSTALL_MODE=0
for arg in "$@"; do
  case "$arg" in
    --rebuild) REBUILD=1 ;;
    --no-desktop) TO_DESKTOP=0 ;;
    --uninstall) UNINSTALL_MODE=2 ;;
    --no-uninstall) UNINSTALL_MODE=1 ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "未知参数：$arg" >&2; exit 2 ;;
  esac
done

command -v flatpak >/dev/null || { echo "缺少 flatpak" >&2; exit 1; }
command -v flatpak-builder >/dev/null || { echo "缺少 flatpak-builder" >&2; exit 1; }

is_installed() {
  flatpak info "$APP_ID" >/dev/null 2>&1
}

uninstall_app() {
  echo "==> 卸载旧版本 $APP_ID"
  # 运行中的实例会占着应用目录，先停掉
  pkill -x dsh-pet-desktop 2>/dev/null || true
  if flatpak info --user "$APP_ID" >/dev/null 2>&1; then
    flatpak uninstall --user -y "$APP_ID"
    echo "    已卸载用户级安装"
  elif is_installed; then
    # 系统级安装需要管理员；不擅自 sudo，给出提示后继续
    echo "    检测到系统级安装，需要手动卸载：sudo flatpak uninstall -y $APP_ID" >&2
  else
    echo "    未安装，跳过"
  fi
}

# 1) 二进制
if [[ $REBUILD -eq 1 || ! -x "$BIN" ]]; then
  echo "==> 编译 Tauri 外壳"
  ( cd "$HERE/tauri/src-tauri" && CARGO_HOME="$CARGO_HOME_DIR" cargo build --release )
fi
[[ -x "$BIN" ]] || { echo "未找到 $BIN" >&2; exit 1; }

# 2) 卸载旧版本（询问 / 强制 / 跳过）
if [[ $UNINSTALL_MODE -eq 0 ]] && is_installed; then
  if [[ -t 0 ]]; then
    printf '检测到已安装 %s，是否先卸载旧版本？[y/N] ' "$APP_ID"
    read -r answer || answer=""
    case "$answer" in
      [yY]|[yY][eE][sS]) UNINSTALL_MODE=2 ;;
      *) UNINSTALL_MODE=1 ;;
    esac
  else
    echo "==> 检测到已安装；非交互环境默认保留（加 --uninstall 可强制先卸载）"
    UNINSTALL_MODE=1
  fi
fi
[[ $UNINSTALL_MODE -eq 2 ]] && uninstall_app

# 3) runtime / SDK（按精确 ref 判断，已装则秒过）
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

# 4) 构建 + 安装（--user）
echo "==> flatpak-builder"
flatpak-builder \
  --user \
  --install \
  --force-clean \
  --disable-rofiles-fuse \
  --repo="$REPO_DIR" \
  "$BUILD_DIR" \
  "$MANIFEST"

# 5) 导出单文件 bundle
echo "==> 导出 bundle"
flatpak build-bundle "$REPO_DIR" "$BUNDLE" "$APP_ID" \
  --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo
ls -lh "$BUNDLE"

# 6) 复制到桌面
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
