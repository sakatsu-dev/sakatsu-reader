"""Synthetic schema checks; no corpus, images or network are needed.

The one test that touches `corpus/reading-gold.json` skips cleanly when it is absent, because it is
private data that most checkouts will not have.
"""
import importlib.util
import json
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "annotation_schema", pathlib.Path(__file__).with_name("annotation_schema.py")
)
schema = importlib.util.module_from_spec(spec)
spec.loader.exec_module(schema)

ROOT = pathlib.Path(__file__).resolve().parent.parent


def capture_case(**overrides):
    case = {
        "id": "app_A:0",
        "source": {
            "kind": "capture",
            "image": "app_A.png",
            "image_sha256": "0" * 64,
            "transcript_tier": ".txt",
        },
        "text": {"original": "誰が来た", "analysis": "誰が来た", "transformations": []},
        "span": {"on": "analysis", "start": 0, "end": 3, "surface": "誰"},
        "provenance": {"creation_method": "unknown", "human_review": "pending"},
        "judgments": [
            {
                "type": "reading",
                "value": ["だれ"],
                "state": "proposed",
                "evidence": "printed_ruby",
            }
        ],
    }
    case.update(overrides)
    return case


def document(*cases):
    return {"schema": 2, "licence": "test", "cases": list(cases)}


def reviewed_case(**overrides):
    """A case that would score: conclusive, identified, and covered by a complete review."""
    case = capture_case(**overrides)
    judgment_id = f"{case['id']}:r"
    case["judgments"][0].update(id=judgment_id, state="accepted", reviewer="a person")
    case["provenance"]["human_review"] = {
        "by": "a person",
        "at": "2026-09-13",
        "judgments": [judgment_id],
    }
    return case


def verified(doc):
    """A verification map saying every case resolved."""
    return {case["id"]: (schema.VERIFIED, "resolved") for case in doc["cases"]}


class Transformations(unittest.TestCase):
    def test_the_identity_transformation_maps_every_byte_to_itself(self):
        text, offsets = schema.apply_transformations("誰が来た", [])
        self.assertEqual(text, "誰が来た")
        self.assertEqual(offsets, list(range(len("誰が来た".encode("utf-8")))))

    def test_a_removed_newline_shifts_everything_after_it(self):
        original = "上\nがる"
        edits = schema.line_break_transformations(original, "wrap")
        text, offsets = schema.apply_transformations(original, edits)
        self.assertEqual(text, "上がる")
        # The newline itself is gone…
        self.assertIsNone(offsets[len("上".encode("utf-8"))])
        # …and the byte after it lands where the newline was.
        after = len("上\n".encode("utf-8"))
        self.assertEqual(offsets[after], len("上".encode("utf-8")))

    def test_an_edit_crossing_character_boundaries_is_refused_before_it_is_applied(self):
        """**The gap this closes.** Decoding the result proves nothing: deleting bytes 2..5 of
        あい leaves E3 81 84, a perfectly valid い produced by cutting both characters in half."""
        with self.assertRaises(schema.Invalid):
            schema.apply_transformations(
                "あい",
                [{"original_start": 2, "original_end": 5, "replacement": "", "certainty": "wrap"}],
            )

    def test_negative_and_non_integer_offsets_are_refused(self):
        for start, end in ((-1, 3), (0, -1), (1.5, 3), (True, 3)):
            with self.subTest(start=start, end=end), self.assertRaises(schema.Invalid):
                schema.apply_transformations(
                    "あい",
                    [
                        {
                            "original_start": start,
                            "original_end": end,
                            "replacement": "",
                            "certainty": "wrap",
                        }
                    ],
                )

    def test_overlapping_transformations_are_refused(self):
        with self.assertRaises(schema.Invalid):
            schema.apply_transformations(
                "あいうえお",
                [
                    {"original_start": 0, "original_end": 6, "replacement": ""},
                    {"original_start": 3, "original_end": 9, "replacement": ""},
                ],
            )

    def test_every_newline_becomes_its_own_reviewable_edit(self):
        """Not a blanket strip: a wrap, a speaker separator and a sentence end want different
        treatment, so each break is proposed individually with a stated certainty."""
        edits = schema.line_break_transformations("あ\nい\nう")
        self.assertEqual(len(edits), 2)
        self.assertTrue(all(edit["certainty"] == "uncertain" for edit in edits))


