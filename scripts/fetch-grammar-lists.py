"""Fetch two public JLPT grammar lists, for evaluation only: names, levels and links — never content.

    python scripts/fetch-grammar-lists.py --download

What it reads
-------------
* **絵でわかる日本語** (www.edewakaru.com): the grammar-level categories (初級 N4, 中級 N2〜N3,
  上級 N1〜N2, 日本語能力試験にない文型), the 口語形 category (casual contractions) and the
  類似文法比較 category (comparisons of grammar that is easy to confuse). Only each post's title
  and address are kept; many titles state their own JLPT level, which is used where present.
* **JLPT Grammar List** (jlptgrammarlist.neocities.org): each point's name, sense number, short
  gloss and level. Its example sentences and audio are **not** kept.

Neither site declares a licence, so both have the standing `project notes` §8 gives KWDLC, Imabi and
Tae Kim: **consult, never import**. The lists are read to measure the app's grammar packs —
`core/tests/grammar_lists.rs` — and nothing from them is committed or shipped. They are written
beside the other evaluation corpora (`corpora/grammar-lists` next to the dictionaries directory),
or to `--out` / `SAKATSU_GRAMMAR_LISTS`.

Politeness: about ninety small index pages, one every two seconds, each cached on disk so a rerun
fetches nothing. Neither site's robots.txt restricts this (edewakaru has none; Neocities allows
all). Nothing is fetched without `--download`.

Python rather than PowerShell: every `scripts/*.ps1` is bundled into the installer
(`tauri.conf.json` resources), and evaluation tooling has no business there.
"""
import argparse
import datetime
import hashlib
import html
import os
import pathlib
import re
import sys
import time
import urllib.request

AGENT = "Mozilla/5.0 (compatible; sakatsu-reader grammar-list evaluation; one request per 2s)"
DELAY = 2.0

EDEWAKARU = "https://www.edewakaru.com/archives/cat_{}.html"
# category id → (list, level). Levels are the category's own range; a title's stated level wins.
EDEWAKARU_CATEGORIES = {
    "116866": ("level", "N5-N4"),   # 初級文法（N４）— the beginner category
    "116821": ("level", "N3-N2"),   # 中級文法（N２〜N３）
    "116822": ("level", "N2-N1"),   # 上級文法（N１〜N２）
    "177818": ("level", "none"),    # 日本語能力試験にない文型 — patterns outside the JLPT
    "159166": ("casual", ""),       # 口語形 — casual contractions
    "215705": ("compare", ""),      # 類似文法比較 — confusable grammar, compared
}
JLPT_GRAMMAR_LIST = "https://jlptgrammarlist.neocities.org/"

FULLWIDTH = str.maketrans("０１２３４５", "012345")


def default_out():
    if os.environ.get("SAKATSU_GRAMMAR_LISTS"):
        return pathlib.Path(os.environ["SAKATSU_GRAMMAR_LISTS"])
    if os.environ.get("SAKATSU_DICT_DIR"):
        return pathlib.Path(os.environ["SAKATSU_DICT_DIR"]).parent / "corpora" / "grammar-lists"
    return pathlib.Path(os.environ["APPDATA"]) / "com.sakatsu.app" / "corpora" / "grammar-lists"


class Fetcher:
    """One request every `DELAY` seconds, and none at all for a page already cached."""

    def __init__(self, cache, download):
        self.cache = cache
        self.download = download
        self.last = 0.0
        self.fetched = 0
        cache.mkdir(parents=True, exist_ok=True)

    def get(self, url):
        path = self.cache / (hashlib.sha256(url.encode()).hexdigest()[:24] + ".html")
        if path.is_file():
            return path.read_text(encoding="utf-8")
        if not self.download:
            sys.exit(f"{url} is not cached; rerun with --download to fetch it")
        wait = self.last + DELAY - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        request = urllib.request.Request(url, headers={"User-Agent": AGENT})
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                text = response.read().decode("utf-8", errors="replace")
        except urllib.error.HTTPError as error:
            text = "" if error.code == 404 else sys.exit(f"{url}: HTTP {error.code}")
        self.last = time.monotonic()
        self.fetched += 1
        path.write_text(text, encoding="utf-8")
        return text


