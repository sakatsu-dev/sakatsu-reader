"""Speaker-component structure, and whether a speaker-disjoint split is possible at all.

    python scripts/chat-partition.py --json scratch/colloquial/chat-partition.json

Held-out data exists to answer "does this generalize to people the development set never saw", and
that question needs the partition to separate **speakers**, not dialogues. Whether it can is a
property of the corpus, not a choice: speakers who share a conversation cannot be separated without
breaking the conversation, and the relation is transitive. If A talks to B and B to C, all three
move together.

So this builds the speaker–dialogue graph, finds its connected components, and reports their sizes
**before** anybody tries to split. Where no combination of whole components lands in the requested
range, the honest response is dialogue-level separation with the generalization claim limited
accordingly — not an 80/20 ratio presented as speaker independence.

Note the precise claim: a dominant component means **no split in the requested size range**, not
that no speaker-disjoint split of any size exists. A 6% holdout may well be available and simply
too small to be worth having.

Identities are namespaced by dataset: two corpora may reuse a pseudonym, and merging them on that
basis would invent a person.
"""

import argparse
import collections
import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

#: Above this share of dialogues in one component, the target range is unreachable by whole
#: components and the fallback is the only honest option.
DOMINANT = 0.85


def load(paths):
    dialogues = []
    for path in paths:
        for line in pathlib.Path(path).open(encoding="utf-8"):
            record = json.loads(line)
            speakers = {
                f"{record['source']}:{speaker}"
                for speaker in record.get("speakers", [])
                if speaker
            }
            speakers |= {
                f"{record['source']}:{utterance['speaker']}"
                for utterance in record["utterances"]
                if utterance.get("speaker")
            }
            dialogues.append(
                {
                    "id": f"{record['source']}:{record['dialogue_id']}",
                    "source": record["source"],
                    "speakers": sorted(speakers),
                    "utterances": len(record["utterances"]),
                }
            )
    return dialogues


def components(dialogues):
    """Connected components of the speaker–dialogue graph, as lists of dialogue ids."""
    parent = {}

    def find(node):
        parent.setdefault(node, node)
        while parent[node] != node:
            parent[node] = parent[parent[node]]
            node = parent[node]
        return node

    def union(a, b):
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[ra] = rb

    for dialogue in dialogues:
        anchor = dialogue["id"]
        find(anchor)
        for speaker in dialogue["speakers"]:
            union(anchor, speaker)

    grouped = collections.defaultdict(lambda: {"dialogues": [], "speakers": set()})
    for dialogue in dialogues:
        root = find(dialogue["id"])
        grouped[root]["dialogues"].append(dialogue["id"])
        grouped[root]["speakers"].update(dialogue["speakers"])
    return sorted(
        ({"dialogues": g["dialogues"], "speakers": sorted(g["speakers"])} for g in grouped.values()),
        key=lambda g: -len(g["dialogues"]),
    )


