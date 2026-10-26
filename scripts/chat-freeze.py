"""Freeze source identities, partitions, duplicate exclusions and the development sample.

    python scripts/chat-freeze.py --write     # produce the freeze
    python scripts/chat-freeze.py --verify    # confirm the freeze still describes the data

Everything the engine comparison is allowed to depend on is decided **here, before any engine
runs**, and recorded with a digest. The reason is narrow and specific: if the partition, the
duplicate exclusions or the sampling allocations can still move after the first failures are seen,
then every later number is a measurement of choices made while looking at the answer.

So `--write` refuses to overwrite an existing freeze. Re-freezing is a deliberate act with a new
file, not a silent consequence of running a script again.

# What is frozen

**Source identities.** Pinned revision, archive digest and size, and the digest of the normalized
JSONL actually read — so a re-download that differs is caught rather than absorbed.

**Partition.** Dialogue-level, deterministic, 20% held out per source. It is dialogue-level because
the speaker graph leaves no alternative in the requested range (`chat-partition.py`): one component
holds 93.4% of dialogues. **Speaker overlap between the partitions is therefore expected, and is
measured and recorded rather than implied away.** Nothing built on this split may be described as
speaker-independent generalization.

A whole-component holdout does exist at 6.6%, and is recorded as available and deliberately not
taken: the components coincide exactly with the two sources, so holding one out measures
generalization to different source material — a different question from the one asked here.

**Duplicate exclusions.** Two granularities, because one number would be wrong for both jobs:

- *dialogues* with an identical utterance sequence are pinned to the same side of the partition, so
  a conversation cannot appear on both;
- *utterance texts* are deduplicated within the sample, so a sampled greeting is not sampled twice.

Utterance-level duplication is **not** used for partitioning. 12.6% of utterances share their text
with another, almost all greetings, and pinning on that basis would collapse the corpus into one
partition while preventing no leakage worth preventing.

**Sampling assignments.** 240 utterances, 120 per source, drawn only from the development side.

# Sampling, and what the allocation means

Per source: 80 weighted to candidate families, 40 drawn independently of the patterns.

The family 80 is weighted by **capture attestation** — the count of promoted captures carrying each
family in the capture coverage measurement — because the captures are the domain the product is
measured on, while the chat corpora are where examples are plentiful. Apportionment is by largest
remainder with a floor, so a family the captures barely show is thinned, never dropped.

**Regional gets a fixed discovery allocation**, not a weighted one. Its capture attestation is zero
and its chat match rate is two orders of magnitude below the rest, so any weighting drops it — and
"we found no dialect because we did not look" is not a finding. The allocation is there to discover
whether the candidates survive review, and it is recorded in the freeze so it cannot quietly shrink
once the engine results are unappealing.

The independent 40 are sampled with no reference to the patterns at all. They are what keeps
discovery from being confined to expressions somebody already listed; how many of them happen to
match a pattern is reported as a diagnostic and never used to resize anything.

**The two sources are sampled equally and reported separately.** The pool is 80% RealPersonaChat and
the sample is 50%, so a combined figure over this sample is an equally weighted diagnostic, not an
estimate of the pooled corpus.

# Determinism

No RNG. Ordering is by SHA-256 over a fixed salt and a stable identity, so the result does not
depend on the Python version, dict ordering, or the order files are read in. Re-running `--write`
against an empty directory reproduces the artifacts byte for byte, and a test asserts it.
"""

import argparse
import collections
import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: Changing this re-randomizes every assignment. It is part of the frozen record.
SALT = "sakatsu-colloquial-pilot-2026-09-13"

#: Share of dialogues held out, per source.
HOLDOUT = 0.20

#: Utterances per source, and how they divide.
PER_SOURCE = 120
FAMILY_BUDGET = 80
INDEPENDENT_BUDGET = 40

#: Reserved for regional discovery — fixed, not weighted. See the module docstring.
REGIONAL_DISCOVERY = 8

#: Every family gets at least this many, so capture weighting thins rather than drops.
FAMILY_FLOOR = 6

#: Promoted captures carrying each family, measured in the capture coverage sweep. These are
#: retrieval matches over 141 promoted transcripts, not construction counts.
CAPTURE_ATTESTATION = {
    "quotation": 29,
    "explanatory": 27,
    "contraction": 9,
    "sentence-ending": 7,
    "negation-obligation": 2,
    "regional": 0,
}

#: Longer utterances are excluded, never truncated: a truncated utterance is a different sentence
#: and would be scored as if it were the original. Measured at zero exclusions on this corpus.
MAX_SCALARS = 200

