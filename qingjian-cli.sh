#!/usr/bin/env bash
# 查拼音候选： ./qingjian-cli.sh kdfa   不给参数则进入交互模式。
set -euo pipefail

if (( $# > 0 )); then
    inputs=("$1")
else
    inputs=()
fi

# 本地整句模型：通变优先，知微回退，旧路径兼容
neural=""
for cand in \
    data/models/hanzhang-tongbian/hanzhang-tongbian-small.qjm \
    data/models/hanzhang-tongbian/model.qjm \
    data/models/hanzhang-zhiwei/hanzhang-zhiwei-small.qjm \
    data/models/hanzhang-zhiwei/model.qjm \
    data/model/model.qjm
do
    if [[ -f "$cand" ]]; then
        neural="$cand"
        break
    fi
done

args=(--shuangpin xiaohe --fuzzy n-l,in-ing)
if [[ -n "$neural" ]]; then
    args+=(--neural "$neural" --neural-async)
fi

cargo run -p qingjian-cli --release -- "${args[@]}" "${inputs[@]}"
