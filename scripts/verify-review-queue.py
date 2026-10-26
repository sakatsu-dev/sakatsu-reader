"""Does the review queue validate, does its evidence resolve, and is every record accounted for?

    python scripts/verify-review-queue.py

Three separate questions, and conflating any two is how a queue starts looking reviewed:

- **validate** is structural and offline. The transformations replay, the spans land on character
  boundaries and name the text they point at, every judgment carries what it rests on. It says
  nothing about whether the evidence exists.
- **verify** resolves each case against the artefact it claims: the capture image on disk with its
  hash, or the utterance in the pinned corpus revision. **Absent evidence is `incomplete`, never
  `verified`.**
- **account** reconciles the records against the document's own totals, so a record cannot leave
  the queue by losing a field.

Neither of the first two is a review. Every judgment is `proposed` and stays that way until a
person adjudicates it; this only establishes that what a reviewer is shown is really there.

# Record kind is read, never inferred

An earlier version decided what a record was by testing for a `span` key. That is the wrong shape
of test: a **full** case that lost its span silently left validation instead of failing it, which
is the exact failure mode — a record disappearing from the denominator — that the runner exists to
catch elsewhere. Records now declare `record_kind`, an undeclared one is an error, and a full case
missing anything a full case needs **fails**.

# Preliminary records are accounted for, not excused

They carry v2 proposals and no engine output, so they are not verified to a full case's standard
and the report may not present the queue as forty equivalent cases. What they *do* carry — their
own judgments, their missing-evidence list — is checked, and their verification status is reported
as `incomplete` with the reason, rather than omitted.

# The chat files are authenticated before they are read

`pinned_chat_records` used to label whatever JSONL occupied the directory with the dossier's
revision. A revision string is a claim, not provenance: a re-download, a partial write or a
different revision left in place would all have been resolved against happily. The freeze records
a SHA-256 for each normalized file, and that is now checked **before** any record is taken from it.
"""

import argparse
import collections
import hashlib
import importlib.util
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

_spec = importlib.util.spec_from_file_location(
    "annotation_schema", pathlib.Path(__file__).with_name("annotation_schema.py")
)
schema = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(schema)

#: What a full case must carry. A record declaring `full` without these is malformed and fails.
FULL_REQUIRED = ("span", "text", "source", "provenance", "judgments")
#: What a preliminary record must carry. Less, and it must say what it is missing.
PRELIMINARY_REQUIRED = ("judgments", "missing_evidence")


def authenticated_chat_files(chat_dir, freeze_path):
    """`(records, problems)` — utterances keyed as `verify_case` keys them, from files whose
    content matches the freeze.

    **A file that does not match its frozen digest contributes nothing**, and says why. Resolving
    a case against an unauthenticated file would let a re-download or a partial write stand in for
    the corpus the sample was drawn from.
    """
    problems = []
    freeze_file = pathlib.Path(freeze_path)
    if not freeze_file.is_file():
        return {}, [f"no freeze at {freeze_file}; chat evidence cannot be authenticated"]
    freeze = json.loads(freeze_file.read_text(encoding="utf-8"))

    records = {}
    for source, pinned in sorted(freeze.get("sources", {}).items()):
        path = pathlib.Path(chat_dir) / f"{source}.jsonl"
        if not path.is_file():
            problems.append(f"{source}: {path.name} is not present")
            continue
        expected = (pinned.get("normalized") or {}).get("sha256")
        if not expected:
            problems.append(f"{source}: the freeze records no digest for {path.name}")
            continue
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != expected:
            problems.append(
                f"{source}: {path.name} does not match the frozen digest "
                f"({actual[:12]}… against {expected[:12]}…)"
            )
            continue
        revision = pinned.get("revision")
        if not revision:
            problems.append(f"{source}: the freeze records no revision")
            continue
        # **The identity is checked for uniqueness, not assumed to have it.**
        #
        # `verify_case` keys on (dataset, revision, dialogue_id, utterance_id), and in
        # RealPersonaChat that key is **not unique**: `dialogues/10279.json` declares
        # `dialogue_id: 10577`, colliding with the file of that name. 60 identities collide. Every
        # one of them happens to carry identical text today, so nothing currently resolves against
        # the wrong content — but "happens to" is not a property to verify against, and taking
        # whichever record was read last would make provenance depend on directory order.
        #
        # So a colliding key survives only while every record under it agrees, and is dropped with
        # a reason the moment they do not. A case naming a dropped key reads `incomplete`, which
        # is the correct answer to "which of these two is it".
        collisions = collections.defaultdict(set)
        for line in path.open(encoding="utf-8"):
            if not line.strip():
                continue
            dialogue = json.loads(line)
            for utterance in dialogue["utterances"]:
                key = (source, revision, dialogue["dialogue_id"], utterance["utterance_id"])
                collisions[key].add(utterance["text"])
        ambiguous = 0
        for key, texts in collisions.items():
            if len(texts) == 1:
                records[key] = next(iter(texts))
            else:
                ambiguous += 1
        if ambiguous:
            problems.append(
                f"{source}: {ambiguous} utterance identity/identities are ambiguous — two records "
                "share (dialogue_id, utterance_id) with different text, so neither can be resolved"
            )
    return records, problems


