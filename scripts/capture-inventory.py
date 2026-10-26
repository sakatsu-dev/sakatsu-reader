"""Inventory the capture corpus by stem and tier, and scan it for colloquial candidates.

    python scripts/capture-inventory.py
    python scripts/capture-inventory.py --json scratch/colloquial/capture-inventory.json

**Read-only.** It never renames a file, promotes a draft, or writes anything inside `corpus/`.

# Why a tier-aware inventory rather than a glob

One capture has up to four text files beside its image, and they are not interchangeable:

- `<stem>.txt` — a transcript a person reviewed and promoted by renaming.
- `<stem>.txt.mine` — the same transcript before anyone reviewed it.
- `<stem>.txt.suggested` — what the OCR engines said. `corpus_bootstrap.rs` writes it and names
  the engine behind each reading. **Never a transcript**: mining it for colloquial expressions
  turns an OCR error into a linguistic finding.
- `<stem>.tok.mine` — draft segmentation, `|`-separated. Not a transcript either.

`*.txt` therefore counts a capture more than once, and `split('.')` cannot tell the tiers apart.
Suffixes are removed longest-first instead, and every count is reported **by tier**, because a
finding in an unreviewed draft is weaker evidence than one in a promoted transcript and the
difference has to survive into the report.

# What is not a capture

`corpus/` holds three files that are not captures at all (`RESULTS.md`, `REVIEW.md`,
`reading-gold.json`), and two `.txt` files with no image beside them. The first three are excluded
by name; the rest is structural — a capture is a stem that has an image. Text-only stems are
reported rather than dropped, so the exclusion is visible instead of silent.
"""

import argparse
import collections
import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Longest first: `.txt.mine` must be tried before `.txt`, or every draft reads as a transcript.
TIERS = (".txt.suggested", ".txt.mine", ".tok.mine", ".txt", ".png", ".jpg", ".jpeg", ".webp")
IMAGE_TIERS = (".png", ".jpg", ".jpeg", ".webp")
# Transcript tiers, in the order they are preferred.
TRANSCRIPT_TIERS = (".txt", ".txt.mine")
NOT_CAPTURES = frozenset({"RESULTS.md", "REVIEW.md", "reading-gold.json"})

# Retrieval patterns, not linguistic labels. Each *may* indicate the family; a reviewer establishes
# the construction from context. Overlaps between families are deliberate and reported as such.
FAMILIES = {
    "explanatory": ["なん", "んだ", "んです", "んじゃ", "のだ", "んで"],
    "sentence-ending": ["じゃん", "っけ", "じゃね", "よね", "かな", "ぜ", "ぞ"],
    "contraction": ["ちゃう", "じゃう", "ちゃっ", "じゃっ", "とく", "どく", "てる", "でる", "てん", "でん"],
    "quotation": ["って", "ってば", "つって", "とか"],
    "negation-obligation": ["なきゃ", "なくちゃ", "ねえ", "ねー", "ねぇ"],
    "regional": ["やん", "へん", "ねん", "やで", "せや", "はる"],
}


def split_name(name):
    """`("firefox_X", ".txt.mine")` for a capture file, or `None` when it is not one.

    Longest-suffix-first, so `.txt.mine` never reads as `.txt` with a stem ending in `.mine`.
    """
    if name in NOT_CAPTURES:
        return None
    for tier in TIERS:
        if name.endswith(tier) and len(name) > len(tier):
            return name[: -len(tier)], tier
    return None


def source_app(stem):
    """The capturing application, from the stem.

    Stems are `<app>_<id>` and **the app may itself contain underscores** — `nexon_client_…` — so
    this splits on the last one. Splitting on the first would file nexon apart from its own
    siblings.
    """
    app, _, _ = stem.rpartition("_")
    return app or stem


def capture_id(root, path, stem):
    """**Relative directory plus stem**, which is the identity a capture actually has.

    Keying on the stem alone merges two captures that share a name in different folders — and the
    corpus is a directory somebody drops screenshots into, so that collision is a matter of time
    rather than of possibility. A top-level capture keeps its bare stem, so existing ids are
    unchanged.
    """
    relative = path.parent.relative_to(root)
    if relative == pathlib.Path("."):
        return stem
    return f"{relative.as_posix()}/{stem}"


def inventory(directory):
    """Every capture in `directory` **and below it**, with its tiers, plus what was ignored.

    Recursive: a corpus that grows tends to grow into folders, and a scanner that reads only the
    top level reports a shrinking corpus as a stable one.
    """
    root = pathlib.Path(directory)
    captures = collections.defaultdict(dict)
    ignored = []
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        split = split_name(path.name)
        if split is None:
            ignored.append(path.relative_to(root).as_posix())
            continue
        stem, tier = split
        captures[capture_id(root, path, stem)][tier] = path
    return dict(captures), ignored


def select_transcript(tiers):
    """Which tier to read the transcript from, or `None` when there is none.

    `.txt.suggested` is never selected: it is machine output, and `corpus_bootstrap.rs` only ever
    writes a *suggestion* — the rename to `.txt` is asserted elsewhere to be a human act.
    """
    for tier in TRANSCRIPT_TIERS:
        if tier in tiers:
            return tier
    return None