#: Turns of context retained around each sampled utterance.
CONTEXT_BEFORE = 2
CONTEXT_AFTER = 1


def _families():
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "capture_inventory", pathlib.Path(__file__).with_name("capture-inventory.py")
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.FAMILIES


def digest(data):
    return hashlib.sha256(data).hexdigest()


def stable(*parts):
    """A deterministic 64-bit ordering key over a salted identity."""
    joined = "\x1f".join(str(part) for part in parts)
    return int(digest(f"{SALT}\x1f{joined}".encode("utf-8"))[:16], 16)


def fraction(*parts):
    """`stable` as a value in [0, 1), for threshold assignment."""
    return stable(*parts) / float(1 << 64)


def allocate(weights, total, floor, fixed=None):
    """Largest-remainder apportionment of `total`, with a floor and fixed reservations.

    Fixed allocations are taken off the top and are not weighted — that is the whole point of them.
    Every remaining key then receives `floor`, and only what is left over is apportioned by weight.
    So a key whose weight is zero still receives its floor, and a dominant weight cannot starve the
    others: weighting decides emphasis, not survival.
    """
    fixed = dict(fixed or {})
    allocation = dict(fixed)
    keys = [key for key in weights if key not in fixed]
    remaining = total - sum(fixed.values())
    if remaining < floor * len(keys):
        raise ValueError(f"{total} cannot give {floor} to each of {len(keys)} keys after reservations")

    for key in keys:
        allocation[key] = floor
    remaining -= floor * len(keys)

    mass = sum(weights[key] for key in keys)
    if mass <= 0:
        # No weight anywhere: spread the remainder evenly, lowest key name first.
        for index, key in enumerate(sorted(keys)):
            allocation[key] += remaining // len(keys) + (1 if index < remaining % len(keys) else 0)
        return allocation

    exact = {key: weights[key] * remaining / mass for key in keys}
    for key in keys:
        allocation[key] += int(exact[key])
    left = remaining - sum(int(exact[key]) for key in keys)
    # Largest fractional part wins; ties break on the key name, so the result never depends on
    # dictionary order.
    order = sorted(keys, key=lambda key: (-(exact[key] - int(exact[key])), key))
    for key in order[:left]:
        allocation[key] += 1
    return allocation


def read_corpus(chat_dir):
    """Dialogues with their utterances, plus the digest of each file actually read."""
    dialogues, files = [], {}
    for path in sorted(pathlib.Path(chat_dir).glob("*.jsonl")):
        raw = path.read_bytes()
        files[path.name] = {"sha256": digest(raw), "bytes": len(raw)}
        for line in raw.decode("utf-8").splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            record["key"] = f"{record['source']}:{record['dialogue_id']}"
            dialogues.append(record)
    return dialogues, files


def duplicate_dialogues(dialogues):
    """Dialogues whose utterance sequence is identical, grouped. Pinned to one partition side."""
    by_content = collections.defaultdict(list)
    for dialogue in dialogues:
        content = digest("\x1e".join(u["text"] for u in dialogue["utterances"]).encode("utf-8"))
        by_content[content].append(dialogue["key"])
    return {
        content: sorted(keys) for content, keys in by_content.items() if len(keys) > 1
    }


def partition(dialogues, duplicates):
    """Deterministic dialogue-level split, with duplicate groups pinned together.

    Assignment is by a threshold on a stable hash rather than by taking the first N of a shuffle,
    so adding a dialogue moves only that dialogue.
    """
    pinned = {}
    for keys in duplicates.values():
        representative = min(keys)
        for key in keys:
            pinned[key] = representative

    assignment = {}
    for dialogue in dialogues:
        key = dialogue["key"]
        decider = pinned.get(key, key)
        assignment[key] = "holdout" if fraction("partition", decider) < HOLDOUT else "development"
    return assignment


def speaker_overlap(dialogues, assignment):
    """How many speakers appear on both sides. Dialogue-level splitting guarantees some do."""
    sides = collections.defaultdict(set)
    for dialogue in dialogues:
        speakers = set(dialogue.get("speakers") or [])
        speakers |= {u["speaker"] for u in dialogue["utterances"] if u.get("speaker")}
        for speaker in speakers:
            sides[f"{dialogue['source']}:{speaker}"].add(assignment[dialogue["key"]])
    both = sum(1 for s in sides.values() if len(s) > 1)
    return {"speakers": len(sides), "in_both_partitions": both}


def candidates(text, families):
    return sorted(f for f, patterns in families.items() if any(p in text for p in patterns))


