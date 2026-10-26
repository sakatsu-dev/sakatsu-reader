"""Write SudachiDict's dictionary-form side table: id -> dictionary form as written, reading.

SudachiDict's 辞書形ID column names the line of the source lexicons — small, core, notcore, in
that build order — that holds an inflected word's dictionary form. Lindera keeps the ID as a raw
string and cannot resolve it, so the analyzer reads this table instead (`dictionary_forms.tsv`,
beside the compiled dictionary). Checked when written: every inflected entry with an ID resolves
to an entry in 終止形. 21 of them cross conjugation class on the way — classical forms (文語四段
了ん → 五段 了る) and variant classes (攀じる filed as both 五段 and 上一段) pointing at the modern
dictionary form — which is the form a reader looks up.

    python scripts/sudachidict/dictionary_forms.py <source dir> <compiled dictionary dir>
"""
import csv
import pathlib
import sys

source, output = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
rows = []
for name in ["small_lex.csv", "core_lex.csv", "notcore_lex.csv"]:
    with open(source / name, encoding="utf-8", newline="") as handle:
        rows.extend(csv.reader(handle))
referenced = sorted({int(row[13]) for row in rows if row[13] not in ("*", "")})
unresolved = crossed = 0
for row in rows:
    if row[13] not in ("*", "") and row[9] != "*":
        target = rows[int(row[13])]
        if not (target[10].startswith("終止形") or target[10] == "*"):
            unresolved += 1
        elif target[9] != row[9]:
            crossed += 1
assert unresolved == 0, f"{unresolved} inflected entries do not resolve to a 終止形 entry"
print(crossed, "resolve across a conjugation class (classical or variant forms)")
with open(output / "dictionary_forms.tsv", "w", encoding="utf-8", newline="\n") as handle:
    handle.write("# SudachiDict 20260723 dictionary_form_id -> display form, reading (katakana); "
                 "built from small+core+notcore in that order\n")
    for index in referenced:
        handle.write(f"{index}\t{rows[index][4]}\t{rows[index][11]}\n")
print(len(rows), "entries,", len(referenced), "dictionary forms ->", output / "dictionary_forms.tsv")