# `corpus_bootstrap.rs::suggestion` marks an engine that produced nothing with this suffix.
NO_READING = ": no reading"


def parse_suggested(text):
    """The engine readings in a `.txt.suggested`, as `{engine: text_or_None}`.

    The format is `corpus_bootstrap.rs::suggestion`'s, and parsing it by shape rather than by
    guesswork matters in three ways this originally got wrong:

    - **A reading may be several lines.** The writer emits `text.trim()`, and a capture with a
      speaker's name above their line comes back as three. Keeping only the first line silently
      truncated the OCR output for every multi-line capture.
    - **An engine with no output says so**, as `# <name>: no reading`, and read as an instruction
      comment it vanished entirely. It is recorded because it is information — but **it does not
      say that the engine ran.** `corpus_bootstrap.rs::read_with` returns nothing both when the
      engine is unavailable, in which case it never ran, and when recognition failed. The marker
      means *no output available, reason not encoded in this file*, and nothing more specific may
      be inferred from it.
    - **The instruction preamble is one line or two**, depending on whether the engines agreed, so
      nothing may be skipped by position.

    A heading with an empty block and no `: no reading` suffix is an instruction, and is dropped.
    """
    readings = {}
    heading = None
    block = []

    def close():
        if heading is not None and block:
            readings[heading] = "\n".join(block).strip()

    for line in text.splitlines():
        if line.startswith("#"):
            close()
            label = line.lstrip("#").strip()
            block = []
            if label.endswith(NO_READING):
                readings[label[: -len(NO_READING)].strip()] = None
                heading = None
            else:
                heading = label
            continue
        if heading is not None:
            block.append(line)
    close()
    return readings


# A `.tok.mine` writes a line break in the capture as its own token.
LINE_BREAK_TOKEN = "/"


def segments_of(text):
    """The tokens of a `.tok.mine`, which separates them with `|`.

    Whitespace-only tokens are **kept**: a space inside a captured line is part of the text, and
    dropping it here would make a faithful segmentation look like a mismatch.
    """
    return [segment for segment in text.strip().split("|") if segment]


def segmentation_matches(segments, transcript):
    """Whether a draft segmentation reassembles its transcript exactly.

    The one convention it has to know is [`LINE_BREAK_TOKEN`]: a multi-line capture — a speaker
    name above their line, most often — is segmented with `/` standing for the break. Reassembling
    without that reads four of the seventeen as corrupt when they are faithful.

    Everything else is compared exactly and any difference is **reported rather than normalised
    away**: it means the segmentation and the transcript disagree about what was captured, and
    which of them is wrong is a question for a person.
    """
    rebuilt = "".join(
        "\n" if segment == LINE_BREAK_TOKEN else segment for segment in segments
    )
    return rebuilt.strip() == transcript.strip()


def scan(text, families=FAMILIES):
    """Candidate-pattern occurrences in one text, as `{family: {pattern: count}}`."""
    found = {}
    for family, patterns in families.items():
        hits = {}
        for pattern in patterns:
            count = text.count(pattern)
            if count:
                hits[pattern] = count
        if hits:
            found[family] = hits
    return found


