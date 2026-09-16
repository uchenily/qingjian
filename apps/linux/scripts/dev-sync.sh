#!/usr/bin/env bash
# 本地快速迭代：只编译并拷贝两个 .so，再重启 fcitx5。
#
# 用法（在仓库根目录）：
#   bash apps/linux/scripts/dev-sync.sh
#
# 前提：首次安装（install.sh / bundle.sh --install）已经做过一次，
#       conf 与数据文件已在 /usr/share/fcitx5 与 /usr/share/qingjian。
#       之后改 Rust / C++ 代码，跑这个脚本即可秒级更新。
#
# 只做三件事：
#   1. cargo build --release -p qingjian-linux        → target/release/libqingjian_linux.so
#   2. cmake --build apps/linux/shim/build            → apps/linux/shim/build/libqingjian.so
#   3. sudo cp 两个 .so 到 fcitx5 addon 目录 + fcitx5 -r
#
# 数据文件、conf 不动（没改就不用拷）。需要 sudo 是因为 /usr/lib*/fcitx5 只有 root 能写。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"

# fcitx5 addon 目录：lib64 优先，回退 lib
ADDON_DIR="/usr/lib64/fcitx5"
[[ -d "$ADDON_DIR" ]] || ADDON_DIR="/usr/lib/fcitx5"
if [[ ! -d "$ADDON_DIR" ]]; then
    echo "找不到 fcitx5 addon 目录（试过 /usr/lib64/fcitx5 和 /usr/lib/fcitx5）" >&2
    echo "先跑一次完整安装：sudo bash apps/linux/scripts/install.sh" >&2
    exit 1
fi

SHIM_BUILD="$ROOT/apps/linux/shim/build"
RUST_SO="$ROOT/target/release/libqingjian_linux.so"
SHIM_SO="$SHIM_BUILD/libqingjian.so"

if [[ ! -f "$RUST_SO" || ! -f "$SHIM_SO" ]]; then
    echo "尚未构建过，先跑一次完整构建：" >&2
    echo "  bash apps/linux/scripts/bundle.sh --install" >&2
    exit 1
fi

echo ">> 编译 Rust cdylib"
cargo build --release -p qingjian-linux

echo ">> 编译 C++ shim"
cmake --build "$SHIM_BUILD" --parallel

echo ">> 拷贝 .so 到 $ADDON_DIR（需要 sudo）"
sudo cp -f "$RUST_SO" "$ADDON_DIR/"
sudo cp -f "$SHIM_SO"  "$ADDON_DIR/"

echo ">> 重启 fcitx5"
fcitx5 -r -d &>/dev/null || fcitx5 -r

echo "完成。"
