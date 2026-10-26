"""Find native example sentences for each grammar pattern, for the JLPT test suite.

Run once, by hand; its *output* is reviewed and committed. The tests must not read the
installed dictionary at run time, or they would depend on which version happens to be
installed and stop being deterministic.

    python scripts/harvest-sentences.py            > candidates.txt
    python scripts/harvest-sentences.py --particle > particles.txt

# The particle mode

`--heteronym` does the same job for the one error class the reading suite cannot otherwise
reach: a spelling with more than one reading. `reading_accuracy.rs` accepts *any* reading the
dictionary lists for such a form, because pinning one from the spelling alone would be my
preference rather than the dictionary's — which means 今日 read こんち scores as agreement.

This mode gets the answer from the dictionary instead. A Jitendex example sentence is attached
to an *entry*, and an entry is a JMdict sequence with its own reading, so a sentence filed under
sequence 1579110 前【まえ】 is the dictionary saying that 前 reads まえ *in that sentence*. Two
readings of one spelling that live in different sequences are different words, and their example
sentences are therefore reading-attributed with no judgement from me at all.

Readings that share a sequence are skipped, and they are the majority: 今日 lists きょう, こんにち,
こんち and こんじつ on one sequence, so its sentences say nothing about which is meant. That is the
same honest limit `reading_accuracy.rs` runs into — this mode simply finds the cases where the
dictionary *does* answer.

`--particle` does the same job for `packs/particles.json`: for every sense of every
particle it finds sentences where that sense's own cues fire, so each reading can be
shown with real sentences instead of ones I made up.

It fires the cues by searching rather than by parsing — the tokenizer is in Rust — which
works because the cues are about *surfaces*. A category cue asks what kind of thing sits
before the particle, and 図書館で is a literal substring, so the word is recoverable by
trying the suffixes that end where the particle begins and looking each up in the WordNet
table. That table is what `scripts/fetch-wordnet.ps1` writes; without it the category
probes are skipped and the structural ones still run.

# Where the sentences come from

Jitendex bundles Tatoeba-derived example sentences with English translations — 26,019 of
them, already installed, already displayed by the app and already attributed under
CC BY-SA 4.0 (`project notes` §8). Nothing is downloaded and no new licence is taken on.

# Why selecting from it is not circular

Candidates are found by **substring search** on the surface a pattern is written with.
The test then asserts that the **token-aware matcher, with its element constraints**,
finds the pattern. Those are different mechanisms, so a sentence passing the first says
nothing about whether it passes the second — which is the whole point.

It also produces the negatives for free: searching for 上に turns up 机の上に along with
the real grammar point, and the false friends are exactly what the precision test needs.
They come out of the same search rather than out of anyone's imagination.

# What it deliberately does not do

Pick. Every candidate is printed with its English translation so a person can decide
whether the grammar point is really what is happening in that sentence. A script that
chose for itself would be automating the one judgement worth making by hand.
"""

import collections
import json
import os
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# How many candidates to show per pattern. Enough to choose from, few enough to read.
PER_PATTERN = 4

# How many sentences to offer per heteronym reading. Few, because each one is read by hand.
PER_HETERONYM = 3

# Filings a read of the English rejected.
#
# **The dictionary decides which reading a sentence carries, and it is occasionally wrong about
# it.** A Tatoeba link points at a JMdict sequence, and a link on the wrong sequence asserts a
# reading the sentence plainly does not have. Those are not evidence of anything, and left in they
# are scored as analyzer errors — which would make the suite's number a claim about the fixture.
#
# Each was rejected by reading the translation that came with it, which is the only check this
# script cannot do. Recorded here rather than deleted from the generated file so that regenerating
# reproduces the reviewed set instead of quietly re-importing the noise.
#
# Keyed on (spelling, reading): it is the *filing* that is wrong, not the individual sentence. A
# spelling left with fewer than two readings afterwards drops out on its own, since one reading
# proves nothing.
MISFILED = {
    # The analyzer was right and the dictionary's filing was not.
    ("上手", "うわて"),   # "Taeko, come on from stage left" — かみて, which the analyzer gave
    ("夜中", "やちゅう"),  # "all of that night" — よなか
    ("気味", "きび"),      # "There was something weird" — 気味が悪い is きみ
    ("袋", "たい"),        # "two bags of popcorn" — ふくろ
    ("屋", "おく"),        # "an all-you-can-eat Yakiniku restaurant" — 焼肉屋 is や
    # Readings no longer current that survived the score and tag filters.
    ("実", "じち"),
    ("出来", "しゅつらい"),
    # The translation does not settle which reading is meant, so it is not evidence either way.
    ("中", "うち"),        # "The church is between my house and yours"
    ("姉", "ねえ"),
    ("者", "もん"),        # a colloquial variant of もの; the English cannot distinguish them
    ("表", "おもて"),      # neither sentence shows 表 in a context the translation decides
}

