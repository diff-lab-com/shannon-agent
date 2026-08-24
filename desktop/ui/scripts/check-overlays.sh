#!/usr/bin/env bash
# T1.2 验收:组件目录不得新增手写 fixed inset-0 弹窗 overlay(白名单除外)
# 白名单解释:
#   - Layout.tsx — 移动端 sidebar overlay (非弹窗,关侧栏用)
#   - artifact/ArtifactPanel.tsx — 全屏 artifact 容器 (非弹窗)
#   - ui/dialog.tsx — shadcn base dialog 原语 (Base UI Backdrop)
#   - ui/modal.tsx — Shannon Modal 原语 (R1b 已迁 Base UI)
#   - ui/side-panel.tsx — Shannon SidePanel 原语 (R1c 已迁 Base UI)
set -euo pipefail
cd "$(dirname "$0")/.."

WHITELIST=(
  src/components/Layout.tsx
  src/components/artifact/ArtifactPanel.tsx
  src/components/ui/dialog.tsx
  src/components/ui/modal.tsx
  src/components/ui/side-panel.tsx
)

# 1) 收集所有命中 (注释行也一起拿)
raw_hits=$(grep -rn 'fixed inset-0' src/components/ --include='*.tsx' || true)

# 2) 过滤掉:以 // 或 /* 开头的注释行(允许前导空白)
code_hits=$(printf '%s\n' "$raw_hits" | grep -vE '^\s*//|^\s*/\*' || true)

# 3) 过滤掉白名单文件
violations=$(printf '%s\n' "$code_hits" | grep -vE "$(IFS='|'; echo "${WHITELIST[*]}")" || true)

if [ -n "$violations" ]; then
  echo "✗ New hand-rolled 'fixed inset-0' overlay detected outside whitelist:"
  echo "$violations"
  exit 1
fi

echo "✓ OK: no hand-rolled overlays outside whitelist (Layout / ArtifactPanel / dialog / modal / side-panel)"
