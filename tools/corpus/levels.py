# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""把词汇等级表转成青简的 `levels-en.tsv`（`词\\t等级`，`# levels` 行给出等级从易到难的顺序）。

CEFR-J Wordlist 1.5（A1–B2）+ Octanove Vocabulary Profile（C1/C2），`headword,pos,CEFR,…`，
斜杠分隔的拼写变体各算一条，同一个词多个词性取最低等级。

用法：
    uv run tools/corpus/levels.py data/levels/cefrj-vocabulary-profile-1.5.csv \\
        data/levels/octanove-vocabulary-profile-c1c2-1.0.csv -o assets/levels/levels-en.tsv
"""

from __future__ import annotations

import argparse
import csv
from pathlib import Path

LEVELS = ["A1", "A2", "B1", "B2", "C1", "C2"]

HEADER = [
    "# 英文词汇等级（CEFR）。来源：The CEFR-J Wordlist Version 1.5, compiled by Yukio Tono, Tokyo University of Foreign Studies,",
    "# retrieved from http://www.cefr-j.org/download.html（A1–B2，研究与商业用途免费，须署名）；",
    "# Octanove Vocabulary Profile C1/C2 1.0（CC BY-SA 4.0，https://github.com/openlanguageprofiles/olp-en-cefrj）。",
    "# 由 tools/corpus/levels.py 生成。格式：词\\\\t等级；`# levels` 行是等级从易到难的顺序。",
]


def clean_english(headword: str) -> list[str]:
    """`a.m./A.M./am/AM` → 各拼写；统一小写。"""
    words = []
    for part in headword.split("/"):
        word = part.strip().lower()
        if word and word not in words:
            words.append(word)
    return words


def convert(sources: list[Path]) -> dict[str, int]:
    rank = {level: index for index, level in enumerate(LEVELS)}
    table: dict[str, int] = {}
    for source in sources:
        with source.open(encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle):
                level = row["CEFR"].strip()
                if level not in rank:
                    continue
                for word in clean_english(row["headword"]):
                    table[word] = min(table.get(word, rank[level]), rank[level])
    return table


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("sources", nargs="+", type=Path)
    parser.add_argument("-o", "--output", type=Path, required=True)
    args = parser.parse_args()
    table = convert(args.sources)
    lines = HEADER + ["# levels\t" + "\t".join(LEVELS)]
    for word, index in sorted(table.items(), key=lambda item: (item[1], item[0])):
        lines.append(f"{word}\t{LEVELS[index]}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    counts = {level: 0 for level in LEVELS}
    for index in table.values():
        counts[LEVELS[index]] += 1
    print(f"{args.output}: {len(table)} 词 {counts}")


if __name__ == "__main__":
    main()
