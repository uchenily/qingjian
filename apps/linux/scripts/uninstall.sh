#!/usr/bin/env bash
# 青简输入法 Linux 卸载脚本。删掉 install.sh 装的文件。
#
# 用法：
#   sudo ./uninstall.sh
set -euo pipefail

if [[ "$EUID" -ne 0 ]]; then
    echo "请用 sudo 运行：sudo ./uninstall.sh" >&2
    exit 1
fi

echo "青简输入法卸载中…"

# 检测 fcitx5 addon 目录
FCITX_ADDON_DIR="/usr/lib64/fcitx5"
if [[ ! -d "$FCITX_ADDON_DIR" && -d "/usr/lib/fcitx5" ]]; then
    FCITX_ADDON_DIR="/usr/lib/fcitx5"
fi

# 1. 删 addon .so
rm -f "$FCITX_ADDON_DIR/libqingjian.so" "$FCITX_ADDON_DIR/libqingjian_linux.so"

# 2. 删配置
rm -f /usr/share/fcitx5/addon/qingjian.conf
rm -f /usr/share/fcitx5/inputmethod/qingjian.conf

# 3. 删数据
rm -rf /usr/share/qingjian

echo "卸载完成。重启 fcitx5 生效：fcitx5 -r"