def feasible_split(groups, total, target=0.20, low=0.15, high=0.25):
    """Whole components summing closest to `target`, or `None` when the range is unreachable.

    **Subset-sum, not greedy.** Taking components largest-first while they fit gets two kinds of
    answer wrong, and both are reproduced in the tests:

    - components of 61, 14, 13, 12 — greedy takes 14, stops at 14% and reports the range
      unreachable, when 13 + 12 is exactly 25% and valid;
    - components of 55, 25, 12, 8 — greedy takes 25 and stops, when 12 + 8 is 20% and hits the
      target exactly.

    A component is indivisible, so this is a subset-sum over their sizes, solved by dynamic
    programming across reachable totals. Deterministic throughout: components are ordered by size
    then by a stable hash of their smallest dialogue id, reconstruction walks that order, and among
    equally distant sums the smaller holdout wins.
    """
    if total <= 0 or not groups:
        return None
    ordered = sorted(
        groups,
        key=lambda g: (
            -len(g["dialogues"]),
            hashlib.sha256(min(g["dialogues"]).encode("utf-8")).hexdigest(),
        ),
    )
    sizes = [len(group["dialogues"]) for group in ordered]

    # reachable[s] is the index of the component whose inclusion first reached s, or None.
    reachable = [None] * (total + 1)
    reachable[0] = -1
    for index, size in enumerate(sizes):
        if size == 0 or size > total:
            continue
        for value in range(total, size - 1, -1):
            if reachable[value] is None and reachable[value - size] is not None:
                reachable[value] = index

    lowest, highest = low * total, high * total
    candidates = [
        value
        for value in range(total + 1)
        if reachable[value] is not None and lowest <= value <= highest
    ]
    if not candidates:
        return None
    # Closest to the target; among equals, the smaller holdout, so the answer never depends on
    # iteration order.
    best = min(candidates, key=lambda value: (abs(value - target * total), value))

    # Reconstruct. `reachable` records one witness per sum, which is enough to walk back.
    chosen, remaining = [], best
    while remaining > 0:
        index = reachable[remaining]
        if index is None or index < 0:
            return None
        chosen.append(index)
        remaining -= sizes[index]
    return {
        "components": len(chosen),
        "dialogues": best,
        "share": best / total,
        "component_sizes": sorted((sizes[index] for index in chosen), reverse=True),
    }


def report(dialogues, groups, out=sys.stdout):
    total = len(dialogues)
    speakers = {speaker for dialogue in dialogues for speaker in dialogue["speakers"]}
    print("=== speaker–dialogue graph ===", file=out)
    print(f"  dialogues  {total:>8,}", file=out)
    print(f"  speakers   {len(speakers):>8,}", file=out)
    print(f"  components {len(groups):>8,}", file=out)
    print("\n  largest components, by dialogues", file=out)
    for group in groups[:8]:
        share = len(group["dialogues"]) / total
        print(
            f"    {len(group['dialogues']):>8,} dialogues"
            f" {len(group['speakers']):>6,} speakers  {share:>6.1%}",
            file=out,
        )
    if len(groups) > 8:
        print(f"    … and {len(groups) - 8:,} more", file=out)

    largest = len(groups[0]["dialogues"]) / total if groups else 0
    print(f"\n  largest component holds {largest:.1%} of dialogues", file=out)
    split = feasible_split(groups, total)
    if split is None:
        print("  **No combination of whole components lands in 15-25%.**", file=out)
        if largest > DOMINANT:
            print(
                f"  One component holds more than {DOMINANT:.0%}, which is why:"
                " what is left cannot reach the floor.",
                file=out,
            )
        print("  This does not mean no speaker-disjoint split of *any* size exists — a smaller", file=out)
        print("  holdout may well be available and simply too small to be worth having. The", file=out)
        print("  fallback is deterministic dialogue-level separation, with the generalization", file=out)
        print("  claim limited to dialogue level and speaker overlap reported, not implied.", file=out)
    else:
        print(
            f"  A speaker-disjoint holdout is available: {split['components']} components,"
            f" {split['dialogues']:,} dialogues, {split['share']:.1%}",
            file=out,
        )


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--chat", default=str(ROOT / "scratch" / "colloquial" / "chat"))
    parser.add_argument("--json")
    args = parser.parse_args(argv)

    paths = sorted(pathlib.Path(args.chat).glob("*.jsonl"))
    if not paths:
        print(f"no normalized corpus in {args.chat}", file=sys.stderr)
        return 1
    dialogues = load(paths)
    groups = components(dialogues)
    report(dialogues, groups)

    if args.json:
        total = len(dialogues)
        path = pathlib.Path(args.json)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(
                {
                    "dialogues": total,
                    "speakers": len({s for d in dialogues for s in d["speakers"]}),
                    "components": [
                        {"dialogues": len(g["dialogues"]), "speakers": len(g["speakers"])}
                        for g in groups
                    ],
                    "largest_share": len(groups[0]["dialogues"]) / total if groups else 0,
                    "feasible_component_split": feasible_split(groups, total),
                },
                ensure_ascii=False,
                indent=2,
            ),
            encoding="utf-8",
        )
        print(f"\nwrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