class SourceSpecificValidation(unittest.TestCase):
    def test_a_capture_keeps_the_image_requirement(self):
        case = capture_case()
        del case["source"]["image_sha256"]
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_an_image_must_be_a_bare_filename(self):
        case = capture_case()
        case["source"]["image"] = "../elsewhere/app_A.png"
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_a_changed_image_fails_verification(self):
        """The guarantee v1 has, kept rather than relaxed for uniformity."""
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            (pathlib.Path(tmp) / "app_A.png").write_bytes(b"different bytes")
            result = schema.verify(document(capture_case()), corpus_dir=tmp)
            self.assertEqual(result["app_A:0"][0], schema.FAILED)

    def test_a_missing_image_is_incomplete_and_never_verified(self):
        """**The gap this closes.** An earlier version skipped the check when the image was
        absent, so a case validated happily against a corpus directory that did not exist —
        missing evidence reading as successful verification, which is the one answer verification
        must never give."""
        import tempfile

        doc = document(capture_case())
        # No directory at all.
        self.assertEqual(schema.verify(doc)["app_A:0"][0], schema.INCOMPLETE)
        # A directory that does not exist.
        self.assertEqual(
            schema.verify(doc, corpus_dir="nowhere-at-all")["app_A:0"][0], schema.INCOMPLETE
        )
        # A directory that exists without the image in it.
        with tempfile.TemporaryDirectory() as tmp:
            status, reason = schema.verify(doc, corpus_dir=tmp)["app_A:0"]
            self.assertEqual(status, schema.INCOMPLETE)
            self.assertIn("not present", reason)

    def test_a_chat_case_is_verified_against_its_pinned_record_not_its_own_digest(self):
        """A text agreeing with a hash computed from itself says nothing about which utterance of
        which revision it came from."""
        text = "行くって言ってた"
        case = capture_case(
            id="rpc:1:9",
            source={
                "kind": "chat",
                "dataset": "real-persona-chat",
                "revision": "abc123",
                "dialogue_id": "d1",
                "utterance_id": "u2",
            },
            text={
                "original": text,
                "analysis": text,
                "transformations": [],
                "original_sha256": schema.digest(text),
            },
            span={"on": "analysis", "start": 0, "end": 6, "surface": "行く"},
        )
        doc = document(case)
        schema.validate(doc)
        key = ("real-persona-chat", "abc123", "d1", "u2")
        self.assertEqual(schema.verify(doc)["rpc:1:9"][0], schema.INCOMPLETE)
        self.assertEqual(
            schema.verify(doc, chat_records={key: text})["rpc:1:9"][0], schema.VERIFIED
        )
        self.assertEqual(
            schema.verify(doc, chat_records={key: "something else"})["rpc:1:9"][0], schema.FAILED
        )

    def test_a_chat_case_validates_its_pinned_record_instead_of_an_image(self):
        text = "行くって言ってた"
        case = capture_case(
            id="rpc:1:2",
            source={
                "kind": "chat",
                "dataset": "real-persona-chat",
                "revision": "abc123",
                "dialogue_id": "d1",
                "utterance_id": "u2",
            },
            text={
                "original": text,
                "analysis": text,
                "transformations": [],
                "original_sha256": schema.digest(text),
            },
            span={"on": "analysis", "start": 0, "end": 6, "surface": "行く"},
        )
        schema.validate(document(case))

    def test_a_chat_case_may_not_claim_an_image(self):
        text = "ねこ"
        case = capture_case(
            id="rpc:1:3",
            source={
                "kind": "chat",
                "dataset": "d",
                "revision": "r",
                "dialogue_id": "d1",
                "utterance_id": "u1",
                "image": "nope.png",
            },
            text={
                "original": text,
                "analysis": text,
                "transformations": [],
                "original_sha256": schema.digest(text),
            },
            span={"on": "analysis", "start": 0, "end": 6, "surface": "ねこ"},
        )
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_a_chat_text_must_match_its_own_digest(self):
        case = capture_case(
            id="rpc:1:4",
            source={
                "kind": "chat",
                "dataset": "d",
                "revision": "r",
                "dialogue_id": "d1",
                "utterance_id": "u1",
            },
            text={
                "original": "ねこ",
                "analysis": "ねこ",
                "transformations": [],
                "original_sha256": "0" * 64,
            },
            span={"on": "analysis", "start": 0, "end": 6, "surface": "ねこ"},
        )
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))


