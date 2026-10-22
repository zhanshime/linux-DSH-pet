#!/usr/bin/env bash
# 启动 dsh-pet Tauri 桌宠（Fedora / Wayland 原生覆盖层）
#
# 用法：
#   ./run.sh                 # 正常桌宠
#   ./run.sh --selftest      # 只验证 VP9-alpha 解码/合成，打印 JSON 后退出
#
# 环境变量（都有合理默认，一般不用设）：
#   DSH_PET_ASSETS=...       # 素材目录（默认 ../assets）
#   DSH_HOME=...             # 用户数据根（默认 ~/.dsh；读 dsh-pet/main-config.jsonc）
#   DSH_PET_KEEP_WEB=1       # 不把 display=web 的宠物提升为 both
#   DSH_PET_XWAYLAND=1       # 强制 XWayland 后端（排查用）
#   DSH_PET_VERBOSE=1        # 详细日志
#   DSH_PET_DEBUG_MARKS=1    # 页面四角画标记，验证窗口原点/尺寸
#   DSH_PET_REPAINT_PUMP=1   # 30fps 整窗重绘（软件合成兜底）
#   DSH_PET_CLEAR_HOOK=1     # 每帧清空窗口缓冲（软件合成兜底）
#   DSH_PET_FORCE_REPAINT=1  # 每帧强制整页重绘（软件合成兜底）
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BIN="$HERE/src-tauri/target/release/dsh-pet-desktop"

if [[ ! -x "$BIN" ]]; then
  echo "未找到构建产物：$BIN" >&2
  echo "请先构建：" >&2
  echo "  cd $HERE/src-tauri && CARGO_HOME=$HERE/../../.cargo-home cargo build --release" >&2
  exit 1
fi

export DSH_PET_ASSETS="${DSH_PET_ASSETS:-$HERE/../assets}"
exec "$BIN" "$@"