def partition(document):
    """`(full, preliminary, malformed)` by declared kind, with every record placed."""
    full, preliminary, malformed = [], [], []
    for record in document.get("cases", []):
        identity = record.get("id", "<no id>")
        kind = record.get("record_kind")
        if kind == "full":
            missing = [field for field in FULL_REQUIRED if field not in record]
            (malformed if missing else full).append(
                (identity, missing) if missing else record
            )
        elif kind == "preliminary":
            missing = [field for field in PRELIMINARY_REQUIRED if field not in record]
            if missing:
                malformed.append((identity, missing))
            else:
                preliminary.append(record)
        else:
            malformed.append((identity, [f"record_kind {kind!r} is not declared or not known"]))
    return full, preliminary, malformed


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--queue", default=str(ROOT / "target" / "evaluation" / "colloquial-review-queue.json")
    )
    parser.add_argument("--corpus", default=str(ROOT / "corpus"))
    parser.add_argument("--chat", default=str(ROOT / "scratch" / "colloquial" / "chat"))
    parser.add_argument("--freeze", default=str(ROOT / "scratch" / "colloquial" / "chat-freeze.json"))
    args = parser.parse_args(argv)

    path = pathlib.Path(args.queue)
    if not path.is_file():
        print(f"no queue at {path}; run the colloquial_review_queue test first", file=sys.stderr)
        return 1
    document = json.loads(path.read_text(encoding="utf-8"))
    full, preliminary, malformed = partition(document)

    print(f"=== {path.name} ===")
    total = len(document.get("cases", []))
    print(f"  {total} records: {len(full)} full, {len(preliminary)} preliminary, {len(malformed)} malformed")

    failed = False
    if malformed:
        failed = True
        print("  MALFORMED RECORDS (a record does not leave the queue by losing a field):")
        for identity, missing in malformed:
            print(f"    {identity}: missing {', '.join(missing)}")

    # Every record placed, and the document's own totals reconciled against what is here.
    if len(full) + len(preliminary) + len(malformed) != total:
        failed = True
        print("  ACCOUNTING: records were lost between reading and partitioning")
    stated = document.get("accounting", {})
    for field, actual in (("full", len(full)), ("preliminary", len(preliminary)), ("records", total)):
        if field in stated and stated[field] != actual:
            failed = True
            print(f"  ACCOUNTING: the document states {field}={stated[field]}, found {actual}")

    if full:
        try:
            schema.validate({"schema": schema.SCHEMA, "cases": full})
        except schema.Invalid as error:
            print(f"  STRUCTURAL VALIDATION FAILED: {error}", file=sys.stderr)
            return 1
        print(f"  structural validation: all {len(full)} full cases are well formed")

    records, problems = authenticated_chat_files(args.chat, args.freeze)
    for problem in problems:
        failed = True
        print(f"  CHAT AUTHENTICATION: {problem}")
    if records:
        print(f"  chat files authenticated against the freeze: {len(records):,} utterances")

    results = schema.verify(
        {"schema": schema.SCHEMA, "cases": full},
        corpus_dir=args.corpus,
        chat_records=records or None,
    )
    counts = collections.Counter(status for status, _ in results.values())
    print(f"  evidence verification (full cases): {dict(counts)}")
    for identity, (status, reason) in sorted(results.items()):
        if status != schema.VERIFIED:
            print(f"    {status:<10} {identity}: {reason}")

    # Preliminary records keep a status of their own rather than being left out of the tally.
    if preliminary:
        print(f"  preliminary records: {len(preliminary)} incomplete by construction")
        missing = collections.Counter(
            item for record in preliminary for item in record["missing_evidence"]
        )
        for item, count in missing.most_common():
            print(f"    {count:>3} missing {item}")

    states = collections.Counter(
        judgment["state"]
        for record in full + preliminary
        for judgment in record["judgments"]
    )
    print(f"  judgment states: {dict(states)}")
    print(
        f"    {sum(len(r['judgments']) for r in full)} on full cases, "
        f"{sum(len(r['judgments']) for r in preliminary)} on preliminary records"
    )
    if set(states) - {schema.PROPOSED}:
        print("  NOTE: a judgment is no longer `proposed`; a review has happened or a bug has.")

    if counts.get(schema.FAILED, 0) or counts.get(schema.INCOMPLETE, 0):
        failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
