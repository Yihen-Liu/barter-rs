#!/usr/bin/env bash
set -e

echo "🦀 Rust 编译优化脚本启动中..."

# 检查总内存
TOTAL_MEM=$(free -m | awk '/^Mem:/{print $2}')
echo "📦 系统总内存: ${TOTAL_MEM} MB"

# 检查是否存在 swap
HAS_SWAP=$(swapon --show | wc -l)

if [ "$HAS_SWAP" -eq 0 ]; then
  echo "⚙️ 检测到无 swap，正在创建 4G swap 空间..."
  sudo fallocate -l 4G /swapfile
  sudo chmod 600 /swapfile
  sudo mkswap /swapfile
  sudo swapon /swapfile
  echo "✅ 已创建并启用 4G swap"
else
  echo "✅ 检测到已有 swap"
fi

# 检查 cargo 是否存在
if ! command -v cargo &>/dev/null; then
  echo "❌ 未检测到 cargo，请先安装 Rust。"
  exit 1
fi

# 启用增量编译
export CARGO_INCREMENTAL=1

# 降低并行度防止 OOM
JOBS=1

echo "🛠️ 开始编译项目（并行任务: $JOBS）..."
echo "-----------------------------------------"

cargo build -j "$JOBS" "$@" 2>&1 | tee build.log

echo "-----------------------------------------"
echo "🎉 编译结束，日志已保存到 build.log"
