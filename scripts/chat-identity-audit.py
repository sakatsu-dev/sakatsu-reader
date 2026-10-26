"""What identifies a chat utterance, and what the freeze got wrong by trusting the publisher's id.

    python scripts/chat-identity-audit.py --write     # produce the correction
    python scripts/chat-identity-audit.py --verify    # recompute and compare

# The defect

`chat-freeze.py` keys a dialogue as `"{source}:{dialogue_id}"`, taking the publisher's declared id
as an identity. It is not one. In RealPersonaChat `dialogues/10279.json` declares
`"dialogue_id": 10577`, colliding with the file actually named `10577.json`; the same happens for
`11052.json` and `11838.json`. Two pairs, 60 utterance identities.

The freeze already contradicts itself because of it: its `sources` block reports 13,583 + 960 =
**14,543** dialogues, while its own `partition.dialogues` totals **14,541**. The two collapsed pairs
are exactly that gap. Nothing detected it because nothing asked.

# Canonical identity

    (dataset, pinned revision, source-relative dialogue path, utterance order)

The path is what the publisher actually distributes one dialogue as, and `order` is the utterance's
position inside it. Both are already in the normalized JSONL, so this needs no re-acquisition.

Declared `dialogue_id` and `utterance_id` are kept as **metadata**. They are useful for citing the
corpus the way its authors do, and they are not identity.

# Text agreement is not identity

All 60 colliding identities carry identical text, and identical speakers, and identical conditions.
That is recorded as evidence and it does **not** license merging them: they are two files the
publisher ships separately, and two source records that happen to agree are still two source
records. Deduplication is a *sampling* decision, taken with the sample in view, and it is recorded
in the freeze's own `duplicates` block — which is a different question from this one, and which
found six content-duplicate groups under *different* declared ids.

# The freeze is not rewritten

This emits a versioned correction that names the freeze's full digest. Rewriting a frozen artifact
after its results have been read is the thing freezing exists to prevent, so the correction is a
separate file and the freeze keeps its history, including the counts now known to be wrong.

# Partition membership is preserved, not recomputed

The four affected files inherit the assignment their shared key already had. Reassigning them by
canonical identity would be more principled for a *future* freeze and would move dialogues between
development and holdout **after the results were inspected**, which is not a trade this pilot is
willing to make. Nothing moves. No sampled case is affected — all 240 are on uncolliding
identities, so the sample's provenance is uniquely recoverable.
"""

import argparse
import collections
import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: The correction format, separate from the freeze's own version.
SCHEMA = 1


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(source, revision, path, order):
    """The identity a dialogue turn actually has.

    Deliberately **not** the publisher's declared ids: those are metadata that may collide, and in
    this corpus they do.
    """
    return f"{source}{revision}{path}{order}"


def load_freeze(path):
    raw = pathlib.Path(path).read_bytes()
    return json.loads(raw.decode("utf-8")), digest(raw)


def authenticated_records(chat_dir, freeze):
    """Every utterance, from files whose bytes match the freeze. `(records, problems)`.

    **Authentication first.** A file that does not match the digest the freeze pinned is not the
    file the sample was drawn from, and resolving anything against it would make the audit a
    statement about whatever happens to be on disk.
    """
    records, problems = [], []
    for source, pinned in sorted(freeze.get("sources", {}).items()):
        path = pathlib.Path(chat_dir) / f"{source}.jsonl"
        if not path.is_file():
            problems.append(f"{source}: {path.name} is absent")
            continue
        expected = (pinned.get("normalized") or {}).get("sha256")
        revision = pinned.get("revision")
        if not expected or not revision:
            problems.append(f"{source}: the freeze pins no digest or no revision")
            continue
        raw = path.read_bytes()
        actual = digest(raw)
        if actual != expected:
            problems.append(
                f"{source}: {path.name} does not match the frozen digest "
                f"({actual[:12]}… against {expected[:12]}…)"
            )
            continue
        for line in raw.decode("utf-8").splitlines():
            if not line.strip():
                continue
            dialogue = json.loads(line)
            records.append((source, revision, dialogue))
    return records, problems


def audit(records):
    """Group every dialogue by its declared key and by its canonical key."""
    by_declared = collections.defaultdict(list)
    by_canonical = {}
    collisions = []

    for source, revision, dialogue in records:
        declared = f"{source}:{dialogue['dialogue_id']}"
        by_declared[declared].append((source, revision, dialogue))

        key = canonical(source, revision, dialogue["path"], "dialogue")
        if key in by_canonical:
            # Two dialogues at one canonical identity would mean the same file twice, which is a
            # corrupt corpus rather than a publisher's id quirk. Refused rather than absorbed.
            collisions.append(key)
        by_canonical[key] = (source, revision, dialogue)

    return by_declared, by_canonical, collisions