# Tags marking a reading as not current Japanese. Jitendex splits them into words, so these are
# the individual tokens rather than the phrases ("rarely used", "old kanji", "irregular reading").
STALE = {"obsolete", "rarely", "irregular", "old", "ateji"}

# How many unmeasured spellings to list. The tail is long and the head is what gets acted on.
BLIND_SPOTS = 40

# Sentences longer than this are hard to judge at a glance and make a poor fixture.
MAX_LEN = 60

# The shortest surface worth searching for.
#
# Two characters is not enough to be distinctive: `します` finds お願いします, which has
# nothing to do with the い-adjective construction the pattern describes. A substring hit
# on a short, common string is noise, and every one of those is a candidate a person has
# to read and reject. Raising the floor spends fewer patterns to get better ones.
MIN_SURFACE = 3


def corpus():
    """Every example sentence in the installed Jitendex, with its translation."""
    appdata = os.environ.get("APPDATA")
    if not appdata:
        sys.exit("no APPDATA; this reads the installed dictionary")
    path = (
        pathlib.Path(appdata)
        / "com.sakatsu.app"
        / "dictionaries"
        / "jitendex"
        / "terms.ndjson"
    )
    if not path.is_file():
        sys.exit(f"{path} not found — run scripts/fetch-dictionaries.ps1 first")

    seen = {}
    for line in path.open(encoding="utf-8"):
        entry = json.loads(line)
        for definition in entry.get("definitions", []):
            for example in definition.get("examples", []):
                japanese = (example.get("japanese") or "").strip()
                if japanese and japanese not in seen:
                    seen[japanese] = (example.get("english") or "").strip()
    return seen


def packs():
    """Every pattern from both packs, with the surfaces it is written with."""
    out = []
    for name in ("hanabira.json", "core.json"):
        data = json.loads(
            (ROOT / "core" / "src" / "grammar" / "packs" / name).read_text(encoding="utf-8")
        )
        for entry in data["patterns"]:
            surfaces = list(entry.get("match_text") or [])
            for element in entry.get("match") or []:
                if not isinstance(element, dict):
                    continue
                for key in ("text", "surface", "surface_ends", "surface_any"):
                    value = element.get(key)
                    if isinstance(value, str):
                        surfaces.append(value)
                    elif isinstance(value, list):
                        surfaces.extend(s for s in value if isinstance(s, str))
            surfaces = sorted({s for s in surfaces if len(s) >= MIN_SURFACE})
            if surfaces:
                out.append(
                    (
                        entry["id"],
                        str(entry.get("jlpt")),
                        entry.get("pattern", ""),
                        # The title says what the point *means*, which is what a candidate
                        # has to be judged against.
                        entry.get("title", ""),
                        surfaces,
                    )
                )
    return out


def pack_examples():
    """The pack's own example sentences — what the suite must stay clear of."""
    out = set()
    for name in ("hanabira.json", "core.json"):
        data = json.loads(
            (ROOT / "core" / "src" / "grammar" / "packs" / name).read_text(encoding="utf-8")
        )
        for entry in data["patterns"]:
            for example in entry.get("examples", []):
                if isinstance(example, dict) and example.get("japanese"):
                    out.add(example["japanese"].strip())
    return out


# ---------------------------------------------------------------------------------------
# Particle senses.
# ---------------------------------------------------------------------------------------

# How far back to look for the word before a particle. Six characters covers ordinary
# compounds (図書館, 新幹線) without reaching across the word before that.
LOOKBACK = 6

# More candidates than the pattern mode shows. A sense has to be illustrated by a
# sentence where *that* reading is unmistakable, and most hits are merely compatible
# with it, so there is more to reject.
PER_SENSE = 8

NUMERALS = set("0123456789０１２３４５６７８９一二三四五六七八九十百千万何")

# What the cues can be fired by searching. `preceding_pos` cannot: it asks the analyzer
# what part of speech the previous word is, and that is not recoverable from the text.
# A sense resting on it gets its candidates from whatever else it has.
SEARCHABLE = {
    "followed_by",
    "predicate",
    "preceding_category",
    "preceding_counter",
    "second_wa",
}

