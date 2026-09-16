#!/usr/bin/env bash
# 把 qingjian-linux 打包成通用安装包（.tar.gz）。
#
#   scripts/bundle.sh                # 打包到 target/dist/qingjian-<版本>-linux-x86_64.tar.gz
#   scripts/bundle.sh --install      # 打包并直接安装到 /usr（需要 sudo）
#
# 包内布局（解压到 /usr 即可用）：
#   usr/lib64/fcitx5/libqingjian.so          C++ shim（fcitx5 addon）
#   usr/lib64/fcitx5/libqingjian_linux.so    Rust cdylib（Engine）
#   usr/share/fcitx5/addon/qingjian.conf     addon 描述
#   usr/share/fcitx5/inputmethod/qingjian.conf 输入法注册
#   usr/share/qingjian/                      词库 / 释义 / 语言模型 / 英文词表
#   install.sh / uninstall.sh                安装 / 卸载脚本
#
# 用户用法：
#   tar -xzf qingjian-*.tar.gz
#   cd qingjian-*
#   sudo ./install.sh          # 装到 /usr
#   fcitx5 -r                  # 重启 fcitx5
#   # 在 fcitx5 配置工具里把「青简」加到输入法列表
#
# 卸载：
#   sudo ./uninstall.sh
#
# 依赖：Rust 1.96.0、C++20 编译器、fcitx5 开发包（fcitx5-devel / libfcitx5-dev）、CMake ≥ 3.16。
# 这些只在打包机上需要；用户机器只要装了 fcitx5 就能直接用包，不需要编译环境。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"

# ── 版本号 ──
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' apps/linux/Cargo.toml | head -1)"
ARCH="$(uname -m)"
DIST="$ROOT/target/dist/qingjian-$VERSION-linux-$ARCH"
TARBALL="$ROOT/target/dist/qingjian-$VERSION-linux-$ARCH.tar.gz"

# ── 命令行参数 ──
DO_INSTALL=0
[[ "${1:-}" == "--install" ]] && DO_INSTALL=1

# ── 构建标识 ──
GIT_REV="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
if [[ -n "$(git status --porcelain 2>/dev/null)" ]]; then GIT_REV="${GIT_REV}+"; fi
export QINGJIAN_BUILD="${GIT_REV} · $(date +%Y-%m-%d)"
echo "青简 Linux $VERSION（$ARCH）打包开始，构建标识：$QINGJIAN_BUILD"

# ── 1. 编译 Rust cdylib ──
echo ">> 编译 Rust cdylib"
cargo build --release -p qingjian-linux --locked

# ── 2. 编译 C++ shim ──
echo ">> 编译 C++ shim"
BUILD_DIR="$ROOT/apps/linux/shim/build"
cmake -B "$BUILD_DIR" -S apps/linux/shim \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr
cmake --build "$BUILD_DIR"

# ── 3. 确保产品词库跟词库源同步 ──
# data/ 被 gitignore，开发分支切换词库改动后本地 .qj 很容易还是旧的；
# 发布 CI 下载的数据包并 touch 过生成文件，不会触发这里的重打包。
if [[ -f assets/lexicon/dict.tsv && ( ! -f data/generated/dict.qj || assets/lexicon/dict.tsv -nt data/generated/dict.qj ) ]]; then
    echo ">> 词库源较新，重打包基础词库"
    cargo run --release -p qingjian-dict-convert --locked -- \
        pack dict --input assets/lexicon/dict.tsv \
        --name 青简基础词库 --license "MIT AND Unicode-3.0"
fi

# ── 4. 组装包目录 ──
echo ">> 组装包目录"
rm -rf "$DIST"
mkdir -p "$DIST"