class Spans(unittest.TestCase):
    def test_a_span_must_say_which_text_it_indexes(self):
        case = capture_case()
        del case["span"]["on"]
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_a_span_must_name_the_text_it_points_at(self):
        case = capture_case()
        case["span"]["surface"] = "が"
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_a_span_off_a_character_boundary_is_refused(self):
        case = capture_case()
        case["span"] = {"on": "analysis", "start": 1, "end": 3, "surface": "誰"}
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_an_insertion_that_splits_a_span_is_refused(self):
        """Every byte survives and the span still has no single home in the analysis text."""
        case = capture_case(
            text={
                "original": "あい",
                "analysis": "あXXい",
                "transformations": [
                    {
                        "original_start": 3,
                        "original_end": 3,
                        "replacement": "XX",
                        "certainty": "wrap",
                    }
                ],
            },
            span={"on": "original", "start": 0, "end": 6, "surface": "あい"},
        )
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_a_span_on_the_original_may_not_cover_removed_text(self):
        """Otherwise the annotation points at characters the engine never analyzed."""
        original = "上\nがる"
        case = capture_case(
            text={
                "original": original,
                "analysis": "上がる",
                "transformations": schema.line_break_transformations(original, "wrap"),
            },
            span={"on": "original", "start": 0, "end": 4, "surface": "上\n"},
        )
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_the_stored_analysis_must_be_what_the_transformations_produce(self):
        original = "上\nがる"
        case = capture_case(
            text={
                "original": original,
                "analysis": "まったく別の文",
                "transformations": schema.line_break_transformations(original, "wrap"),
            },
            span={"on": "original", "start": 0, "end": 3, "surface": "上"},
        )
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))