# Occurrences that are not the particle at all, by what follows them.
#
# で inside です and でした is the copula, and the tokenizer never splits it — so every
# such hit is noise the reader would have to reject by hand. Rejecting them here is not
# the script "picking": it is declining to offer a sentence that does not contain the
# thing being illustrated.
NOT_THE_PARTICLE = {"で": ("す", "し")}


def categories():
    """The WordNet noun → class table, if `scripts/fetch-wordnet.ps1` has been run."""
    appdata = os.environ.get("APPDATA")
    path = pathlib.Path(appdata or ".") / "com.sakatsu.app" / "wordnet" / "categories.tsv"
    if not path.is_file():
        return {}
    table = {}
    for line in path.open(encoding="utf-8"):
        word, _, category = line.partition("\t")
        if category:
            table[word] = category.strip()
    return table


def preceding_word(sentence, at, table):
    """The word ending just before position `at`, longest match in the table wins."""
    for length in range(min(LOOKBACK, at), 0, -1):
        candidate = sentence[at - length : at]
        if candidate in table:
            return candidate, table[candidate]
    return None, None


def cue_fires(cue, sentence, at, surface, table):
    """Whether one cue fires at this occurrence, and how to say it in the report."""
    kind = cue["kind"]
    after = at + len(surface)

    if kind == "followed_by":
        return next(
            (f"followed by {s}" for s in cue["surfaces"] if sentence.startswith(s, after)),
            None,
        )

    if kind == "predicate":
        # After the particle only. A verb standing in front of it is some other clause's,
        # and counting it would manufacture agreement the sentence does not have.
        return next((f"…{s}" for s in cue["surfaces"] if s in sentence[after:]), None)

    if kind == "preceding_category":
        word, found = preceding_word(sentence, at, table)
        if found in cue["categories"]:
            return f"{word} is {found}"
        return None

    if kind == "preceding_counter":
        before = sentence[max(0, at - LOOKBACK) : at]
        return "an amount before it" if any(c in NUMERALS for c in before) else None

    if kind == "second_wa":
        return "another は" if sentence.count("は") > 1 else None

    # preceding_pos needs the analyzer's part of speech, which is not recoverable from
    # the text. Those senses get candidates from their other cues.
    return None


def particles():
    """Every sense of every particle, with its cues."""
    data = json.loads(
        (ROOT / "core" / "src" / "grammar" / "packs" / "particles.json").read_text(
            encoding="utf-8"
        )
    )
    return data["particles"]


def harvest_particles():
    sentences = corpus()
    table = categories()

    print(f"# corpus: {len(sentences)} native sentences")
    print(f"# wordnet: {len(table) or 'not fetched — category cues skipped'}")
    print()

    for entry in particles():
        surface = entry["surface"]
        print("=" * 70)
        print(f"{surface}")
        print("=" * 70)

        skip_after = NOT_THE_PARTICLE.get(surface, ())

        for sense in entry["senses"]:
            print(f"\n{sense['id']} — {sense['label']}")
            wanted = [cue for cue in sense.get("cues", []) if cue["kind"] in SEARCHABLE]
            if not wanted:
                kinds = {cue["kind"] for cue in sense.get("cues", [])}
                print(f"   nothing to search on ({', '.join(sorted(kinds)) or 'no cues'})")
                continue

            hits = []
            for japanese, english in sentences.items():
                if len(japanese) > MAX_LEN:
                    continue
                at = japanese.find(surface)
                while at != -1 and len(hits) < PER_SENSE:
                    after = at + len(surface)
                    if japanese[after : after + 1] in skip_after and skip_after:
                        at = japanese.find(surface, at + 1)
                        continue
                    reasons = [
                        reason
                        for cue in wanted
                        if (reason := cue_fires(cue, japanese, at, surface, table))
                    ]
                    # Every searchable cue, not any of them. One cue firing is what the
                    # app needs to *lean*; a sentence printed as the example of a sense
                    # should be one where the reading is not in question.
                    if len(reasons) == len(wanted):
                        hits.append((japanese, english, reasons))
                        break
                    at = japanese.find(surface, at + 1)
                if len(hits) >= PER_SENSE:
                    break

            if not hits:
                print("   nothing found with every cue firing")
            for japanese, english, reasons in hits:
                print(f"    JA  {japanese}")
                print(f"    EN  {english}")
                print(f"    ..  {', '.join(reasons)}")


