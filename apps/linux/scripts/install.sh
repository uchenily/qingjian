#!/usr/bin/env bash
# 青简输入法 Linux 安装脚本。把包内文件复制到 /usr 下对应目录。
#
# 用法（在解压后的包目录里）：
#   sudo ./install.sh
#
# 装完重启 fcitx5，在配置工具里把「青简」加到输入法列表：
#   fcitx5 -r
set -euo pipefail

# 必须用 sudo
if [[ "$EUID" -ne 0 ]]; then
    echo "请用 sudo 运行：sudo ./install.sh" >&2
    exit 1
fi

# 包目录（脚本所在目录）
PKG_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$PKG_DIR"

echo "青简输入法安装中…"

# 检测 fcitx5 addon 目录（lib64 或 lib）
FCITX_ADDON_DIR="/usr/lib64/fcitx5"
if [[ ! -d "$FCITX_ADDON_DIR" && -d "/usr/lib/fcitx5" ]]; then
    FCITX_ADDON_DIR="/usr/lib/fcitx5"
fi

# 1. fcitx5 addon（.so）
echo ">> 安装 addon 到 $FCITX_ADDON_DIR"
mkdir -p "$FCITX_ADDON_DIR"
cp -f usr/lib64/fcitx5/libqingjian.so "$FCITX_ADDON_DIR/" 2>/dev/null \
    || cp -f usr/lib/fcitx5/libqingjian.so "$FCITX_ADDON_DIR/" 2>/dev/null \
    || true
cp -f usr/lib64/fcitx5/libqingjian_linux.so "$FCITX_ADDON_DIR/" 2>/dev/null \
    || cp -f usr/lib/fcitx5/libqingjian_linux.so "$FCITX_ADDON_DIR/" 2>/dev/null \
    || true
chmod 755 "$FCITX_ADDON_DIR"/libqingjian*.so

# 2. addon 描述 + 输入法注册
echo ">> 安装配置到 /usr/share/fcitx5"
mkdir -p /usr/share/fcitx5/addon /usr/share/fcitx5/inputmethod
cp -f usr/share/fcitx5/addon/qingjian.conf /usr/share/fcitx5/addon/
cp -f usr/share/fcitx5/inputmethod/qingjian.conf /usr/share/fcitx5/inputmethod/

# 3. 数据文件（词库 / 释义 / 语言模型）
echo ">> 安装数据到 /usr/share/qingjian"
mkdir -p /usr/share/qingjian
cp -rf usr/share/qingjian/* /usr/share/qingjian/

echo ""
echo "安装完成。"
echo ""
echo "下一步："
echo "  1. 重启 fcitx5：fcitx5 -r"
echo "  2. 打开 fcitx5 配置工具（fcitx5-configtool），把「青简」加到输入法列表"
echo ""
echo "卸载：sudo ./uninstall.sh"