def compare(members):
    """What agrees across dialogues sharing one declared key, field by field.

    Reported per field because they are different claims: identical text with different speakers
    would be a very different finding from identical everything.
    """
    texts = {tuple(u["text"] for u in d["utterances"]) for _, _, d in members}
    speakers = {tuple(d.get("speakers") or ()) for _, _, d in members}
    conditions = {d.get("condition") for _, _, d in members}
    lengths = {len(d["utterances"]) for _, _, d in members}
    return {
        "text_agrees": len(texts) == 1,
        "speakers_agrees": len(speakers) == 1,
        "condition_agrees": len(conditions) == 1,
        "turn_count_agrees": len(lengths) == 1,
        "paths": sorted(d["path"] for _, _, d in members),
        "turn_counts": sorted(lengths),
    }


def sample_impact(sample_path, declared_collisions):
    """Which frozen sample cases sit on a colliding declared identity.

    A case that does is **provenance-incomplete**: its source cannot be chosen without picking one
    of two files, and picking the convenient one is exactly what must not happen.
    """
    path = pathlib.Path(sample_path)
    if not path.is_file():
        return {"sample_cases": 0, "on_colliding_identity": [], "note": "no frozen sample found"}
    affected = []
    total = 0
    for line in path.open(encoding="utf-8"):
        if not line.strip():
            continue
        case = json.loads(line)
        total += 1
        if f"{case['source']}:{case['dialogue_id']}" in declared_collisions:
            affected.append(case["case_id"])
    return {
        "sample_cases": total,
        "on_colliding_identity": sorted(affected),
        "provenance_incomplete": len(affected),
    }


def build(chat_dir, freeze_path, sample_path, assignment_path):
    freeze, freeze_digest = load_freeze(freeze_path)
    records, problems = authenticated_records(chat_dir, freeze)
    if problems and not records:
        raise SystemExit("; ".join(problems))

    by_declared, by_canonical, canonical_collisions = audit(records)
    colliding = {key: members for key, members in by_declared.items() if len(members) > 1}

    # What the freeze said, against what the files hold.
    stated_sources = sum(source["dialogues"] for source in freeze["sources"].values())
    stated_partition = sum(freeze["partition"]["dialogues"].values())
    counted_files = len(records)
    counted_declared = len(by_declared)
    counted_canonical = len(by_canonical)

    assignment = {}
    path = pathlib.Path(assignment_path)
    if path.is_file():
        for line in path.open(encoding="utf-8"):
            if line.startswith("dialogue\t"):
                continue
            key, side = line.rstrip("\n").split("\t")
            assignment[key] = side

    # Old declared key -> canonical identities, with the inherited partition side.
    mappings = []
    for declared, members in sorted(colliding.items()):
        side = assignment.get(declared)
        agreement = compare(members)
        for source, revision, dialogue in sorted(members, key=lambda m: m[2]["path"]):
            mappings.append(
                {
                    "declared_key": declared,
                    "declared_dialogue_id": dialogue["dialogue_id"],
                    "canonical": canonical(source, revision, dialogue["path"], "dialogue"),
                    "path": dialogue["path"],
                    "partition": side,
                    # Preserved, never recomputed. See the module docstring.
                    "partition_source": "inherited from the shared declared key",
                    "ambiguous": not agreement["text_agrees"],
                    "utterances": len(dialogue["utterances"]),
                }
            )

    affected_utterances = sum(len(d["utterances"]) for members in colliding.values() for _, _, d in members)
    # How many *identities* collide, which is the number the verifier has to refuse to guess at.
    colliding_utterance_identities = 0
    for members in colliding.values():
        positions = collections.Counter()
        for _, _, dialogue in members:
            for utterance in dialogue["utterances"]:
                positions[utterance["utterance_id"]] += 1
        colliding_utterance_identities += sum(1 for count in positions.values() if count > 1)

    correction = {
        "schema": SCHEMA,
        "corrects": {
            "artifact": "chat-freeze.json",
            "freeze_id": freeze["freeze_id"],
            "file_sha256": freeze_digest,
            "note": "the freeze is not rewritten; this supersedes the counts named below",
        },
        "canonical_identity": {
            "fields": ["dataset", "pinned revision", "source-relative dialogue path", "utterance order"],
            "declared_ids_are": "metadata, retained for citation and not used as identity",
            "text_agreement_is_not_identity": (
                "two source records that agree are still two source records; deduplication is a "
                "sampling decision and is recorded separately in the freeze's `duplicates` block"
            ),
        },
        "authentication": {
            "files_checked": len(freeze.get("sources", {})),
            "problems": problems,
        },
        "counts": {
            "dialogue_files": counted_files,
            "distinct_declared_keys": counted_declared,
            "distinct_canonical_identities": counted_canonical,
            "freeze_sources_stated": stated_sources,
            "freeze_partition_stated": stated_partition,
            "collapsed_by_declared_key": counted_files - counted_declared,
            "discrepancy_explained": stated_sources - stated_partition,
        },
        "declared_id_collisions": {
            "keys": len(colliding),
            "dialogues_involved": sum(len(m) for m in colliding.values()),
            "utterances_involved": affected_utterances,
            "colliding_utterance_identities": colliding_utterance_identities,
            "detail": [
                {"declared_key": key, **compare(members)} for key, members in sorted(colliding.items())
            ],
        },
        "canonical_collisions": {
            "count": len(canonical_collisions),
            "keys": canonical_collisions,
            "note": "a canonical collision means one file counted twice, which is refused",
        },
        "mappings": mappings,
        "partition_effect": {
            "policy": "preserved, not recomputed",
            "why": (
                "reassigning by canonical identity would move dialogues between development and "
                "holdout after the results were inspected"
            ),
            "rows_affected": len({m["declared_key"] for m in mappings}),
            "dialogues_moved": 0,
        },
        "sample_effect": sample_impact(sample_path, set(colliding)),
        "supersedes": [
            f"chat-freeze.json sources total {stated_sources} dialogues",
            f"chat-freeze.json partition total {stated_partition} dialogues",
            "Earlier corpus report: 14,543 dialogues",
        ],
        "recommendation": "a future freeze keys on canonical identity from the start",
    }
    correction["correction_id"] = digest(
        json.dumps(correction, sort_keys=True, ensure_ascii=False).encode("utf-8")
    )
    return correction