def heteronym_entries():
    """Every (spelling, sequence, reading, examples) the installed Jitendex carries.

    Unlike `corpus()`, which flattens every example into one sentence-to-translation map, this
    keeps *which entry* an example came from. That association is the whole point: it is what
    turns a sentence into evidence for one reading rather than another.
    """
    appdata = os.environ.get("APPDATA")
    if not appdata:
        sys.exit("no APPDATA; this reads the installed dictionary")
    path = (
        pathlib.Path(appdata)
        / "com.sakatsu.app"
        / "dictionaries"
        / "jitendex"
        / "terms.ndjson"
    )
    if not path.is_file():
        sys.exit(f"{path} not found - run scripts/fetch-dictionaries.ps1 first")

    forms = collections.defaultdict(
        lambda: collections.defaultdict(lambda: {"reading": "", "sentences": []})
    )
    for line in path.open(encoding="utf-8"):
        entry = json.loads(line)
        term = entry.get("term") or ""
        reading = entry.get("reading") or ""
        sequence = entry.get("sequence")
        # A "reading" equal to the spelling is a kana headword carrying no reading claim.
        if not term or not reading or reading == term or sequence is None:
            continue
        # **Readings the dictionary itself marks as not current.** A sequence can carry a
        # reading nobody uses — 上手 lists じょうしゅ at score -102, tagged obsolete — and an
        # example linked to it would assert that reading against a sentence plainly meaning
        # じょうず. Excluding them is the dictionary's own judgement, not mine: `score` below
        # zero is JMdict saying rare, and the tags say obsolete, irregular or ateji outright.
        #
        # Deliberately not excluded: a *low but non-negative* reading such as 下手【しもて】
        # (stage left, score 0, untagged). Those are ordinary words in their own register, and
        # they are exactly where an analyzer goes wrong.
        if entry.get("score", 0) < 0 or (set(entry.get("definition_tags") or []) & STALE):
            continue
        slot = forms[term][sequence]
        slot["reading"] = reading
        for definition in entry.get("definitions", []):
            for example in definition.get("examples", []):
                japanese = (example.get("japanese") or "").strip()
                english = (example.get("english") or "").strip()
                if japanese:
                    slot["sentences"].append((japanese, english))
    return forms


def frequency_ranks():
    """jpdb's frequency rank per spelling, lower being commoner. Empty if it is not installed."""
    appdata = os.environ.get("APPDATA")
    if not appdata:
        return {}
    path = (
        pathlib.Path(appdata)
        / "com.sakatsu.app"
        / "dictionaries"
        / "frequency"
        / "jpdb"
        / "term_meta_bank_1.json"
    )
    if not path.is_file():
        return {}
    # **Two shapes in one bank, and reading only one of them is silently wrong.** A kana entry is
    # `{"value": n}`; a kanji entry nests it as `{"reading": …, "frequency": {"value": n}}`. Taking
    # only the flat form collects every kana word and not one kanji word — which is exactly the
    # half this report is about, and it showed up as a blind-spot list of zero rather than as an
    # error.
    ranks = {}
    for row in json.loads(path.read_text(encoding="utf-8")):
        if len(row) != 3 or row[1] != "freq" or not isinstance(row[2], dict):
            continue
        body = row[2]
        value = body.get("value")
        if value is None and isinstance(body.get("frequency"), dict):
            value = body["frequency"].get("value")
        # Keep the best (lowest) rank a spelling is given; a word appears once per corpus.
        if isinstance(value, int) and value < ranks.get(row[0], 1 << 30):
            ranks[row[0]] = value
    return ranks


