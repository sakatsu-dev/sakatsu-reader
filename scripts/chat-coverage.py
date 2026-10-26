"""Does the acquired chat data carry enough colloquial material to justify the pilot?

    python scripts/chat-coverage.py
    python scripts/chat-coverage.py --json scratch/colloquial/chat-coverage.json

Reads the normalized JSONL written by `fetch-chat-corpora.py`. Reuses `capture-inventory.py`'s
family patterns unchanged, so the chat numbers and the capture numbers are the same measurement on
different material and can be put beside each other.

**These are retrieval patterns, not linguistic labels.** Every count is *occurrences matched by the
current patterns*; linguistic precision and recall are unestablished. It is not an upper bound
either — the patterns have false positives (the capture batch measured 2 true of 6 on a selected
sample) **and** they miss expressions nobody thought to list. A family scoring zero has not been
shown absent; it has been shown unmatched by six strings.

# Concentration, not just frequency

A family that appears 500 times in one speaker's dialogues is a fact about that speaker. So every
family is reported with the number of distinct dialogues and distinct speakers it reaches, and the
share contributed by its single largest speaker — **of candidate-bearing utterances**, which is a
narrower claim than concentration of genuine constructions, since those are a subset nobody has
identified yet.

Totals are also broken down **by source and condition**, because the corpus is dominated by one of
the two sources while the pilot intends to sample them equally: a headline rate over the union
describes a sample nobody is going to draw.

Every per-source rate is reported **per utterance and per 1,000 characters together**, because a
per-utterance rate is partly a statement about utterance length. One source's turns here are about
twice as long as the other's, so a higher per-utterance rate is what a longer turn produces whether
or not the register differs at all. Both have to be read before either is called a difference of
register.
"""

import argparse
import collections
import importlib.util
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

_spec = importlib.util.spec_from_file_location(
    "capture_inventory", pathlib.Path(__file__).with_name("capture-inventory.py")
)
_inventory = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_inventory)
FAMILIES = _inventory.FAMILIES

#: Specific, shallow detectors — **named by what they match, not by what they mean**. They are
#: enough to show whether typed-chat artefacts are common, and not enough to characterise register:
#: the laughter pattern needs two or more Latin w and so misses a single trailing `w`; the
#: bracketed-run pattern only looks at the end of an utterance and so misses a kaomoji mid-sentence.
#: Deliberately not exhaustive — the question here is order of magnitude, not a taxonomy.
EMOJI = re.compile(
    "[\U0001F300-\U0001FAFF\U00002600-\U000027BF\U0001F1E6-\U0001F1FF️❤]"
)
KAOMOJI = re.compile(r"[(（][^)）]{0,12}[)）]\s*$")
LAUGHTER = re.compile(r"(?:ｗ{1,}|w{2,})$")
ASCII_RUN = re.compile(r"[A-Za-z]{3,}")


def utterances(path):
    for line in pathlib.Path(path).open(encoding="utf-8"):
        record = json.loads(line)
        for utterance in record["utterances"]:
            yield record, utterance