def report(correction, out=sys.stdout):
    counts = correction["counts"]
    collisions = correction["declared_id_collisions"]
    print("=== chat source identity audit ===", file=out)
    print(f"  correcting freeze {correction['corrects']['freeze_id'][:16]}…", file=out)
    print(f"  correction id     {correction['correction_id'][:16]}…", file=out)
    for problem in correction["authentication"]["problems"]:
        print(f"  AUTHENTICATION: {problem}", file=out)

    print("\n  counts", file=out)
    print(f"    dialogue files                 {counts['dialogue_files']:>7,}", file=out)
    print(f"    distinct declared keys         {counts['distinct_declared_keys']:>7,}", file=out)
    print(f"    distinct canonical identities  {counts['distinct_canonical_identities']:>7,}", file=out)
    print(f"    freeze `sources` stated        {counts['freeze_sources_stated']:>7,}", file=out)
    print(f"    freeze `partition` stated      {counts['freeze_partition_stated']:>7,}", file=out)
    print(
        f"    collapsed by declared key      {counts['collapsed_by_declared_key']:>7,}"
        f"  (explains the {counts['discrepancy_explained']} discrepancy)",
        file=out,
    )

    print(f"\n  declared-id collisions: {collisions['keys']} key(s), "
          f"{collisions['dialogues_involved']} dialogues, "
          f"{collisions['colliding_utterance_identities']} utterance identities", file=out)
    for detail in collisions["detail"]:
        agree = ", ".join(
            name for name in ("text", "speakers", "condition", "turn_count")
            if detail[f"{name}_agrees"]
        )
        print(f"    {detail['declared_key']}: {detail['paths']}", file=out)
        print(f"      agrees on: {agree or '(nothing)'}", file=out)

    if correction["canonical_collisions"]["count"]:
        print(f"\n  CANONICAL COLLISIONS: {correction['canonical_collisions']['keys']}", file=out)

    sample = correction["sample_effect"]
    print(f"\n  frozen sample: {sample['sample_cases']} cases, "
          f"{sample.get('provenance_incomplete', 0)} on a colliding identity", file=out)
    partition = correction["partition_effect"]
    print(f"  partition: {partition['rows_affected']} row(s) affected, "
          f"{partition['dialogues_moved']} dialogue(s) moved ({partition['policy']})", file=out)
    print("\n  supersedes:", file=out)
    for claim in correction["supersedes"]:
        print(f"    {claim}", file=out)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--chat", default=str(ROOT / "scratch" / "colloquial" / "chat"))
    parser.add_argument("--freeze", default=str(ROOT / "scratch" / "colloquial" / "chat-freeze.json"))
    parser.add_argument("--sample", default=str(ROOT / "scratch" / "colloquial" / "chat-sample.jsonl"))
    parser.add_argument(
        "--assignment", default=str(ROOT / "scratch" / "colloquial" / "chat-partition-assignment.tsv")
    )
    parser.add_argument("--out", default=str(ROOT / "scratch" / "colloquial" / "chat-identity-correction.json"))
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args(argv)

    correction = build(args.chat, args.freeze, args.sample, args.assignment)
    existing = pathlib.Path(args.out)

    if args.verify:
        if not existing.is_file():
            print(f"no correction at {existing}", file=sys.stderr)
            return 1
        frozen = json.loads(existing.read_text(encoding="utf-8"))
        if frozen["correction_id"] == correction["correction_id"]:
            print(f"correction {frozen['correction_id'][:16]}… verified: the data still matches.")
            return 0
        print(
            f"CORRECTION MISMATCH\n  recorded {frozen['correction_id']}\n"
            f"  computed {correction['correction_id']}",
            file=sys.stderr,
        )
        return 1

    report(correction)
    if args.write:
        existing.parent.mkdir(parents=True, exist_ok=True)
        existing.write_text(
            json.dumps(correction, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        print(f"\nwrote {existing}")
    else:
        print("\n(dry run; pass --write to record the correction)")
    return 1 if correction["canonical_collisions"]["count"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