def harvest_heteronyms():
    """Sentences where the dictionary itself settles which reading a spelling takes."""
    forms = heteronym_entries()
    excluded = pack_examples()

    picked = []
    for term, sequences in forms.items():
        by_reading = collections.defaultdict(list)
        for sequence, slot in sequences.items():
            by_reading[slot["reading"]].append(sequence)
        if len(by_reading) < 2:
            continue

        evidence = {}
        for reading, seqs in by_reading.items():
            if (term, reading) in MISFILED:
                continue
            hits, seen = [], set()
            for sequence in seqs:
                for japanese, english in sequences[sequence]["sentences"]:
                    if japanese in excluded or len(japanese) > MAX_LEN:
                        continue
                    # **Exactly once.** Twice and there is no saying which occurrence the
                    # dictionary filed the sentence for, so the sentence could answer either
                    # way and proves nothing.
                    if japanese.count(term) != 1 or japanese in seen:
                        continue
                    seen.add(japanese)
                    hits.append((japanese, english))
            if hits:
                evidence[reading] = hits

        # Two readings, each with a sentence of its own. One alone proves nothing: an
        # analyzer that always answers the commonest reading would pass it every time.
        if len(evidence) < 2:
            continue
        for reading, hits in sorted(evidence.items()):
            for japanese, english in hits[:PER_HETERONYM]:
                picked.append((term, reading, japanese, english))

    picked.sort()
    covered = len({term for term, _, _, _ in picked})
    print(f"# {len(picked)} sentences across {covered} spellings")
    print("# Each line: the dictionary filed this sentence under this spelling AND this reading,")
    print("# on a JMdict sequence that no other reading of the spelling shares.")
    print("# Read the English before keeping one. It is the evidence that the sentence really is")
    print("# that word, and it is the only check this script does not do for you.")
    print()
    for term, reading, japanese, english in picked:
        print(f"{term}	{reading}")
        print(f"    JA  {japanese}")
        print(f"    EN  {english}")

    blind_spots(forms, {term for term, _, _, _ in picked})


def blind_spots(forms, covered):
    """Contested spellings the fixture cannot reach, commonest first.

    **Coverage here is an accident of the corpus, not a choice.** A spelling only enters the
    fixture if Jitendex happens to carry example sentences under two of its sequences, which has
    nothing to do with how often a learner meets it. So the fixture's silence about a word is not
    evidence the app reads it correctly — and without this list there is no way to tell the two
    apart.

    Ranked by jpdb frequency, which the app already installs and already shows. Skipped entirely
    when it is not there; the harvest is still valid, only unranked.
    """
    ranks = frequency_ranks()
    print()
    print("#" * 70)
    if not ranks:
        print("# No frequency bank installed, so blind spots cannot be ranked.")
        return

    gaps = []
    for term, sequences in forms.items():
        if term in covered or term not in ranks:
            continue
        readings = {slot["reading"] for slot in sequences.values() if slot["reading"]}
        if len(readings) < 2:
            continue
        gaps.append((ranks[term], term, sorted(readings)))
    gaps.sort()

    print(f"# BLIND SPOTS — {len(gaps)} contested spellings with no usable example sentences.")
    print("# These are NOT measured by anything. The fixture's silence about them is the corpus")
    print("# running out, not the app getting them right.")
    print(f"# The {BLIND_SPOTS} commonest, by jpdb rank:")
    for rank, term, readings in gaps[:BLIND_SPOTS]:
        print(f"#   {rank:>6}  {term}	{' / '.join(readings)}")


def main():
    if "--particle" in sys.argv:
        harvest_particles()
        return
    if "--heteronym" in sys.argv:
        harvest_heteronyms()
        return

    sentences = corpus()
    excluded = pack_examples()
    overlap = set(sentences) & excluded

    print(f"# corpus: {len(sentences)} native sentences")
    print(f"# pack examples: {len(excluded)}")
    print(f"# overlap (excluded from candidates): {len(overlap)}")
    print()

    by_level = collections.defaultdict(list)
    for pattern_id, level, written, title, surfaces in packs():
        hits = []
        for japanese, english in sentences.items():
            if japanese in excluded or len(japanese) > MAX_LEN:
                continue
            # *All* the surfaces, not any of them.
            #
            # A discontinuous pattern like だれも〜ません is spelled with two pieces that
            # sit apart in the sentence. Matching either one alone returns every sentence
            # ending in ません, none of which has だれも in it — which is what the first
            # run of this script did, and it made those patterns look unusable when the
            # search was simply asking the wrong question.
            if all(surface in japanese for surface in surfaces):
                hits.append((japanese, english))
                if len(hits) >= PER_PATTERN:
                    break
        if hits:
            by_level[level].append((pattern_id, written, title, surfaces, hits))

    for level in ("N5", "N4", "N3", "N2", "N1"):
        found = by_level.get(level, [])
        print(f"{'=' * 70}")
        print(f"{level} — {len(found)} patterns with at least one native sentence")
        print(f"{'=' * 70}")
        for pattern_id, written, title, surfaces, hits in found:
            print(f"\n{pattern_id}")
            print(f"   pattern: {written}")
            print(f"   means:   {title}")
            print(f"   found by: {'/'.join(surfaces)}")
            for japanese, english in hits:
                print(f"    JA  {japanese}")
                print(f"    EN  {english}")


if __name__ == "__main__":
    main()