def scan(paths):
    """Family hits, concentration, and the chat-specific features, in one pass."""
    stats = {
        "utterances": 0,
        "characters": 0,
        "dialogues": set(),
        "speakers": set(),
        "by_source": collections.defaultdict(lambda: {"utterances": 0, "characters": 0}),
        "families": collections.defaultdict(
            lambda: {
                "hits": 0,
                "utterances": 0,
                "dialogues": set(),
                "speakers": collections.Counter(),
                "by_source": collections.Counter(),
                "by_condition": collections.Counter(),
            }
        ),
        "patterns": collections.Counter(),
        "noise": collections.Counter(),
        "lengths": [],
        "conditions": collections.Counter(),
    }
    for path in paths:
        for record, utterance in utterances(path):
            text = utterance["text"]
            speaker = f"{record['source']}:{utterance['speaker']}"
            stats["utterances"] += 1
            stats["characters"] += len(text)
            stats["dialogues"].add(f"{record['source']}:{record['dialogue_id']}")
            stats["speakers"].add(speaker)
            stats["lengths"].append(len(text))
            stats["conditions"][record.get("condition") or "(unstated)"] += 1

            if EMOJI.search(text):
                stats["noise"]["emoji codepoint"] += 1
            if KAOMOJI.search(text):
                stats["noise"]["utterance ends in (…)"] += 1
            if LAUGHTER.search(text):
                stats["noise"]["trailing ｗ or 2+ w"] += 1
            if ASCII_RUN.search(text):
                stats["noise"]["run of 3+ Latin letters"] += 1
            if len(text) <= 3:
                stats["noise"]["three characters or fewer"] += 1
            if not any(ord(c) > 0x2FFF for c in text):
                stats["noise"]["no CJK at all"] += 1

            stats["by_source"][record["source"]]["utterances"] += 1
            stats["by_source"][record["source"]]["characters"] += len(text)

            for family, patterns in FAMILIES.items():
                found = sum(text.count(pattern) for pattern in patterns)
                if not found:
                    continue
                bucket = stats["families"][family]
                bucket["hits"] += found
                bucket["utterances"] += 1
                bucket["dialogues"].add(f"{record['source']}:{record['dialogue_id']}")
                bucket["speakers"][speaker] += 1
                bucket["by_source"][record["source"]] += 1
                bucket["by_condition"][record.get("condition") or "(unstated)"] += 1
                for pattern in patterns:
                    count = text.count(pattern)
                    if count:
                        stats["patterns"][pattern] += count
    return stats


