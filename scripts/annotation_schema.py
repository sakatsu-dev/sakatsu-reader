"""Reading/annotation records, schema v2: source-specific evidence and honest spans.

    from annotation_schema import validate, verify, scorable, from_v1

`corpus/reading-gold.json` is schema 1 and is **not migrated in place**. Conversion writes a
separate validated output and leaves the original alone, so `reading_pipeline.rs`'s v1 consumer
keeps passing unchanged and no test can migrate a fixture as a side effect.

# Why v2 exists

v1 answers one question — what does this span read — about one kind of source, a capture with an
image on disk. Three things it cannot express are now needed.

**Different sources prove themselves differently.** v1's consumer demands a local image whose
SHA-256 matches, which is exactly right for capture evidence and impossible for a chat utterance
that has no image at all. The answer is not to drop the requirement but to make it specific.

**The text an annotation indexes is not always the text an engine was given.** Capture transcripts
preserve visual line wrapping: a newline may split a word across rendered lines, separate a speaker
from their line, or end a sentence, and those want opposite treatment. A blanket strip makes the
analyzed text differ from the transcript, and then an exact UTF-8 span means different things
depending on which one you hold. So both texts are stored, with the transformations between them
and an offset map, and every span says **which text it indexes**.

**Creation, verification and review are three states, not one.** v1's `source_verified` says a
transcription was checked against its source. It does not say who wrote the transcript, and it is
not independent human adjudication of a *reading*.

# Structure and verification are separate, and only one of them works offline

[`validate`] is **structural**: internal consistency, offsets, payloads, states. It needs nothing
but the document and it always runs.

[`verify`] is **evidential**: it resolves each case against the artefact it claims — the image on
disk, or the pinned chat record — and returns [`VERIFIED`], [`INCOMPLETE`] or [`FAILED`] per case.

Keeping them apart matters because the first version of this conflated them, and a capture case
validated happily against a corpus directory that did not exist: absent evidence read as successful
verification, which is the one answer verification must never give. **Missing evidence is
[`INCOMPLETE`], never verified.**

# Scoring fails closed, and names its basis

[`scorable`] takes a **required** policy and a **required** verification map. Every gap found in
this file so far was a default that failed open — a filter applied *only if supplied* admits
whatever the caller forgot — so there are no optional filters left: omitting either argument raises
rather than scoring. [`conclusive`] is the ungated view, named differently so the difference is
visible where it is called.
"""

import copy
import hashlib
import json
import pathlib

SCHEMA = 2

SOURCE_KINDS = ("capture", "chat")
SOURCE_REQUIRED = {
    "capture": ("image", "image_sha256", "transcript_tier"),
    "chat": ("dataset", "revision", "dialogue_id", "utterance_id"),
}

JUDGMENT_TYPES = (
    "reading",
    "morphological_boundary",
    "learner_grouping",
    "construction_explanation",
    "lookup",
)

#: A judgment somebody proposed and nobody has adjudicated.
PROPOSED = "proposed"
#: Adjudicated and affirmed.
ACCEPTED = "accepted"
#: Adjudicated and rejected.
REJECTED = "rejected"
#: **The reviewer looked and there is nothing of this type here.** Negative linguistic evidence,
#: and usable as such — a capture with no construction in it is a fact about the capture.
REVIEWED_ABSENT = "reviewed_absent"
#: **The reviewer could not determine the answer.** Review activity, *not* negative evidence: it
#: says something about the question's difficulty and nothing about the language.
UNRESOLVED = "unresolved"
REVIEW_STATES = (PROPOSED, ACCEPTED, REJECTED, REVIEWED_ABSENT, UNRESOLVED)

#: States that assert something a score may count.
CONCLUSIVE_STATES = (ACCEPTED, REVIEWED_ABSENT)

BREAK_CERTAINTY = ("wrap", "structural", "uncertain")

VERIFIED = "verified"
INCOMPLETE = "incomplete"
FAILED = "failed"

#: The cohort v1's reading gold converts into. Kept separate so it is never silently blended with
#: independently reviewed judgments — combining them has to be an explicit request.
V1_COHORT = "v1-source-ruby"