# fcitx5 addon（.so + 配置）
# lib 目录按架构：x86_64 → lib64，aarch64 → lib64，其他 → lib
LIB_DIR="lib64"
[[ "$ARCH" == "aarch64" ]] && LIB_DIR="lib64"
FCITX_ADDON_DIR="$DIST/usr/$LIB_DIR/fcitx5"
FCITX_DATA_DIR="$DIST/usr/share/fcitx5"
mkdir -p "$FCITX_ADDON_DIR" "$FCITX_DATA_DIR/addon" "$FCITX_DATA_DIR/inputmethod"

cp "$BUILD_DIR/libqingjian.so" "$FCITX_ADDON_DIR/"
cp "$ROOT/target/release/libqingjian_linux.so" "$FCITX_ADDON_DIR/"
cp apps/linux/shim/qingjian-addon.conf "$FCITX_DATA_DIR/addon/qingjian.conf"
cp apps/linux/shim/qingjian.conf "$FCITX_DATA_DIR/inputmethod/qingjian.conf"

# ── 4. 数据文件 ──
echo ">> 组装数据文件"
DATA_DIR="$DIST/usr/share/qingjian"
mkdir -p "$DATA_DIR"

# 产品数据（data/generated/）优先，没有就退回 assets/sample/ 样例
if [[ -f data/generated/dict.qj || -f data/generated/dict.tsv ]]; then
    echo "使用 data/generated/ 的产品数据"
    # 词库
    [[ -f data/generated/dict.qj ]] && cp data/generated/dict.qj "$DATA_DIR/"
    # 领域词库
    if ls data/generated/dicts/*.qj >/dev/null 2>&1; then
        mkdir -p "$DATA_DIR/dicts"
        cp data/generated/dicts/*.qj "$DATA_DIR/dicts/"
    fi
    # 语言模型
    [[ -f data/generated/lm.qj ]] && cp data/generated/lm.qj "$DATA_DIR/"
    # 释义表
    for lang in en ja zh; do
        [[ -f data/generated/glossary-$lang.qj ]] && cp data/generated/glossary-$lang.qj "$DATA_DIR/"
    done
    # 英文词表
    [[ -f data/generated/english.tsv ]] && cp data/generated/english.tsv "$DATA_DIR/"
    # 神经整句模型（可选，约 100 MB）
    if [[ -f data/model/model.qjm ]]; then
        mkdir -p "$DATA_DIR/model"
        cp data/model/model.qjm "$DATA_DIR/model/"
        echo "  含神经整句模型"
    fi
else
    echo "退回 assets/sample/ 样例数据（功能受限，建议下载产品数据）"
    cp assets/sample/*.tsv "$DATA_DIR/" 2>/dev/null || true
fi

# emoji 表（Unicode CLDR，可发布）
if ls assets/emoji/*.tsv >/dev/null 2>&1; then
    cp assets/emoji/*.tsv "$DATA_DIR/"
fi

# ── 5. 安装 / 卸载脚本 ──
echo ">> 写安装 / 卸载脚本"
cp apps/linux/scripts/install.sh "$DIST/install.sh"
cp apps/linux/scripts/uninstall.sh "$DIST/uninstall.sh"
chmod +x "$DIST/install.sh" "$DIST/uninstall.sh"

# ── 6. 打包 ──
echo ">> 打包"
mkdir -p "$ROOT/target/dist"
tar -czf "$TARBALL" -C "$ROOT/target/dist" "qingjian-$VERSION-linux-$ARCH"
echo ""
echo "打包完成：$TARBALL"
echo "大小：$(du -h "$TARBALL" | cut -f1)"
echo ""
echo "用户安装："
echo "  tar -xzf $(basename "$TARBALL")"
echo "  cd qingjian-$VERSION-linux-$ARCH"
echo "  sudo ./install.sh"
echo "  fcitx5 -r"

# ── 7. 直接安装（可选）──
if [[ "$DO_INSTALL" == "1" ]]; then
    echo ""
    echo ">> 直接安装到 /usr"
    cd "$DIST"
    sudo ./install.sh
fi