def report(stats, out=sys.stdout):
    total = stats["utterances"]
    chars = stats["characters"]
    lengths = sorted(stats["lengths"])
    median = lengths[len(lengths) // 2] if lengths else 0

    print("=== acquired chat corpus ===", file=out)
    print(f"  dialogues   {len(stats['dialogues']):>9,}", file=out)
    print(f"  utterances  {total:>9,}", file=out)
    print(f"  characters  {chars:>9,}", file=out)
    print(f"  utterance length: median {median}, max {lengths[-1] if lengths else 0}", file=out)
    print("  conditions:", dict(stats["conditions"].most_common()), file=out)

    print("\n=== candidate families ===", file=out)
    print("  Occurrences matched by the current retrieval patterns. Linguistic precision and", file=out)
    print("  recall are unestablished: these patterns have false positives and they miss", file=out)
    print("  expressions nobody listed, so a count is neither an estimate nor a bound.", file=out)
    print("  'top spkr' is the largest single speaker's share of *candidate-bearing utterances*.\n", file=out)
    print(
        f"  {'family':22} {'hits':>8} {'utts':>8} {'rate':>7} {'per 1k ch':>10}"
        f" {'dialogues':>10} {'speakers':>9} {'top spkr':>9}",
        file=out,
    )
    for family in FAMILIES:
        bucket = stats["families"].get(family)
        if not bucket:
            print(f"  {family:22} {0:>8} {0:>8} {'0%':>7} {'0.0':>10} {0:>10} {0:>9} {'-':>9}", file=out)
            continue
        top = bucket["speakers"].most_common(1)[0][1] if bucket["speakers"] else 0
        share = f"{top / bucket['utterances']:.0%}" if bucket["utterances"] else "-"
        print(
            f"  {family:22} {bucket['hits']:>8,} {bucket['utterances']:>8,}"
            f" {bucket['utterances'] / total:>6.1%} {bucket['hits'] * 1000 / chars:>10.1f}"
            f" {len(bucket['dialogues']):>10,} {len(bucket['speakers']):>9,} {share:>9}",
            file=out,
        )

    print("\n  per pattern", file=out)
    for pattern, count in stats["patterns"].most_common():
        print(f"    {pattern:10} {count:>8,}", file=out)

    print("\n=== by source ===", file=out)
    print("  The union is dominated by one corpus; the pilot samples both equally, so the", file=out)
    print("  headline rates above describe a sample nobody is going to draw.\n", file=out)
    for source, counts in sorted(stats["by_source"].items()):
        print(
            f"  {source:34} {counts['utterances']:>9,} utterances {counts['characters']:>11,} ch",
            file=out,
        )
    print("\n  Per utterance AND per 1,000 characters. A per-utterance rate is partly a", file=out)
    print("  statement about utterance length: a longer turn has more chances to match, so", file=out)
    print("  read both before calling any gap between the sources a difference of register.\n", file=out)
    print(f"  {'family':22} " + "  ".join(f"{s:>32}" for s in sorted(stats["by_source"])), file=out)
    for family in FAMILIES:
        bucket = stats["families"].get(family)
        cells = []
        for source in sorted(stats["by_source"]):
            hit = bucket["by_source"][source] if bucket else 0
            counts = stats["by_source"][source]
            per_utterance = hit / (counts["utterances"] or 1)
            per_1k = hit * 1000 / (counts["characters"] or 1)
            cells.append(f"{hit:>9,} {per_utterance:>6.1%}/utt {per_1k:>6.2f}/1k ch")
        print(f"  {family:22} " + "  ".join(cells), file=out)

    print(
        f"\n  {'mean utterance length':22} "
        + "  ".join(
            f"{stats['by_source'][s]['characters'] / (stats['by_source'][s]['utterances'] or 1):>29.1f} ch"
            for s in sorted(stats["by_source"])
        ),
        file=out,
    )

    print("\n=== detector counts ===", file=out)
    print("  Named by what each pattern matches, not by what it means. Shallow by design: the", file=out)
    print("  laughter pattern needs 2+ Latin w and misses a single trailing one; the bracketed", file=out)
    print("  pattern only looks at the end of an utterance. Order of magnitude, not a taxonomy.\n", file=out)
    for feature, count in stats["noise"].most_common():
        print(f"    {feature:28} {count:>8,}  {count / total:>6.1%}", file=out)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--chat", default=str(ROOT / "scratch" / "colloquial" / "chat"), help="normalized JSONL dir"
    )
    parser.add_argument("--json", help="also write the machine-readable summary here")
    args = parser.parse_args(argv)

    paths = sorted(pathlib.Path(args.chat).glob("*.jsonl"))
    if not paths:
        print(f"no normalized corpus in {args.chat}; run fetch-chat-corpora.py --download", file=sys.stderr)
        return 1
    stats = scan(paths)
    report(stats)

    if args.json:
        serialisable = {
            "utterances": stats["utterances"],
            "characters": stats["characters"],
            "dialogues": len(stats["dialogues"]),
            "speakers": len(stats["speakers"]),
            "conditions": dict(stats["conditions"]),
            "detector_counts": dict(stats["noise"]),
            "by_source": {k: dict(v) for k, v in stats["by_source"].items()},
            "patterns": dict(stats["patterns"]),
            "families": {
                family: {
                    "hits": bucket["hits"],
                    "utterances": bucket["utterances"],
                    "dialogues": len(bucket["dialogues"]),
                    "speakers": len(bucket["speakers"]),
                    "by_source": dict(bucket["by_source"]),
                    "by_source_rates": {
                        source: {
                            "candidate_bearing_utterances": count,
                            "per_utterance": count
                            / (stats["by_source"][source]["utterances"] or 1),
                            "per_1k_characters": count
                            * 1000
                            / (stats["by_source"][source]["characters"] or 1),
                        }
                        for source, count in bucket["by_source"].items()
                    },
                    "by_condition": dict(bucket["by_condition"]),
                    "largest_speaker_share_of_candidate_bearing_utterances": (
                        bucket["speakers"].most_common(1)[0][1] / bucket["utterances"]
                        if bucket["utterances"]
                        else 0
                    ),
                }
                for family, bucket in stats["families"].items()
            },
        }
        path = pathlib.Path(args.json)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(serialisable, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"\nwrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