class Judgments(unittest.TestCase):
    def test_a_case_with_no_judgment_asserts_nothing(self):
        case = capture_case(judgments=[])
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_an_accepted_judgment_must_name_its_reviewer(self):
        case = capture_case()
        case["judgments"][0]["state"] = "accepted"
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_an_accepted_boundary_judgment_must_carry_boundaries(self):
        """**The gap this closes.** Five type names were not yet five contracts: a boundary
        judgment could be accepted, and scored, with no boundaries in it."""
        case = capture_case()
        case["judgments"] = [
            {
                "type": "morphological_boundary",
                "state": "accepted",
                "evidence": "nothing at all",
                "reviewer": "somebody",
            }
        ]
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_each_type_validates_its_own_payload(self):
        good = {
            "morphological_boundary": {"boundaries": [[0, 3]]},
            "learner_grouping": {"groups": [[0, 6]]},
            "construction_explanation": {"construction": "explanatory-no-da"},
            "lookup": {"expected": {"available": True}},
            "reading": {"value": ["だれ"]},
        }
        for kind, payload in good.items():
            with self.subTest(kind=kind):
                case = capture_case()
                case["judgments"] = [
                    dict(
                        id="j1",
                        type=kind,
                        state="accepted",
                        evidence="reviewed",
                        reviewer="a person",
                        **payload,
                    )
                ]
                schema.validate(document(case))
                # …and the same judgment without its payload is refused.
                bare = capture_case()
                bare["judgments"] = [
                    {
                        "id": "j1",
                        "type": kind,
                        "state": "accepted",
                        "evidence": "reviewed",
                        "reviewer": "a person",
                    }
                ]
                with self.assertRaises(schema.Invalid):
                    schema.validate(document(bare))

    def test_a_boundary_outside_its_text_is_refused(self):
        case = capture_case()
        case["judgments"] = [
            {
                "id": "j1",
                "type": "morphological_boundary",
                "state": "accepted",
                "evidence": "reviewed",
                "reviewer": "a person",
                "boundaries": [[0, 999]],
            }
        ]
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_the_five_judgment_types_are_separate_records(self):
        """A reading string may not be overloaded to carry a boundary or a grouping."""
        case = capture_case()
        case["judgments"] = [
            {"type": kind, "state": "proposed", "evidence": "agent proposal"}
            for kind in ("morphological_boundary", "learner_grouping", "lookup")
        ]
        schema.validate(document(case))

    def test_scoring_without_a_verification_map_raises_rather_than_scoring(self):
        """**The gap this closes.** Verification was enforced only *if supplied*, so a caller who
        omitted it scored unverified evidence. Absent verification is not a pass."""
        doc = document(reviewed_case(id="a:0"))
        schema.validate(doc)
        with self.assertRaises(schema.Invalid):
            schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, None)

    def test_scoring_without_a_policy_raises_rather_than_scoring(self):
        doc = document(reviewed_case(id="a:0"))
        with self.assertRaises(schema.Invalid):
            schema.scorable(doc, "reading", None, verified(doc))
        with self.assertRaises(schema.Invalid):
            schema.scorable(doc, "reading", "no-such-policy", verified(doc))

    def test_an_empty_review_object_is_not_a_review(self):
        """**The gap this closes.** Human review meant only that the field was a dictionary."""
        case = reviewed_case(id="e:0")
        case["provenance"]["human_review"] = {}
        doc = document(case)
        schema.validate(doc)
        self.assertEqual(
            schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, verified(doc)), []
        )

    def test_a_review_missing_its_date_or_reviewer_is_incomplete(self):
        for record in (
            {"at": "2026-09-13", "judgments": ["e:0:r"]},
            {"by": "a person", "judgments": ["e:0:r"]},
            {"by": "   ", "at": "2026-09-13", "judgments": ["e:0:r"]},
            {"by": "a person", "at": "2026-09-13"},
            {"by": "a person", "at": "2026-09-13", "judgments": []},
        ):
            with self.subTest(record=record):
                case = reviewed_case(id="e:0")
                case["provenance"]["human_review"] = record
                doc = document(case)
                schema.validate(doc)
                self.assertEqual(
                    schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, verified(doc)), []
                )

    def test_a_review_must_name_the_judgment_being_scored(self):
        """"This case was reviewed" does not establish that this judgment was looked at."""
        case = reviewed_case(id="n:0")
        case["provenance"]["human_review"]["judgments"] = ["some-other-judgment"]
        doc = document(case)
        schema.validate(doc)
        self.assertEqual(
            schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, verified(doc)), []
        )

    def test_a_conclusive_judgment_needs_an_id_so_a_review_can_name_it(self):
        case = reviewed_case(id="i:0")
        del case["judgments"][0]["id"]
        with self.assertRaises(schema.Invalid):
            schema.validate(document(case))

    def test_scoring_requires_an_explicitly_named_cohort_policy(self):
        """**The gap this closes.** Cohort filtering applied only if requested, so historical and
        independently reviewed judgments scored together whenever nobody asked."""
        current = reviewed_case(id="cur:0")
        historical = reviewed_case(id="hist:0")
        historical["cohort"] = schema.V1_COHORT
        doc = document(current, historical)
        schema.validate(doc)
        check = verified(doc)

        independent = schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, check)
        self.assertEqual([c["id"] for c, _ in independent], ["cur:0"])

        historical_only = schema.scorable(doc, "reading", schema.HISTORICAL_SOURCE_RUBY, check)
        self.assertEqual([c["id"] for c, _ in historical_only], ["hist:0"])

    def test_the_historical_cohort_stays_excluded_even_if_it_gains_a_review(self):
        """Excluded by identity, not by symptom. A rule phrased as "requires human review" would
        admit these the moment somebody attached a review record."""
        historical = reviewed_case(id="hist:0")
        historical["cohort"] = schema.V1_COHORT
        doc = document(historical)
        self.assertEqual(
            schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, verified(doc)), []
        )

    def test_the_usability_review_never_scores_as_independent_review(self):
        """**A person really did review these**, which is exactly why the rule cannot be phrased as
        "requires human review". The reader was asked which analysis reads better, not which is
        correct, and the same review accepted a reading both engines agreed on and neither
        established. So the cohort is excluded by identity, and stays excluded with a reviewer
        attached."""
        usability = reviewed_case(id="hybrid:7")
        usability["cohort"] = schema.HYBRID_USABILITY_REVIEW
        doc = document(usability)
        schema.validate(doc)
        self.assertEqual(
            schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, verified(doc)),
            [],
            "usability preferences must not reach linguistic scoring",
        )

    def test_the_usability_review_is_still_reportable_under_its_own_policy(self):
        """Excluding it from linguistic scoring is not the same as discarding it. The answers are
        real evidence about usefulness and remain addressable under a named policy."""
        usability = reviewed_case(id="hybrid:7")
        usability["cohort"] = schema.HYBRID_USABILITY_REVIEW
        doc = document(usability)
        scored = schema.scorable(doc, "reading", schema.HYBRID_USABILITY, verified(doc))
        self.assertEqual([c["id"] for c, _ in scored], ["hybrid:7"])

    def test_the_two_excluded_cohorts_do_not_leak_into_each_other(self):
        """Each policy scores its own cohort and nothing else, so adding a second exclusion did not
        widen the first."""
        historical = reviewed_case(id="hist:0")
        historical["cohort"] = schema.V1_COHORT
        usability = reviewed_case(id="hybrid:7")
        usability["cohort"] = schema.HYBRID_USABILITY_REVIEW
        current = reviewed_case(id="cur:0")
        doc = document(historical, usability, current)
        schema.validate(doc)
        check = verified(doc)
        self.assertEqual(
            [c["id"] for c, _ in schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, check)],
            ["cur:0"],
        )
        self.assertEqual(
            [
                c["id"]
                for c, _ in schema.scorable(
                    doc, "reading", schema.HISTORICAL_SOURCE_RUBY, check
                )
            ],
            ["hist:0"],
        )
        self.assertEqual(
            [c["id"] for c, _ in schema.scorable(doc, "reading", schema.HYBRID_USABILITY, check)],
            ["hybrid:7"],
        )

    def test_the_historical_policy_does_not_demand_a_review_it_does_not_have(self):
        """Inventing one to satisfy a gate would be worse than admitting it has none."""
        historical = capture_case(id="hist:0")
        historical["cohort"] = schema.V1_COHORT
        historical["judgments"][0].update(id="hist:0:r", state="accepted", reviewer="an agent")
        doc = document(historical)
        schema.validate(doc)
        scored = schema.scorable(doc, "reading", schema.HISTORICAL_SOURCE_RUBY, verified(doc))
        self.assertEqual(len(scored), 1)

    def test_an_unverified_case_is_never_scorable(self):
        doc = document(reviewed_case(id="v:0"))
        for check in (
            {"v:0": (schema.INCOMPLETE, "no corpus directory supplied")},
            {"v:0": (schema.FAILED, "the image changed")},
            {},  # absent from the map entirely
        ):
            with self.subTest(check=check):
                self.assertEqual(
                    schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, check), []
                )

    def test_conclusive_is_the_ungated_view_and_says_so(self):
        doc = document(reviewed_case(id="c:0"))
        self.assertEqual(len(schema.conclusive(doc, "reading")), 1)
        self.assertEqual(
            schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, {}), []
        )

    def test_text_resting_on_an_uncertain_transformation_is_not_scorable(self):
        """What was analyzed depends on a question nobody has answered."""
        original = "上\nがる"
        case = reviewed_case(
            id="u:0",
            text={
                "original": original,
                "analysis": "上がる",
                "transformations": schema.line_break_transformations(original, "uncertain"),
            },
            span={"on": "analysis", "start": 0, "end": 3, "surface": "上"},
        )
        doc = document(case)
        schema.validate(doc)
        self.assertEqual(
            schema.scorable(doc, "reading", schema.INDEPENDENT_REVIEW, verified(doc)), []
        )

    def test_reviewed_absence_and_unresolved_are_different_outcomes(self):
        """One is negative linguistic evidence; the other is review activity and no evidence."""
        absent = capture_case(id="absent:0")
        absent["judgments"] = [
            {
                "id": "absent:0:c",
                "type": "construction_explanation",
                "state": schema.REVIEWED_ABSENT,
                "evidence": "read against the image",
                "reviewer": "a person",
            }
        ]
        absent["provenance"]["human_review"] = {
            "by": "a person",
            "at": "2026-09-13",
            "judgments": ["absent:0:c"],
        }
        unresolved = capture_case(id="unresolved:0")
        unresolved["judgments"] = [
            {
                "type": "construction_explanation",
                "state": schema.UNRESOLVED,
                "evidence": "read against the image",
            }
        ]
        unresolved["provenance"]["human_review"] = {
            "by": "a person",
            "at": "2026-09-13",
            "judgments": ["unresolved:0:c"],
        }
        doc = document(absent, unresolved)
        schema.validate(doc)
        scored = schema.scorable(
            doc, "construction_explanation", schema.INDEPENDENT_REVIEW, verified(doc)
        )
        self.assertEqual([c["id"] for c, _ in scored], ["absent:0"])

    def test_a_duplicate_case_id_is_refused(self):
        with self.assertRaises(schema.Invalid):
            schema.validate(document(capture_case(), capture_case()))