def digest_of(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build(directory):
    """The whole inventory: per capture, its tiers, selected transcript, scan and warnings."""
    found, ignored = inventory(directory)
    captures = []
    warnings = []
    for identity in sorted(found):
        tiers = found[identity]
        # The id may carry a relative directory; the application is read from the stem alone.
        stem = identity.rsplit("/", 1)[-1]
        image = next((tiers[tier] for tier in IMAGE_TIERS if tier in tiers), None)
        if image is None:
            warnings.append(f"{identity}: text tiers {sorted(tiers)} but no image — not a capture")
            continue
        selected_tier = select_transcript(tiers)
        record = {
            "stem": identity,
            "source_app": source_app(stem),
            "tiers": sorted(tiers),
            "image_sha256": digest_of(image),
            "transcript_tier": selected_tier,
        }
        if selected_tier is None:
            record["status"] = "pending transcription"
            warnings.append(f"{identity}: image with no transcript at any tier")
        else:
            text = tiers[selected_tier].read_text(encoding="utf-8").strip()
            record["status"] = "promoted" if selected_tier == ".txt" else "draft"
            record["text"] = text
            record["text_sha256"] = hashlib.sha256(text.encode("utf-8")).hexdigest()
            record["characters"] = len(text)
            record["candidates"] = scan(text)
        if ".txt.suggested" in tiers:
            record["engines"] = parse_suggested(
                tiers[".txt.suggested"].read_text(encoding="utf-8")
            )
        if ".tok.mine" in tiers:
            segments = segments_of(tiers[".tok.mine"].read_text(encoding="utf-8"))
            record["segments"] = len(segments)
            if selected_tier is not None:
                agreed = segmentation_matches(segments, record["text"])
                record["segmentation_matches_text"] = agreed
                if not agreed:
                    warnings.append(
                        f"{identity}: .tok.mine segments do not reassemble the {selected_tier} text"
                    )
        captures.append(record)
    return {"captures": captures, "ignored_files": ignored, "warnings": warnings}


def duplicates(captures):
    """Captures sharing identical text, and captures sharing an identical image.

    Kept as groups rather than resolved: every source association survives, and a later sampler
    needs to know which captures move together so duplicates cannot straddle a partition.
    """
    by_text = collections.defaultdict(list)
    by_image = collections.defaultdict(list)
    for record in captures:
        if "text_sha256" in record:
            by_text[record["text_sha256"]].append(record["stem"])
        by_image[record["image_sha256"]].append(record["stem"])
    return {
        "text": {key: stems for key, stems in by_text.items() if len(stems) > 1},
        "image": {key: stems for key, stems in by_image.items() if len(stems) > 1},
    }


def report(built, out=sys.stdout):
    captures = built["captures"]
    by_status = collections.Counter(record["status"] for record in captures)
    by_app = collections.Counter(record["source_app"] for record in captures)

    print("=== capture inventory ===", file=out)
    print(f"  captures (stem with an image)  {len(captures)}", file=out)
    for status, count in sorted(by_status.items()):
        print(f"    {status:24} {count}", file=out)
    print(f"  files ignored as non-captures  {len(built['ignored_files'])}", file=out)
    for name in built["ignored_files"]:
        print(f"    {name}", file=out)

    print("\n  by capturing application, promoted / total", file=out)
    for app, total in by_app.most_common():
        promoted = sum(
            1 for r in captures if r["source_app"] == app and r["status"] == "promoted"
        )
        print(f"    {app:24} {promoted:4} / {total}", file=out)

    groups = duplicates(captures)
    print(
        f"\n  duplicate text groups {len(groups['text'])}"
        f"   duplicate image groups {len(groups['image'])}",
        file=out,
    )

    print("\n=== candidate density, by capturing application ===", file=out)
    print("  **The application is not the register.** A browser or image-viewer capture can hold", file=out)
    print("  manga dialogue; the column says which program was in front, and nothing more. Rates", file=out)
    print("  are given because a raw count over seven captures is not comparable with one over", file=out)
    print("  ninety-two.\n", file=out)
    print(
        f"  {'application':20} {'caps':>5} {'with hits':>10} {'rate':>7}"
        f" {'chars':>7} {'hits':>6} {'per 1k chars':>13}",
        file=out,
    )
    for app in sorted({record["source_app"] for record in captures}):
        rows = [record for record in captures if record["source_app"] == app]
        scored = [record for record in rows if "text" in record]
        with_hits = sum(1 for record in scored if record.get("candidates"))
        chars = sum(record["characters"] for record in scored)
        hits = sum(
            sum(found.values())
            for record in scored
            for found in record.get("candidates", {}).values()
        )
        rate = f"{with_hits / len(scored):.0%}" if scored else "-"
        per_k = f"{hits * 1000 / chars:.1f}" if chars else "-"
        print(
            f"  {app:20} {len(rows):>5} {with_hits:>10} {rate:>7}"
            f" {chars:>7} {hits:>6} {per_k:>13}",
            file=out,
        )

    print("\n=== candidate families, by transcript tier ===", file=out)
    print("  Retrieval patterns only. A count is not a construction, and an OCR error in a", file=out)
    print("  draft tier is indistinguishable here from a colloquial form.\n", file=out)
    header = f"  {'family':22} {'promoted hits':>14} {'promoted caps':>14} {'draft hits':>11} {'draft caps':>11}"
    print(header, file=out)
    for family in FAMILIES:
        row = {}
        for tier in ("promoted", "draft"):
            hits = 0
            caps = 0
            for record in captures:
                found = record.get("candidates", {}).get(family)
                if found and record["status"] == tier:
                    hits += sum(found.values())
                    caps += 1
            row[tier] = (hits, caps)
        print(
            f"  {family:22} {row['promoted'][0]:>14} {row['promoted'][1]:>14}"
            f" {row['draft'][0]:>11} {row['draft'][1]:>11}",
            file=out,
        )

    print("\n  per pattern (promoted transcripts only)", file=out)
    per_pattern = collections.Counter()
    for record in captures:
        if record["status"] != "promoted":
            continue
        for hits in record.get("candidates", {}).values():
            per_pattern.update(hits)
    for pattern, count in per_pattern.most_common():
        print(f"    {pattern:10} {count}", file=out)

    if built["warnings"]:
        print(f"\n=== warnings ({len(built['warnings'])}) ===", file=out)
        for warning in built["warnings"]:
            print(f"  {warning}", file=out)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--corpus",
        default=str(ROOT / "corpus"),
        help="the capture directory (default: the repository's ignored corpus/)",
    )
    parser.add_argument("--json", help="also write the full inventory here")
    args = parser.parse_args(argv)

    directory = pathlib.Path(args.corpus)
    if not directory.is_dir():
        print(f"no capture directory at {directory}", file=sys.stderr)
        return 1

    built = build(directory)
    report(built)
    if args.json:
        out = pathlib.Path(args.json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(
            json.dumps(built, ensure_ascii=False, indent=2, sort_keys=True), encoding="utf-8"
        )
        print(f"\nwrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