def sample(dialogues, assignment, families, allocation):
    """The frozen development sample: family-weighted, then independent, per source.

    Families are filled **scarcest pool first** so a thin family is not left with whatever a
    plentiful one declined to take. Texts are claimed as they are used, so no utterance and no
    repeated text is sampled twice.
    """
    development = [d for d in dialogues if assignment[d["key"]] == "development"]
    by_source = collections.defaultdict(list)
    for dialogue in development:
        by_source[dialogue["source"]].append(dialogue)

    selected, shortages = [], []
    for source in sorted(by_source):
        eligible = []
        for dialogue in by_source[source]:
            for index, utterance in enumerate(dialogue["utterances"]):
                text = utterance["text"]
                if len(text) > MAX_SCALARS:
                    continue
                eligible.append((dialogue, index, utterance, candidates(text, families)))

        claimed_texts, claimed_ids = set(), set()

        pools = {
            family: sorted(
                (item for item in eligible if family in item[3]),
                key=lambda item: stable("family", source, family, item[0]["key"], item[2]["utterance_id"]),
            )
            for family in families
        }
        # Scarcest pool first, name as tie-break.
        for family in sorted(pools, key=lambda f: (len(pools[f]), f)):
            want, taken = allocation[family], 0
            for dialogue, index, utterance, found in pools[family]:
                if taken >= want:
                    break
                identity = (dialogue["key"], utterance["utterance_id"])
                if identity in claimed_ids or utterance["text"] in claimed_texts:
                    continue
                claimed_ids.add(identity)
                claimed_texts.add(utterance["text"])
                selected.append(
                    _case(source, dialogue, index, utterance, found, "family", family)
                )
                taken += 1
            if taken < want:
                shortages.append(
                    {
                        "source": source,
                        "stratum": f"family:{family}",
                        "wanted": want,
                        "got": taken,
                        "pool": len(pools[family]),
                    }
                )

        independent = sorted(
            eligible,
            key=lambda item: stable("independent", source, item[0]["key"], item[2]["utterance_id"]),
        )
        taken = 0
        for dialogue, index, utterance, found in independent:
            if taken >= INDEPENDENT_BUDGET:
                break
            identity = (dialogue["key"], utterance["utterance_id"])
            if identity in claimed_ids or utterance["text"] in claimed_texts:
                continue
            claimed_ids.add(identity)
            claimed_texts.add(utterance["text"])
            selected.append(_case(source, dialogue, index, utterance, found, "independent", None))
            taken += 1
        if taken < INDEPENDENT_BUDGET:
            shortages.append(
                {
                    "source": source,
                    "stratum": "independent",
                    "wanted": INDEPENDENT_BUDGET,
                    "got": taken,
                    "pool": len(independent),
                }
            )

    selected.sort(key=lambda case: case["case_id"])
    return selected, shortages


def _case(source, dialogue, index, utterance, found, stratum, family):
    turns = dialogue["utterances"]
    before = turns[max(0, index - CONTEXT_BEFORE) : index]
    after = turns[index + 1 : index + 1 + CONTEXT_AFTER]
    return {
        "case_id": f"{source}/{dialogue['dialogue_id']}/{utterance['utterance_id']}",
        "source": source,
        "dialogue_id": dialogue["dialogue_id"],
        "utterance_id": utterance["utterance_id"],
        "order": utterance.get("order"),
        "speaker": utterance.get("speaker"),
        "condition": dialogue.get("condition"),
        "stratum": stratum,
        "family": family,
        "matched_families": found,
        "text": utterance["text"],
        "sha256": utterance.get("sha256") or digest(utterance["text"].encode("utf-8")),
        "characters": len(utterance["text"]),
        "context_before": [
            {"speaker": t.get("speaker"), "text": t["text"]} for t in before
        ],
        "context_after": [{"speaker": t.get("speaker"), "text": t["text"]} for t in after],
    }