class V1Compatibility(unittest.TestCase):
    def v1(self):
        return {
            "schema": 1,
            "licence": "private",
            "cases": [
                {
                    "id": "firefox_X:0",
                    "source_image": "firefox_X.png",
                    "source_sha256": "a" * 64,
                    "group": "school_manga",
                    "split": "holdout",
                    "text": "誰が来た",
                    "surface": "誰",
                    "start": 0,
                    "end": 3,
                    "accepted": ["だれ"],
                    "reading_kind": "lexical",
                    "status": "source_verified",
                    "evidence": "printed_ruby",
                    "reviewer": "an agent, visual source transcription",
                    "note": "printed ruby; not an analyzer label",
                }
            ],
            "pending_sources": [{"source_image": "g.png", "status": "pending_review"}],
        }

    def test_a_v1_document_converts_and_validates(self):
        converted = schema.from_v1(self.v1())
        schema.validate(converted)
        self.assertEqual(converted["schema"], 2)

    def test_source_verified_becomes_verification_not_human_review(self):
        """v1's `source_verified` says a transcription was checked against its image. It is not
        independent adjudication of the reading, and promoting it would manufacture gold."""
        case = schema.from_v1(self.v1())["cases"][0]
        self.assertEqual(case["provenance"]["human_review"], "pending")
        self.assertIn("visual source transcription", case["provenance"]["source_verification"]["by"])

    def test_the_partition_survives_conversion(self):
        case = schema.from_v1(self.v1())["cases"][0]
        self.assertEqual(case["partition"], {"group": "school_manga", "split": "holdout"})

    def test_pending_sources_are_carried_not_promoted(self):
        converted = schema.from_v1(self.v1())
        self.assertEqual(len(converted["pending_sources"]), 1)
        self.assertEqual(converted["cases"][0]["judgments"][0]["state"], "accepted")

    def test_conversion_does_not_mutate_its_input(self):
        """Conversion writes a separate output; nothing migrates a fixture as a side effect."""
        original = self.v1()
        before = json.dumps(original, sort_keys=True)
        schema.from_v1(original)
        self.assertEqual(json.dumps(original, sort_keys=True), before)

    def test_the_real_v1_file_converts_when_it_is_present(self):
        path = ROOT / "corpus" / "reading-gold.json"
        if not path.exists():
            self.skipTest("no private capture gold in this checkout")
        v1 = json.loads(path.read_text(encoding="utf-8"))
        converted = schema.from_v1(v1)
        schema.validate(converted)
        self.assertEqual(len(converted["cases"]), len(v1["cases"]))
        self.assertTrue(
            all(case["provenance"]["human_review"] == "pending" for case in converted["cases"])
        )
        # Every image resolves and hashes as recorded.
        result = schema.verify(converted, corpus_dir=ROOT / "corpus")
        self.assertTrue(all(status == schema.VERIFIED for status, _ in result.values()))
        # None of it reaches independent-review scoring…
        self.assertEqual(
            schema.scorable(converted, "reading", schema.INDEPENDENT_REVIEW, result), []
        )
        # …and all of it is available through its own named policy, which asks for no review it
        # does not have.
        historical = schema.scorable(
            converted, "reading", schema.HISTORICAL_SOURCE_RUBY, result
        )
        self.assertEqual(len(historical), len(v1["cases"]))

    def test_a_v1_status_that_is_not_source_verified_is_not_promoted(self):
        """**The gap this closes.** Conversion hardcoded `accepted`, so a case marked
        `unresolved` came out of it as gold."""
        altered = self.v1()
        altered["cases"][0]["status"] = "unresolved"
        judgment = schema.from_v1(altered)["cases"][0]["judgments"][0]
        self.assertEqual(judgment["state"], schema.UNRESOLVED)
        self.assertEqual(judgment["v1_status"], "unresolved")


if __name__ == "__main__":
    unittest.main()
