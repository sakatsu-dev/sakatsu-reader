"""Cache reading exemplars from Aozora's explicit ruby, never from a guessed pronunciation.

    python scripts/harvest-aozora-readings.py --download

The default output is gitignored scratch data. Only original Japanese works marked out of
copyright (including every credited person) and written in modern kana are eligible. Source
archives and full catalog records stay beside the derived TSV, with hashes and a record of the
transformation. Aozora's catalog is CC BY 4.0; its work files have separate terms:
https://www.aozora.gr.jp/guide/kijyunn.html . No KWDLC data enters this bank.

This is literary retrieval evidence, not reading gold for manga. The spelling whitelist comes
from the committed heteronym fixture and the same frequency-ranked blind spots as the existing
harvester. Neither query sentences nor their correct readings select the books or exemplars.
"""

import argparse
import collections
import csv
import hashlib
import importlib.util
import io
import json
import pathlib
import re
import time
import urllib.parse
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
INDEX = "https://www.aozora.gr.jp/index_pages/list_person_all_extended_utf8.zip"
RUBY = re.compile(r"｜([^｜《》\r\n]+)《([ぁ-ゖァ-ヺー]+)》|([一-龯々〆ヶ]+)《([ぁ-ゖァ-ヺー]+)》")
ANNOTATION = re.compile(r"［＃[^］]*］")


def hiragana(text):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in text)


def spellings():
    fixture = (ROOT / "core/tests/common/heteronyms.rs").read_text(encoding="utf-8")
    wanted = set(re.findall(r'term: "([^"\n]+)"', fixture))
    spec = importlib.util.spec_from_file_location("sentence_harvest", ROOT / "scripts/harvest-sentences.py")
    harvest = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(harvest)
    # Reuse the existing entry grouping and ranking; keep the same blind-spot population.
    forms = harvest.heteronym_entries()
    ranks = harvest.frequency_ranks()
    gaps = []
    for term, sequences in forms.items():
        readings = {slot["reading"] for slot in sequences.values() if slot["reading"]}
        if term not in wanted and term in ranks and len(readings) > 1:
            gaps.append((ranks[term], term))
    wanted.update(term for _, term in sorted(gaps)[:harvest.BLIND_SPOTS])
    return wanted


def extract(text, wanted):
    """Yield distinct (spelling, reading, plain sentence), including implicit kanji ruby.

    Remove annotations before finding ruby; exclude ambiguous multiple uses of one spelling
    within the same sentence. A ruby suffix never annotates the last character of a larger
    kanji run: the complete annotated base must itself be whitelisted.
    """
    # Aozora surrounds its notation guide with dashed rules, and the source credits begin here.
    pieces = re.split(r"(?m)^-{20,}\s*$", text, maxsplit=2)
    if len(pieces) == 3:
        text = pieces[2]
    text = re.split(r"(?m)^底本：", text, maxsplit=1)[0]
    text = ANNOTATION.sub("", text)
    for raw in re.findall(r"[^。！？!?\r\n]+[。！？!?]?[」』）]*", text):
        found = [(m.group(1) or m.group(3), hiragana(m.group(2) or m.group(4))) for m in RUBY.finditer(raw)]
        plain = " ".join(RUBY.sub(lambda m: m.group(1) or m.group(3), raw).replace("｜", "").split())
        if not 8 <= len(plain) <= 240 or any(c in plain for c in "《》※"):
            continue
        for surface, reading in sorted(set(found)):
            if surface not in wanted or plain.count(surface) != 1:
                continue
            yield surface, reading, plain


def eligible(rows):
    by_work = collections.defaultdict(list)
    for row in rows:
        by_work[row["作品ID"]].append(row)
    works = []
    for group in by_work.values():
        row = group[0]
        if any(r["作品著作権フラグ"] != "なし" or r["人物著作権フラグ"] != "なし" or r["役割フラグ"] != "著者" for r in group):
            continue
        if row["文字遣い種別"] != "新字新仮名" or not row["分類番号"].startswith(("NDC 913", "NDC 914")):
            continue
        if row["テキストファイルURL"]:
            works.append(group)
    # An entry-id hash avoids alphabetical author bias and stays stable across machines.
    return sorted(works, key=lambda g: hashlib.sha256(g[0]["作品ID"].encode()).hexdigest())


def fetch(url, path, download):
    if path.exists():
        return path.read_bytes()
    if not download:
        raise FileNotFoundError(f"{path}: use --download to fetch explicitly")
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme not in {"http", "https"} or parsed.hostname != "www.aozora.gr.jp":
        raise ValueError(f"not an Aozora URL: {url}")
    url = urllib.parse.urlunsplit(("https", parsed.netloc, parsed.path, parsed.query, ""))
    request = urllib.request.Request(url, headers={"User-Agent": "Sakatsu-reading-evaluation/1.0"})
    with urllib.request.urlopen(request, timeout=30) as response:
        data = response.read(8 * 1024 * 1024 + 1)
    if len(data) > 8 * 1024 * 1024:
        raise ValueError("one archive exceeds the 8 MiB transfer limit")
    temporary = path.with_suffix(".part")
    temporary.write_bytes(data)
    temporary.replace(path)
    time.sleep(0.25)
    return data