def build(chat_dir, acquisition):
    families = _families()
    dialogues, files = read_corpus(chat_dir)
    if not dialogues:
        raise SystemExit(f"no normalized corpus in {chat_dir}; run fetch-chat-corpora.py --download")

    duplicates = duplicate_dialogues(dialogues)
    assignment = partition(dialogues, duplicates)
    allocation = allocate(
        CAPTURE_ATTESTATION,
        FAMILY_BUDGET,
        FAMILY_FLOOR,
        fixed={"regional": REGIONAL_DISCOVERY},
    )
    cases, shortages = sample(dialogues, assignment, families, allocation)

    sides = collections.Counter(assignment.values())
    per_source = collections.defaultdict(collections.Counter)
    for dialogue in dialogues:
        per_source[dialogue["source"]][assignment[dialogue["key"]]] += 1

    sources = {}
    acquired = {}
    if acquisition and pathlib.Path(acquisition).exists():
        acquired = json.loads(pathlib.Path(acquisition).read_text(encoding="utf-8"))
    for source in sorted(per_source):
        record = (acquired.get("sources") or {}).get(source, {})
        sources[source] = {
            "revision": record.get("revision"),
            "licence": record.get("licence"),
            "archive_sha256": record.get("sha256"),
            "archive_bytes": record.get("bytes"),
            "normalized": files.get(f"{source}.jsonl"),
            "dialogues": sum(per_source[source].values()),
            "development_dialogues": per_source[source]["development"],
            "holdout_dialogues": per_source[source]["holdout"],
        }

    manifest = {
        "frozen_for": "colloquial-recognition pilot, engine comparison",
        "salt": SALT,
        "tool_sha256": digest(pathlib.Path(__file__).read_bytes()),
        "sources": sources,
        "partition": {
            "level": "dialogue",
            "why_not_speaker": (
                "one connected component holds 93.4% of dialogues, so no combination of whole "
                "components lands in the 15-25% range; see chat-partition.py"
            ),
            "whole_component_holdout_available": {
                "share": 0.066,
                "not_taken_because": (
                    "the components coincide exactly with the two sources, so this holds out "
                    "different source material and answers a different question"
                ),
            },
            "target_holdout": HOLDOUT,
            "dialogues": dict(sides),
            "achieved_holdout_share": sides["holdout"] / len(assignment),
            "speaker_overlap": speaker_overlap(dialogues, assignment),
            "claim_limit": (
                "dialogue-level separation only; speakers appear on both sides and nothing built "
                "on this split may be called speaker-independent generalization"
            ),
            "assignment_sha256": digest(
                json.dumps(assignment, sort_keys=True, ensure_ascii=False).encode("utf-8")
            ),
        },
        "duplicates": {
            "dialogue_groups": len(duplicates),
            "dialogues_involved": sum(len(v) for v in duplicates.values()),
            "groups": [sorted(keys) for keys in sorted(duplicates.values())],
            "pinned_to_one_side": True,
            "utterance_text_duplicates": (
                "not used for partitioning; deduplicated within the sample only"
            ),
        },
        "sample": {
            "per_source": PER_SOURCE,
            "family_budget": FAMILY_BUDGET,
            "independent_budget": INDEPENDENT_BUDGET,
            "allocation": allocation,
            "allocation_basis": (
                "largest-remainder apportionment over promoted-capture attestation, floor "
                f"{FAMILY_FLOOR}; regional is a fixed discovery reservation of "
                f"{REGIONAL_DISCOVERY}, not weighted"
            ),
            "max_scalars": MAX_SCALARS,
            "context_turns": {"before": CONTEXT_BEFORE, "after": CONTEXT_AFTER},
            "cases": len(cases),
            "by_source": dict(collections.Counter(c["source"] for c in cases)),
            "by_stratum": dict(collections.Counter(c["stratum"] for c in cases)),
            "independent_that_match_a_pattern": sum(
                1 for c in cases if c["stratum"] == "independent" and c["matched_families"]
            ),
            "shortages": shortages,
            "case_ids_sha256": digest(
                "\n".join(c["case_id"] for c in cases).encode("utf-8")
            ),
            "content_sha256": digest(
                "\n".join(f"{c['case_id']}\x1f{c['sha256']}" for c in cases).encode("utf-8")
            ),
        },
        "reporting_rules": [
            "report each source separately; the sample is equally weighted and the pool is not",
            "any combined figure is an equally weighted diagnostic sample, not an estimate of the "
            "pooled corpus's accuracy",
            "per-utterance rates depend on utterance length; show per-character rates alongside",
            "the regional and independent allocations are frozen and may not be resized after "
            "engine results are seen",
        ],
    }
    manifest["freeze_id"] = digest(
        json.dumps(manifest, sort_keys=True, ensure_ascii=False).encode("utf-8")
    )
    return manifest, cases, assignment


