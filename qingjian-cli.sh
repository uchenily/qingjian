#!/usr/bin/env bash
# 查拼音候选： ./qingjian-cli.sh kdfa   不给参数则进入交互模式。
set -euo pipefail

if (( $# > 0 )); then
    inputs=("$1")
else
    inputs=()
fi

cargo run -p qingjian-cli --release -- \
    --shuangpin xiaohe --fuzzy n-l,in-ing \
    --neural data/model/model.qjm --neural-async \
    "${inputs[@]}"