#: The blinded hybrid review: **one reader's usability preferences**, not adjudicated Japanese.
#:
#: The reader was asked which of two analyses reads better, over twelve cases that had already been
#: inspected during development. Those answers are real evidence about usefulness and no evidence at
#: all about which analysis is *correct* — the same review accepted a reading both engines agreed on
#: and neither established.
#:
#: Excluded by identity for the reason [`V1_COHORT`] is: a rule phrased as "requires human review"
#: would admit these, because a human did review them. What disqualifies them is what they are, and
#: that does not change if review records are added later.
HYBRID_USABILITY_REVIEW = "hybrid-usability-review"


class Invalid(Exception):
    """A record that does not validate. The message names the case and the rule."""


def digest(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _is_boundary(raw, index):
    """Whether `index` is a character boundary of the UTF-8 bytes `raw`."""
    if index == 0 or index == len(raw):
        return True
    if not 0 < index < len(raw):
        return False
    return (raw[index] & 0xC0) != 0x80


def apply_transformations(original, transformations):
    """`(analysis_text, offset_map)` for `original` under `transformations`.

    Each transformation replaces `original[start:end]` — **byte** offsets — with `replacement`.

    **Boundaries are checked before anything is applied, not inferred from the result.** Decoding
    the output successfully proves nothing: deleting bytes 2..5 of あい leaves E3 81 84, which is a
    perfectly valid い and an edit that cut both characters in half. Every offset must land on a
    character boundary of the original.
    """
    raw = original.encode("utf-8")
    edits = []
    for edit in transformations:
        start, end = edit.get("original_start"), edit.get("original_end")
        if not isinstance(start, int) or not isinstance(end, int):
            raise Invalid("transformation offsets must be integers")
        if isinstance(start, bool) or isinstance(end, bool):
            raise Invalid("transformation offsets must be integers")
        if start < 0 or end < 0:
            raise Invalid("transformation offsets must not be negative")
        if end < start:
            raise Invalid("transformation ends before it starts")
        if end > len(raw):
            raise Invalid("transformation runs past the end of the original")
        if not _is_boundary(raw, start) or not _is_boundary(raw, end):
            raise Invalid(
                f"transformation {start}..{end} does not land on character boundaries"
            )
        edits.append(edit)

    edits.sort(key=lambda edit: (edit["original_start"], edit["original_end"]))
    last_end = 0
    for edit in edits:
        if edit["original_start"] < last_end:
            raise Invalid("transformations overlap")
        last_end = edit["original_end"]

    out = bytearray()
    offsets = [None] * len(raw)
    cursor = 0
    for edit in edits:
        start, end = edit["original_start"], edit["original_end"]
        for index in range(cursor, start):
            offsets[index] = len(out)
            out.append(raw[index])
        out.extend(edit.get("replacement", "").encode("utf-8"))
        cursor = end
    for index in range(cursor, len(raw)):
        offsets[index] = len(out)
        out.append(raw[index])
    return out.decode("utf-8"), offsets


def _require(case, condition, message):
    if not condition:
        raise Invalid(f"{case.get('id', '<no id>')}: {message}")


def validate_text(case):
    """The two texts, the transformations between them, and the offset map."""
    text = case.get("text")
    _require(case, isinstance(text, dict), "no text record")
    _require(case, isinstance(text.get("original"), str), "no original transcript")
    transformations = text.get("transformations", [])
    _require(case, isinstance(transformations, list), "transformations must be a list")
    for edit in transformations:
        _require(
            case,
            edit.get("certainty") in BREAK_CERTAINTY,
            f"a transformation's certainty must be one of {BREAK_CERTAINTY}",
        )
    try:
        derived, offsets = apply_transformations(text["original"], transformations)
    except Invalid as error:
        raise Invalid(f"{case.get('id', '<no id>')}: {error}") from error
    _require(
        case,
        derived == text.get("analysis", text["original"]),
        "the stored analysis text is not what the transformations produce from the original",
    )
    if "original_sha256" in text:
        _require(
            case,
            text["original_sha256"] == digest(text["original"]),
            "the transcript does not match its own recorded digest",
        )
    return offsets


def validate_source(case):
    """**Structural** source checks only. Resolving the artefact is [`verify`]'s job."""
    source = case.get("source")
    _require(case, isinstance(source, dict), "no source record")
    kind = source.get("kind")
    _require(case, kind in SOURCE_KINDS, f"source kind {kind!r} is not one of {SOURCE_KINDS}")
    for field in SOURCE_REQUIRED[kind]:
        _require(case, source.get(field), f"{kind} source is missing {field!r}")
    if kind == "capture":
        _require(
            case,
            "/" not in source["image"] and "\\" not in source["image"],
            "the image must be a bare local filename",
        )
        _require(case, "dataset" not in source, "a capture source must not carry a dataset revision")
    else:
        _require(case, "image" not in source, "a chat source has no image and must not claim one")
        # The self-digest is internal consistency, and deliberately **not** called verification:
        # a text agreeing with a hash computed from itself says nothing about which utterance of
        # which revision it came from. `verify` answers that against the pinned record.
        _require(
            case,
            "original_sha256" in case.get("text", {}),
            "a chat case must record its text digest",
        )


def _span_text(case, span):
    text = case["text"]
    return text["original"] if span["on"] == "original" else text.get("analysis", text["original"])


def validate_span(case, offsets):
    """A span names which text it indexes, and survives intact into the other one."""
    span = case.get("span")
    _require(case, isinstance(span, dict), "no span")
    _require(case, span.get("on") in ("original", "analysis"), "a span must say which text it indexes")
    raw = _span_text(case, span).encode("utf-8")
    start, end = span.get("start"), span.get("end")
    _require(
        case,
        isinstance(start, int) and isinstance(end, int)
        and not isinstance(start, bool) and not isinstance(end, bool),
        "span offsets must be integers",
    )
    _require(case, 0 <= start < end <= len(raw), "the span is outside the text it indexes")
    _require(
        case,
        _is_boundary(raw, start) and _is_boundary(raw, end),
        "the span does not land on character boundaries",
    )
    _require(
        case,
        raw[start:end].decode("utf-8") == span.get("surface"),
        "the span does not name the text it points at",
    )
    if span["on"] == "original" and case["text"].get("transformations"):
        mapped = [offsets[index] for index in range(start, end)]
        _require(
            case,
            all(offset is not None for offset in mapped),
            "the span covers text the transformations removed, so the engine never analyzed it",
        )
        # **Contiguity, not merely survival.** An insertion inside the span leaves every byte
        # mapped and the span itself split in two, which no single analysis offset describes.
        _require(
            case,
            mapped == list(range(mapped[0], mapped[0] + len(mapped))),
            "the transformations split this span in the analysis text; it has no single span there",
        )


def validate_provenance(case):
    provenance = case.get("provenance")
    _require(case, isinstance(provenance, dict), "no provenance record")
    _require(
        case,
        "creation_method" in provenance,
        "creation_method must be stated, and 'unknown' is a legitimate value",
    )
    _require(case, "human_review" in provenance, "human_review must be stated, even as 'pending'")


def _spans_within(case, judgment, field):
    """Every `[start, end]` in `judgment[field]` must index the case's own text."""
    pairs = judgment.get(field)
    _require(case, isinstance(pairs, list) and pairs, f"a {judgment['type']} needs a non-empty {field!r}")
    on = judgment.get("on", case["span"]["on"])
    _require(case, on in ("original", "analysis"), f"{field!r} must say which text it indexes")
    raw = _span_text(case, {"on": on}).encode("utf-8")
    for pair in pairs:
        _require(
            case,
            isinstance(pair, (list, tuple)) and len(pair) == 2,
            f"each entry of {field!r} is a [start, end] pair",
        )
        start, end = pair
        _require(
            case,
            isinstance(start, int) and isinstance(end, int) and 0 <= start < end <= len(raw),
            f"{field!r} entry {pair} is outside the text it indexes",
        )
        _require(
            case,
            _is_boundary(raw, start) and _is_boundary(raw, end),
            f"{field!r} entry {pair} does not land on character boundaries",
        )


def validate_judgments(case):
    """Five type names are not five contracts until each one validates its own payload."""
    judgments = case.get("judgments")
    _require(case, isinstance(judgments, list) and judgments, "a case with no judgment asserts nothing")
    for judgment in judgments:
        kind = judgment.get("type")
        _require(case, kind in JUDGMENT_TYPES, f"judgment type {kind!r} is not one of {JUDGMENT_TYPES}")
        state = judgment.get("state")
        _require(case, state in REVIEW_STATES, f"review state {state!r} is not one of {REVIEW_STATES}")
        _require(case, judgment.get("evidence"), "a judgment must say what it rests on")
        if state in CONCLUSIVE_STATES:
            _require(case, judgment.get("reviewer"), f"a {state} judgment must name who decided it")
            # A review record has to be able to say *which* judgments it covered, and it cannot do
            # that if they have no names.
            _require(
                case,
                isinstance(judgment.get("id"), str) and judgment["id"],
                f"a {state} judgment needs an id so a review can identify it",
            )

        # A judgment that asserts nothing needs no payload; one that asserts something must carry
        # it. This is the gap that let a boundary judgment be accepted with no boundaries in it.
        if state != ACCEPTED:
            continue
        if kind == "reading":
            value = judgment.get("value")
            _require(
                case,
                isinstance(value, list) and value and all(isinstance(v, str) and v for v in value),
                "an accepted reading carries a non-empty list of admissible readings",
            )
        elif kind == "morphological_boundary":
            _spans_within(case, judgment, "boundaries")
        elif kind == "learner_grouping":
            _spans_within(case, judgment, "groups")
        elif kind == "construction_explanation":
            _require(
                case,
                isinstance(judgment.get("construction"), str) and judgment["construction"],
                "an accepted construction explanation must name the construction",
            )
        elif kind == "lookup":
            _require(
                case,
                isinstance(judgment.get("expected"), dict) and judgment["expected"],
                "an accepted lookup judgment must state what lookup should return",
            )


def validate(document):
    """**Structural** validation. Offline, total, and never a statement about evidence."""
    if document.get("schema") != SCHEMA:
        raise Invalid(f"schema {document.get('schema')!r} is not {SCHEMA}")
    seen = set()
    for case in document.get("cases", []):
        identity = case.get("id")
        if not identity:
            raise Invalid("a case has no id")
        if identity in seen:
            raise Invalid(f"{identity}: recorded twice")
        seen.add(identity)
        offsets = validate_text(case)
        validate_source(case)
        validate_span(case, offsets)
        validate_provenance(case)
        validate_judgments(case)
    return document


def verify_case(case, corpus_dir=None, chat_records=None):
    """Resolve one case against the artefact it claims. `(status, reason)`.

    **Absent evidence is [`INCOMPLETE`]**, never [`VERIFIED`]. That distinction is the whole point:
    an earlier version skipped the check when the image was missing, so a case validated against a
    corpus directory that did not exist.
    """
    source = case["source"]
    if source["kind"] == "capture":
        if corpus_dir is None:
            return INCOMPLETE, "no corpus directory supplied"
        directory = pathlib.Path(corpus_dir)
        if not directory.is_dir():
            return INCOMPLETE, f"corpus directory {directory} does not exist"
        path = directory / source["image"]
        if not path.is_file():
            return INCOMPLETE, f"{source['image']} is not present"
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != source["image_sha256"]:
            return FAILED, "the image changed since it was reviewed"
        return VERIFIED, "image resolved and hash matched"

    if chat_records is None:
        return INCOMPLETE, "no pinned chat records supplied"
    key = (source["dataset"], source["revision"], source["dialogue_id"], source["utterance_id"])
    record = chat_records.get(key)
    if record is None:
        return INCOMPLETE, f"no pinned record for {key}"
    if record != case["text"]["original"]:
        return FAILED, "the pinned record does not carry this text"
    return VERIFIED, "utterance resolved in the pinned revision"


def verify(document, corpus_dir=None, chat_records=None):
    """`{case_id: (status, reason)}` for every case."""
    return {
        case["id"]: verify_case(case, corpus_dir, chat_records)
        for case in document.get("cases", [])
    }


def _rests_on_uncertainty(case):
    return any(
        edit.get("certainty") == "uncertain"
        for edit in case.get("text", {}).get("transformations", [])
    )


def completed_review(record):
    """Whether a `human_review` record describes a review that actually happened.

    **An empty object is not a review**, and neither is the string `pending`. A completed record
    names a reviewer, carries the date the decision was made, and says **which judgments it
    covered** — because "this case was reviewed" does not establish that the judgment being scored
    was one of the things looked at.
    """
    if not isinstance(record, dict):
        return False
    if not isinstance(record.get("by"), str) or not record["by"].strip():
        return False
    if not isinstance(record.get("at"), str) or not record["at"].strip():
        return False
    covered = record.get("judgments")
    return isinstance(covered, list) and bool(covered)


class Policy:
    """A named basis for scoring. Scoring without naming one is not possible.

    Different evidence is admissible for different questions, and the way that goes wrong is a
    default: a gate that filters *only if asked* silently admits everything nobody thought to
    exclude. So every rule here is stated by the policy and none of them is optional at the call
    site.

    `excluded_cohorts` is deliberately separate from `cohorts`. Excluding the historical benchmark
    by *identity* keeps it excluded from independent-review scoring **even if its provenance later
    changes** — if somebody adds human-review records to those cases, they still do not become
    independent review, and a rule phrased as "requires human review" would have quietly admitted
    them.
    """

    def __init__(
        self,
        name,
        *,
        cohorts=None,
        excluded_cohorts=(),
        require_human_review=True,
        allow_uncertain_text=False,
    ):
        self.name = name
        self.cohorts = None if cohorts is None else frozenset(cohorts)
        self.excluded_cohorts = frozenset(excluded_cohorts)
        self.require_human_review = require_human_review
        self.allow_uncertain_text = allow_uncertain_text

    def __repr__(self):
        return f"Policy({self.name!r})"


#: Judgments a person adjudicated. Excludes by identity, not by symptom.
INDEPENDENT_REVIEW = Policy(
    "independent-review",
    excluded_cohorts=(V1_COHORT, HYBRID_USABILITY_REVIEW),
    require_human_review=True,
)

#: The historical source-ruby benchmark, on its own terms. It has **no** human review and inventing
#: one to satisfy a gate would be worse than admitting it has none — so this policy does not ask for
#: it, and in exchange it can score nothing else.
HISTORICAL_SOURCE_RUBY = Policy(
    "historical-source-ruby",
    cohorts=(V1_COHORT,),
    require_human_review=False,
)

#: The hybrid review on its own terms: usability preferences, scored as nothing else.
#:
#: It exists so the answers can be *reported* under a named policy rather than floating free, while
#: `scorable(..., INDEPENDENT_REVIEW, ...)` continues to return none of them.
HYBRID_USABILITY = Policy(
    "hybrid-usability",
    cohorts=(HYBRID_USABILITY_REVIEW,),
    require_human_review=False,
)

POLICIES = {
    policy.name: policy
    for policy in (INDEPENDENT_REVIEW, HISTORICAL_SOURCE_RUBY, HYBRID_USABILITY)
}


def conclusive(document, kind):
    """Every conclusive judgment of `kind`, **ungated**. Inspection, never scoring.

    Separate from [`scorable`] and named so the difference is visible at the call site: this is for
    looking at what a document contains, and it applies no policy at all.
    """
    return [
        (case, judgment)
        for case in document.get("cases", [])
        for judgment in case.get("judgments", [])
        if judgment.get("type") == kind and judgment.get("state") in CONCLUSIVE_STATES
    ]


def scorable(document, kind, policy, verification):
    """The judgments a score may be computed over, under a named policy and against verification.

    **Both `policy` and `verification` are required, and there are no defaults.** Every gap closed
    here was a default that failed open — a filter applied *only if supplied* admits whatever the
    caller forgot. Passing `None` for either raises rather than scoring.

    A judgment is scorable when all of these hold:

    - its state is **conclusive** — [`ACCEPTED`], or [`REVIEWED_ABSENT`] where the question is
      whether anything is there;
    - its case is [`VERIFIED`] in `verification` — `incomplete` is not a pass, and a case absent
      from the map is not either;
    - the policy admits the case's cohort, and does not exclude it by identity;
    - where the policy requires review, [`completed_review`] holds **and names this judgment**;
    - the analysis text does not rest on an `uncertain` transformation, unless the policy says so.
    """
    if policy is None:
        raise Invalid("scoring requires a named policy; see POLICIES")
    if isinstance(policy, str):
        if policy not in POLICIES:
            raise Invalid(f"unknown scoring policy {policy!r}; known: {sorted(POLICIES)}")
        policy = POLICIES[policy]
    if verification is None:
        raise Invalid(
            "scoring requires a verification map; run verify() and pass it. Unverified evidence "
            "is not scoreable, and absent verification is not a pass"
        )

    out = []
    for case in document.get("cases", []):
        cohort = case.get("cohort")
        if cohort in policy.excluded_cohorts:
            continue
        if policy.cohorts is not None and cohort not in policy.cohorts:
            continue
        if verification.get(case["id"], (None, None))[0] != VERIFIED:
            continue
        if not policy.allow_uncertain_text and _rests_on_uncertainty(case):
            continue
        review = case.get("provenance", {}).get("human_review")
        reviewed = completed_review(review)
        if policy.require_human_review and not reviewed:
            continue
        for judgment in case.get("judgments", []):
            if judgment.get("type") != kind or judgment.get("state") not in CONCLUSIVE_STATES:
                continue
            if policy.require_human_review and judgment.get("id") not in review["judgments"]:
                continue
            out.append((case, judgment))
    return out


def read(path):
    return validate(json.loads(pathlib.Path(path).read_text(encoding="utf-8")))


def from_v1(document):
    """A schema-1 reading-gold document as schema 2, without changing what it claims.

    **`source_verified` becomes source verification, not human review.** In v1 it records that a
    transcription was checked against its image — which is what its own `reviewer` field describes,
    "visual source transcription". It is not independent adjudication of the reading.

    **A v1 status that is not `source_verified` is preserved, not promoted.** The first version
    hardcoded `accepted`, so a case marked `unresolved` came out of conversion as gold.

    Every converted case is tagged [`V1_COHORT`] and left `human_review: pending`. Independent
    review scoring excludes that cohort **by identity**, so these stay out of it even if somebody
    later attaches review records to them; reaching them at all requires asking for
    [`HISTORICAL_SOURCE_RUBY`] by name, which scores nothing else.
    """
    if document.get("schema") != 1:
        raise Invalid(f"not a schema-1 document: {document.get('schema')!r}")
    cases = []
    for case in document.get("cases", []):
        state = ACCEPTED if case.get("status") == "source_verified" else UNRESOLVED
        judgment = {
            "id": f"{case['id']}:reading",
            "type": "reading",
            "value": list(case["accepted"]),
            "state": state,
            "evidence": case["evidence"],
            "reviewer": case.get("reviewer"),
            "reading_kind": case.get("reading_kind"),
            "v1_status": case.get("status"),
        }
        cases.append(
            {
                "id": case["id"],
                "cohort": V1_COHORT,
                "source": {
                    "kind": "capture",
                    "image": case["source_image"],
                    "image_sha256": case["source_sha256"],
                    # v1 predates the tier distinction; it cannot be recovered from the file.
                    "transcript_tier": "unrecorded",
                },
                "text": {
                    "original": case["text"],
                    "original_sha256": digest(case["text"]),
                    "analysis": case["text"],
                    "transformations": [],
                },
                "span": {
                    "on": "analysis",
                    "start": case["start"],
                    "end": case["end"],
                    "surface": case["surface"],
                },
                "provenance": {
                    "creation_method": "unknown",
                    "source_verification": {
                        "by": case.get("reviewer"),
                        "note": case.get("note"),
                    },
                    "human_review": "pending",
                },
                "judgments": [judgment],
                "partition": {"group": case.get("group"), "split": case.get("split")},
            }
        )
    return {
        "schema": SCHEMA,
        "licence": document.get("licence"),
        "cases": cases,
        "pending_sources": copy.deepcopy(document.get("pending_sources", [])),
    }


def line_break_transformations(original, certainty="uncertain"):
    """One transformation per newline, so every break is an explicit, reviewable decision.

    Deliberately **not** a blanket strip. A newline in a capture transcript may wrap a word across
    rendered lines, separate a speaker's name from their line, or end a sentence, and those want
    different treatment. This proposes the edits with the certainty the caller states; it does not
    decide which kind each one is.
    """
    raw = original.encode("utf-8")
    return [
        {
            "original_start": index,
            "original_end": index + 1,
            "replacement": "",
            "kind": "line-break",
            "certainty": certainty,
        }
        for index, byte in enumerate(raw)
        if byte == 0x0A
    ]