def clean(text):
    return " ".join(html.unescape(re.sub(r"<[^>]+>", "", text)).split())


def edewakaru_titles(page):
    """(address, title) for every post listed on one category page."""
    return re.findall(
        r'class="article-title[^"]*"[^>]*>\s*<a href="(https://www\.edewakaru\.com/archives/\d+\.html)"[^>]*>(.*?)</a>',
        page, flags=re.S)


def split_title(title):
    """`〜し〜し①（並列）｜日本語能力試験　JLPT N4文法` → name, stated levels."""
    name, _, rest = title.partition("｜")
    levels = sorted({f"N{n}" for n in re.findall(r"N\s*([1-5])", rest.translate(FULLWIDTH))})
    return name.strip(), levels


def edewakaru(fetch):
    rows = []
    for category, (kind, span) in EDEWAKARU_CATEGORIES.items():
        seen = set()
        for number in range(1, 80):
            url = EDEWAKARU.format(category) + ("" if number == 1 else f"?p={number}")
            fresh = [(u, clean(t)) for u, t in edewakaru_titles(fetch.get(url)) if u not in seen]
            if not fresh:
                break
            for address, title in fresh:
                seen.add(address)
                name, stated = split_title(title)
                if stated:
                    level = stated[0] if len(stated) == 1 else f"{stated[-1]}-{stated[0]}"
                    source = "title"
                else:
                    level, source = span, ("category" if span else "")
                members = "／".join(re.findall(r"「([^」]+)」", title)) if kind == "compare" else ""
                rows.append(("edewakaru", kind, name, "", level, source, address, members))
        print(f"  edewakaru {category} ({kind}): {len(seen)} posts")
    return rows


def jlpt_grammar_list(fetch):
    page = fetch.get(JLPT_GRAMMAR_LIST)
    rows = []
    for level, body in re.findall(r'<div class="grammar-list n([1-5])">(.*?)(?=<div class="grammar-list n|\Z)', page, flags=re.S):
        for item in re.findall(r'<div class="item">(.*?)(?=<div class="item">|\Z)', body, flags=re.S):
            term = re.search(r'<span class="term">(.*?)</span>', item, flags=re.S)
            if not term:
                continue
            sense = re.search(r"<sup>\s*(\d+)\s*</sup>", term.group(1))
            name = clean(re.sub(r"<sup>.*?</sup>", "", term.group(1)))
            # The gloss is the bare text between the term and the example; the example itself and
            # its audio are deliberately not read.
            after = item[term.end():]
            gloss = clean(re.split(r'<div class="japanese-sentence">', after)[0])
            rows.append(("jlptgrammarlist", "level", name, sense.group(1) if sense else "",
                         f"N{level}", "section", JLPT_GRAMMAR_LIST, gloss))
    print(f"  jlptgrammarlist: {len(rows)} points")
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--download", action="store_true", help="fetch pages not yet cached")
    parser.add_argument("--out", type=pathlib.Path, default=None)
    args = parser.parse_args()
    out = args.out or default_out()
    fetch = Fetcher(out / "cache", args.download)

    rows = edewakaru(fetch) + jlpt_grammar_list(fetch)
    header = "source\tlist\tname\tsense\tlevel\tlevel_from\turl\tnote\n"
    (out / "grammar-lists.tsv").write_text(
        header + "".join("\t".join(r) + "\n" for r in rows), encoding="utf-8", newline="\n")
    (out / "SOURCES.md").write_text(
        f"# Grammar lists — evaluation only\n\nRead {datetime.date.today().isoformat()} by "
        "`scripts/fetch-grammar-lists.py`.\n\n"
        "- 絵でわかる日本語, https://www.edewakaru.com/ — post titles and addresses only.\n"
        "- JLPT Grammar List, https://jlptgrammarlist.neocities.org/ — names, glosses and levels only.\n\n"
        "Neither declares a licence. Consult, never import (`project notes` §8): nothing here is committed\n"
        "or shipped.\n", encoding="utf-8", newline="\n")
    print(f"{len(rows)} rows → {out / 'grammar-lists.tsv'} ({fetch.fetched} pages fetched, rest cached)")


if __name__ == "__main__":
    main()