def parse_args(argv=None):
    """Every bound this harvest runs under, in one place so a test can reach them.

    Separated from `main` because the limits are the whole experimental variable: the first run
    took 300 works and the classifier it fed could not decide anything, blocked on coverage rather
    than on being wrong. A widened run has to say what it widened, and the provenance records it.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--download", action="store_true")
    parser.add_argument("--cache", type=pathlib.Path, default=ROOT / "scratch/context-plan/aozora")
    parser.add_argument("--index", type=pathlib.Path, default=ROOT / "scratch/context-plan/aozora-index.zip")
    parser.add_argument("--max-books", type=int, default=300)
    parser.add_argument("--per-reading", type=int, default=24)
    parser.add_argument("--per-book", type=int, default=3)
    # The transfer budget was an implicit constant, and it raises rather than being caught — so a
    # wider run would abort with no TSV written. Making it an argument keeps the guard while
    # forcing a run that wants more to say so, and to record how much in the provenance.
    parser.add_argument("--max-bytes", type=int, default=100 * 1024 * 1024)
    args = parser.parse_args(argv)
    if min(args.max_books, args.per_reading, args.per_book, args.max_bytes) < 1:
        parser.error("limits must be positive")
    return args


def main():
    args = parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    args.index.parent.mkdir(parents=True, exist_ok=True)
    raw = fetch(INDEX, args.index, args.download)
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        rows = list(csv.DictReader(io.StringIO(archive.read(archive.namelist()[0]).decode("utf-8-sig"))))
    wanted = spellings()
    bank = collections.defaultdict(list)
    seen = set()
    manifest = []
    failed = []
    byte_count = 0
    for group in eligible(rows)[:args.max_books]:
        row = group[0]
        work = row["作品ID"]
        try:
            raw = fetch(row["テキストファイルURL"], args.cache / f"{work}.zip", args.download)
            byte_count += len(raw)
            if byte_count > args.max_bytes:
                raise RuntimeError(f"{args.max_bytes} byte archive budget exceeded")
            with zipfile.ZipFile(io.BytesIO(raw)) as archive:
                texts = [n for n in archive.namelist() if n.lower().endswith(".txt")]
                if len(texts) != 1 or archive.getinfo(texts[0]).file_size > 16 * 1024 * 1024:
                    raise ValueError("expected one bounded text member")
                text = archive.read(texts[0]).decode("cp932")
            book_counts = collections.Counter()
            for surface, reading, sentence in extract(text, wanted):
                key = surface, reading
                identity = surface, reading, sentence
                if identity in seen or len(bank[key]) >= args.per_reading or book_counts[key] >= args.per_book:
                    continue
                seen.add(identity)
                book_counts[key] += 1
                bank[key].append((surface, reading, sentence, work, row["図書カードURL"]))
            manifest.append({"records": group, "sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw)})
        except (OSError, UnicodeError, ValueError, zipfile.BadZipFile) as error:
            failed.append({"work": work, "error": str(error)})
        if (len(manifest) + len(failed)) % 25 == 0:
            print(f"books {len(manifest)}, failed {len(failed)}, exemplars {sum(map(len, bank.values()))}", flush=True)
    output = args.cache / "readings.tsv"
    with output.open("w", encoding="utf-8", newline="") as stream:
        # Every field is already one line with no tabs; quotes are Japanese text, not TSV syntax.
        writer = csv.writer(stream, delimiter="\t", lineterminator="\n", quoting=csv.QUOTE_NONE, quotechar=None)
        writer.writerow(["surface", "reading", "sentence", "work_id", "source"])
        writer.writerows(row for key in sorted(bank) for row in bank[key])
    provenance = {"catalog": INDEX, "catalog_sha256": hashlib.sha256(args.index.read_bytes()).hexdigest(),
                  "spelling_whitelist": sorted(wanted), "tsv_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                  "terms": "https://www.aozora.gr.jp/guide/kijyunn.html", "catalog_license": "CC BY 4.0",
                  "transformation": "explicit ruby; annotations removed; modern-kana original Japanese; complete base only; one occurrence; bounded per reading and work",
                  "limits": {"books": args.max_books, "per_reading": args.per_reading, "per_book": args.per_book,
                             "max_bytes": args.max_bytes},
                  "works": manifest, "failures": failed}
    output.with_suffix(".provenance.json").write_text(json.dumps(provenance, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"{len(manifest)} books; {len(failed)} failures; {len(bank)} reading pairs; {sum(map(len, bank.values()))} exemplars; {byte_count} archive bytes", flush=True)
    if not bank:
        raise SystemExit("No exemplars collected; this is not a usable evaluation bank.")


if __name__ == "__main__":
    main()