def write_artifacts(out_dir, manifest, cases, assignment):
    out = pathlib.Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    (out / "chat-freeze.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    with (out / "chat-sample.jsonl").open("w", encoding="utf-8", newline="\n") as handle:
        for case in cases:
            handle.write(json.dumps(case, ensure_ascii=False, sort_keys=True) + "\n")
    with (out / "chat-partition-assignment.tsv").open("w", encoding="utf-8", newline="\n") as handle:
        handle.write("dialogue\tpartition\n")
        for key in sorted(assignment):
            handle.write(f"{key}\t{assignment[key]}\n")
    return out


def report(manifest, out=sys.stdout):
    print("=== freeze ===", file=out)
    print(f"  freeze id  {manifest['freeze_id'][:16]}…", file=out)
    print(f"  tool       {manifest['tool_sha256'][:16]}…", file=out)
    for source, record in manifest["sources"].items():
        print(
            f"  {source:34} rev {(record['revision'] or '?')[:12]}"
            f"  {record['dialogues']:>6,} dialogues"
            f"  dev {record['development_dialogues']:>6,} / hold {record['holdout_dialogues']:>5,}",
            file=out,
        )

    part = manifest["partition"]
    print("\n=== partition ===", file=out)
    print(f"  level {part['level']}, holdout {part['achieved_holdout_share']:.1%}"
          f" (target {part['target_holdout']:.0%})", file=out)
    overlap = part["speaker_overlap"]
    print(
        f"  speakers {overlap['speakers']:,}, on both sides {overlap['in_both_partitions']:,}"
        f" ({overlap['in_both_partitions'] / max(overlap['speakers'], 1):.0%})",
        file=out,
    )
    print(f"  {part['claim_limit']}", file=out)

    dup = manifest["duplicates"]
    print(f"\n  duplicate dialogue groups {dup['dialogue_groups']},"
          f" {dup['dialogues_involved']} dialogues, pinned to one side", file=out)

    sample_ = manifest["sample"]
    print("\n=== sample ===", file=out)
    print(f"  {sample_['cases']} cases  {sample_['by_source']}", file=out)
    print(f"  strata {sample_['by_stratum']}", file=out)
    print("\n  family allocation (per source)", file=out)
    for family, count in sorted(sample_["allocation"].items(), key=lambda kv: (-kv[1], kv[0])):
        note = "  (fixed discovery reservation)" if family == "regional" else ""
        print(f"    {family:22} {count:>3}{note}", file=out)
    print(
        f"\n  {sample_['independent_that_match_a_pattern']} of the independently sampled"
        f" utterances happen to match a pattern — a diagnostic, not a target", file=out
    )
    if sample_["shortages"]:
        print("\n  SHORTAGES", file=out)
        for shortage in sample_["shortages"]:
            print(
                f"    {shortage['source']} {shortage['stratum']}:"
                f" wanted {shortage['wanted']}, got {shortage['got']}"
                f" (pool {shortage['pool']})",
                file=out,
            )
    else:
        print("\n  no shortages: every stratum filled", file=out)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--chat", default=str(ROOT / "scratch" / "colloquial" / "chat"))
    parser.add_argument("--out", default=str(ROOT / "scratch" / "colloquial"))
    parser.add_argument(
        "--acquisition", default=str(ROOT / "scratch" / "colloquial" / "chat" / "acquisition-manifest.json")
    )
    parser.add_argument("--write", action="store_true", help="produce the freeze; refuses to overwrite")
    parser.add_argument("--verify", action="store_true", help="recompute and compare with the freeze")
    args = parser.parse_args(argv)

    existing = pathlib.Path(args.out) / "chat-freeze.json"
    if args.write and existing.exists():
        print(
            f"{existing} already exists. A freeze is not re-run silently — move it aside "
            "deliberately if you really mean to re-freeze.",
            file=sys.stderr,
        )
        return 2

    manifest, cases, assignment = build(args.chat, args.acquisition)

    if args.verify:
        if not existing.exists():
            print(f"no freeze at {existing}", file=sys.stderr)
            return 1
        frozen = json.loads(existing.read_text(encoding="utf-8"))
        if frozen["freeze_id"] == manifest["freeze_id"]:
            print(f"freeze {frozen['freeze_id'][:16]}… verified: the data still matches.")
            return 0
        print(f"FREEZE MISMATCH\n  frozen   {frozen['freeze_id']}\n  computed {manifest['freeze_id']}",
              file=sys.stderr)
        for field in ("sources", "partition", "duplicates", "sample"):
            if json.dumps(frozen.get(field), sort_keys=True) != json.dumps(
                manifest.get(field), sort_keys=True
            ):
                print(f"  differs: {field}", file=sys.stderr)
        return 1

    report(manifest)
    if args.write:
        out = write_artifacts(args.out, manifest, cases, assignment)
        print(f"\nwrote {out / 'chat-freeze.json'}")
        print(f"wrote {out / 'chat-sample.jsonl'}")
        print(f"wrote {out / 'chat-partition-assignment.tsv'}")
    else:
        print("\n(dry run; pass --write to produce the freeze)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
