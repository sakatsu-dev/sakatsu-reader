"""The parts of scripts/mt-metrics/score.py that decide things without a model.

No model, corpus or network: the control matcher, the tie band, the sign test and the flags are
plain functions, and a mistake in any of them would quietly change a verdict.
"""
import importlib.util
import json
import pathlib
import unittest

HERE = pathlib.Path(__file__).with_name("mt-metrics")
spec = importlib.util.spec_from_file_location("mt_score", HERE / "score.py")
score = importlib.util.module_from_spec(spec)
spec.loader.exec_module(score)


def item(*groups):
    return {"accept": [list(g) for g in groups]}


class Matcher(unittest.TestCase):
    """Whole words, every group, any phrase in a group."""

    def test_a_phrase_does_not_match_inside_a_longer_word(self):
        self.assertFalse(score.accepted(item(["no"]), "I know the answer."))
        self.assertTrue(score.accepted(item(["no"]), "There is no cure."))

    def test_an_optional_plural_is_still_a_whole_word(self):
        self.assertTrue(score.accepted(item(["pounds?"]), "It costs 20 pounds."))
        self.assertTrue(score.accepted(item(["pounds?"]), "a pound"))
        self.assertFalse(score.accepted(item(["pounds?"]), "it was impounded"))

    def test_a_symbol_matches_beside_a_digit(self):
        self.assertTrue(score.accepted(item(["100"], ["dollars?", "usd", r"\$"]), "It is $100 a night."))

    def test_every_group_must_match(self):
        twenty = item(["20", "twenty"], ["pounds?", "£"])
        self.assertTrue(score.accepted(twenty, "at least twenty pounds"))
        self.assertTrue(score.accepted(twenty, "at least £20"))
        self.assertFalse(score.accepted(twenty, "at least 20 dollars"))

    def test_a_curly_apostrophe_reads_as_a_straight_one(self):
        self.assertTrue(score.accepted(item(["don't"]), "Please don’t touch."))

    def test_case_is_ignored(self):
        self.assertTrue(score.accepted(item(["north sea"]), "in the North Sea"))


class Pairing(unittest.TestCase):
    def test_the_sign_test_is_exact(self):
        self.assertEqual(score.sign_test(0, 0), 1.0)
        self.assertAlmostEqual(score.sign_test(10, 0), 2 / 1024)
        self.assertAlmostEqual(score.sign_test(5, 5), 1.0)

    def test_metricx_is_an_error_so_lower_wins(self):
        self.assertEqual(score.better("metricx", 2.0, 4.0), 1)
        self.assertEqual(score.better("comet", 0.80, 0.70), 1)

    def test_a_gap_inside_the_tie_band_is_a_tie(self):
        self.assertEqual(score.better("comet", 0.805, 0.80), 0)
        self.assertEqual(score.better("metricx", 3.0, 3.2), 0)


class Flags(unittest.TestCase):
    def test_japanese_left_in_the_output_is_flagged(self):
        self.assertIn("japanese-left", score.flags("猫", "The 猫 sat."))
        self.assertEqual(score.flags("猫が座った。", "The cat sat."), [])

    def test_full_width_digits_and_thousands_separators_compare_as_numbers(self):
        self.assertEqual(score.flags("３０００円かかった。", "It cost 3,000 yen."), [])
        self.assertIn("digits-differ", score.flags("３０００円かかった。", "It cost 300 yen."))

    def test_a_number_spelled_out_is_flagged_for_reading_not_failed(self):
        # Flagged — someone reads it — and it is right; flags are never scored.
        self.assertIn("digits-differ", score.flags("６人", "for six"))


class Screening(unittest.TestCase):
    """The four cases a review caught the first control set getting wrong, pinned."""

    items = {i["index"]: i for i in json.loads((HERE / "controls.json").read_text(encoding="utf-8"))["items"]}

    def test_a_reversed_explanation_is_flagged(self):
        self.assertTrue(score.screen(self.items[32],
                                     "He came back because he was homesick, not because he was running short of money."))

    def test_advice_to_keep_drinking_is_flagged(self):
        self.assertTrue(score.screen(self.items[260], "He advised her to keep drinking."))

    def test_two_hundred_is_not_one_hundred(self):
        self.assertTrue(score.screen(self.items[265], "It is two hundred dollars a night plus tax."))
        self.assertFalse(score.screen(self.items[265], "It is a hundred dollars a night plus tax."))

    def test_a_correct_rewording_of_did_not_break_passes(self):
        self.assertFalse(score.screen(self.items[136], "The branch bent but remained intact."))

    def test_a_reject_phrase_flags_even_when_every_group_matches(self):
        entry = {"accept": [["advised her"]], "reject": ["keep drinking"]}
        self.assertEqual(score.screen(entry, "He advised her to keep drinking."), ["contains (keep drinking)"])

    def test_every_written_alternative_passes_its_own_screen(self):
        for entry in self.items.values():
            for alternative in entry.get("alternatives", []):
                self.assertFalse(score.screen(entry, alternative), (entry["index"], alternative))


class Approval(unittest.TestCase):
    """Only an approval naming the current content unlocks scoring."""

    def run_with_digest(self, reviewed, current="abc"):
        original = score.approval_digest
        score.approval_digest = lambda controls: current
        try:
            score.require_approval({"reviewed": reviewed})
        finally:
            score.approval_digest = original

    def test_a_bare_truthy_value_is_not_an_approval(self):
        with self.assertRaises(SystemExit):
            self.run_with_digest(True)

    def test_an_approval_of_other_content_is_refused(self):
        with self.assertRaises(SystemExit):
            self.run_with_digest({"by": "x", "on": "2026-10-10", "digest": "old"})

    def test_an_approval_of_this_content_passes(self):
        self.run_with_digest({"by": "x", "on": "2026-10-10", "digest": "abc"})


JUDGED = "judged by: test\n"


class Judgements(unittest.TestCase):
    """The tally takes only explicit answers, keeps uncertainty, and accounts for everything."""

    key = [{"n": 1, "index": 18, "choice": 1, "labels": ["fugumt"], "text": "x"},
           {"n": 1, "index": 18, "choice": 2, "labels": ["lfm2-t0"], "text": "y"}]

    def test_not_sure_is_unresolved_never_no(self):
        self.assertEqual(score.classify(" not sure "), "unsure")
        self.assertEqual(score.classify("?"), "unsure")

    def test_only_whole_answers_count(self):
        self.assertEqual(score.classify("Yes."), "yes")
        self.assertEqual(score.classify("N"), "no")
        self.assertEqual(score.classify("nope"), "invalid")
        self.assertEqual(score.classify("maybe"), "invalid")
        self.assertIsNone(score.classify("   "))

    def test_every_expected_translation_must_be_answered(self):
        sheet = JUDGED + "## 1\n\n1. x\n   keeps: yes\n\n2. y\n   keeps: \n"
        _, problems = score.parse_judgements(sheet, self.key)
        self.assertEqual(problems, ["item 1 choice 2 not yet answered"])

    def test_a_missing_or_doubled_translation_is_refused(self):
        _, missing = score.parse_judgements(JUDGED + "## 1\n\n1. x\n   keeps: yes\n", self.key)
        self.assertIn("item 1 choice 2 is missing from the sheet", missing)
        doubled = JUDGED + "## 1\n\n1. x\n   keeps: yes\n   keeps: no\n\n2. y\n   keeps: no\n"
        _, problems = score.parse_judgements(doubled, self.key)
        self.assertIn("item 1 choice 1 answered twice", problems)

    def test_a_clean_sheet_parses_to_its_answers(self):
        sheet = JUDGED + "## 1\n\n1. x\n   keeps: yes\n\n2. y\n   keeps: unsure\n"
        answers, problems = score.parse_judgements(sheet, self.key)
        self.assertEqual(problems, [])
        self.assertEqual(answers, {(1, 1): "yes", (1, 2): "unsure"})

    def test_a_written_sheet_reads_back_once_answered(self):
        import tempfile
        controls = {"items": [{"index": 0, "element": "e", "kind": "number"},
                              {"index": 1, "element": "f", "kind": "entity"}]}
        pooled = {0: ("ja0", "en0"), 1: ("ja1", "en1")}
        # Two engines agree on item 0, so it is shown once and judged once.
        candidates = {"base": {0: "same", 1: "one"}, "cand": {0: "same", 1: "two"}}
        with tempfile.TemporaryDirectory() as directory:
            package = pathlib.Path(directory)
            key = score.write_control_sheet(package, controls, pooled, candidates)
            sheet = (package / "controls-judgement.md").read_text(encoding="utf-8")
        self.assertEqual(len(key), 3)
        _, unanswered = score.parse_judgements(sheet, key)
        self.assertEqual(len(unanswered), 4)
        self.assertIn("the sheet does not say who judged it (`judged by:`)", unanswered)
        filled = sheet.replace("keeps: \n", "keeps: yes\n").replace("judged by: \n", "judged by: test\n")
        answers, problems = score.parse_judgements(filled, key)
        self.assertEqual(problems, [])
        self.assertEqual(set(answers.values()), {"yes"})

    def test_an_answer_beside_edited_text_is_not_attributed(self):
        sheet = JUDGED + "## 1\n\n1. something else entirely\n   keeps: yes\n\n2. y\n   keeps: no\n"
        _, problems = score.parse_judgements(sheet, self.key)
        self.assertEqual(problems, ["item 1 choice 1: the translation shown is not the one the key recorded "
                                    "— the answer cannot be attributed"])

    def test_exchanged_choices_are_refused(self):
        sheet = JUDGED + "## 1\n\n1. y\n   keeps: yes\n\n2. x\n   keeps: no\n"
        _, problems = score.parse_judgements(sheet, self.key)
        self.assertEqual(len(problems), 2)

    def test_only_the_answers_may_change(self):
        original = "judged by: \n## 1\n\nelement: `e`\n\n1. x\n   keeps: \n\n2. y\n   keeps: \n"
        digest = __import__("hashlib").sha256(score.skeleton(original).encode("utf-8")).hexdigest()
        answered = original.replace("judged by: ", "judged by: test").replace("keeps: \n", "keeps: yes\n")
        self.assertEqual(score.parse_judgements(answered, self.key, digest)[1], [])
        reworded = answered.replace("element: `e`", "element: `f`")
        self.assertIn("the sheet's text differs from the package as written — only the answers may change "
                      "(compare it with controls-judgement.original.md)",
                      score.parse_judgements(reworded, self.key, digest)[1])

    def test_a_sheet_that_does_not_say_who_judged_it_is_refused(self):
        sheet = "## 1\n\n1. x\n   keeps: yes\n\n2. y\n   keeps: no\n"
        _, problems = score.parse_judgements(sheet, self.key)
        self.assertEqual(problems, ["the sheet does not say who judged it (`judged by:`)"])

    def test_an_engine_outside_the_family_is_refused(self):
        with self.assertRaises(SystemExit):
            score.check_key_labels(self.key, ["fugumt", "lfm2-t05"])

    def test_an_engine_the_sheet_never_names_is_refused(self):
        with self.assertRaises(SystemExit):
            score.check_key_labels(self.key, ["fugumt", "lfm2-t0", "lfm2-t05"])


class Verdict(unittest.TestCase):
    """Unresolved answers can go either way, so the verdict holds on every resolution or is open."""

    def test_passes_only_in_the_candidates_worst_case(self):
        self.assertEqual(score.control_verdict([1, 2, 3], [], [], [1, 2], [3], []), "passes")
        self.assertEqual(score.control_verdict([1, 2], [], [3], [1], [2], [3]), "passes")

    def test_fails_only_in_the_candidates_best_case(self):
        self.assertEqual(score.control_verdict([1], [2, 3], [], [1, 2, 3], [], []), "fails")

    def test_otherwise_undetermined(self):
        self.assertEqual(score.control_verdict([1, 2], [], [3], [1, 2], [], [3]), "undetermined")


class Family(unittest.TestCase):
    """The correction family is fixed, and must add up."""

    def spec(self, **change):
        spec = {"baseline": "fugumt", "candidates": ["a", "b"], "metrics": ["metricx", "comet"],
                "primary": "metricx", "tests": 4, "interval_level": 1 - 0.05 / 4}
        spec.update(change)
        return {"comparison": spec}

    def test_a_consistent_family_is_accepted(self):
        self.assertEqual(score.family(self.spec())["tests"], 4)

    def test_a_family_whose_test_count_does_not_add_up_is_refused(self):
        with self.assertRaises(SystemExit):
            score.family(self.spec(candidates=["a"]))

    def test_a_level_that_does_not_match_the_count_is_refused(self):
        with self.assertRaises(SystemExit):
            score.family(self.spec(interval_level=0.95))

    def test_the_committed_family_is_consistent(self):
        controls = json.loads((HERE / "controls.json").read_text(encoding="utf-8"))
        self.assertEqual(score.family(controls)["tests"], 4)


class Mirror(unittest.TestCase):
    """The copy outside target/ never overwrites what it already holds."""

    def test_a_changed_file_is_kept_beside_the_old_one(self):
        import os
        import tempfile
        with tempfile.TemporaryDirectory() as source, tempfile.TemporaryDirectory() as copy:
            out, saved = score.OUT, os.environ.get("SAKATSU_MT_MIRROR")
            score.OUT = pathlib.Path(source)
            os.environ["SAKATSU_MT_MIRROR"] = copy
            try:
                (score.OUT / "review-x").mkdir()
                sheet = score.OUT / "review-x" / "controls-judgement.md"
                sheet.write_text("first", encoding="utf-8")
                score.mirror()
                sheet.write_text("answered", encoding="utf-8")
                score.mirror()
                # A third version within the same second: content-addressed names cannot collide.
                sheet.write_text("answered again", encoding="utf-8")
                score.mirror()
                score.mirror()  # and an unchanged rerun stores nothing new
                held = sorted(p.read_text(encoding="utf-8") for p in pathlib.Path(copy, "review-x").glob("*.md"))
                self.assertEqual(held, ["answered", "answered again", "first"])
                self.assertTrue((pathlib.Path(copy) / "SHA256SUMS").is_file())
            finally:
                score.OUT = out
                if saved is None:
                    os.environ.pop("SAKATSU_MT_MIRROR", None)
                else:
                    os.environ["SAKATSU_MT_MIRROR"] = saved

    def test_a_mirror_inside_target_is_refused(self):
        import os
        saved = os.environ.get("SAKATSU_MT_MIRROR")
        os.environ["SAKATSU_MT_MIRROR"] = str(score.ROOT / "target" / "elsewhere")
        try:
            with self.assertRaises(SystemExit):
                score.mirror_root()
        finally:
            if saved is None:
                os.environ.pop("SAKATSU_MT_MIRROR", None)
            else:
                os.environ["SAKATSU_MT_MIRROR"] = saved


class ContrastFile(unittest.TestCase):
    """Excluded contrast items stay, with a reason and a same-kind replacement."""

    contrast = json.loads((HERE / "contrast.json").read_text(encoding="utf-8"))["items"]

    def test_every_exclusion_says_why_and_names_a_replacement_of_its_kind(self):
        by_id = {c["tatoeba_jpn"]: c for c in self.contrast}
        excluded = [c for c in self.contrast if c.get("status") == "excluded"]
        self.assertEqual(len(excluded), 4)
        for item in excluded:
            self.assertTrue(item["why"])
            replacement = by_id[item["replaced_by"]]
            self.assertEqual(replacement["kind"], item["kind"])
            self.assertEqual(replacement["replaces"], item["tatoeba_jpn"])

    def test_ten_active_items_of_each_kind(self):
        kinds = __import__("collections").Counter(c["kind"] for c in self.contrast if c.get("status") != "excluded")
        self.assertEqual(set(kinds.values()), {10})


class Interval(unittest.TestCase):
    def test_the_bootstrap_interval_is_repeatable_and_brackets_the_mean(self):
        gains = [0.1, -0.05, 0.2, 0.0, 0.05, 0.15, -0.1, 0.08]
        first = score.bootstrap_interval(gains, 0.95)
        self.assertEqual(first, score.bootstrap_interval(gains, 0.95))
        self.assertLessEqual(first[0], sum(gains) / len(gains))
        self.assertGreaterEqual(first[1], sum(gains) / len(gains))


class ControlSet(unittest.TestCase):
    """The committed file is well formed; whether it fits the corpus is checked at run time."""

    controls = json.loads((HERE / "controls.json").read_text(encoding="utf-8"))

    def test_every_item_is_one_of_the_four_kinds_and_corrupts_something(self):
        for entry in self.controls["items"]:
            self.assertIn(entry["kind"], {"negation", "number", "entity", "referent"})
            old, new = entry["corrupt"]
            self.assertNotEqual(old, new)
            self.assertTrue(entry["accept"] and all(entry["accept"]))

    def test_every_accept_phrase_compiles(self):
        for entry in self.controls["items"]:
            for group in entry["accept"]:
                for phrase in group:
                    score.phrase(phrase)

    def test_indices_are_unique_and_inside_the_pooled_corpus(self):
        indices = [entry["index"] for entry in self.controls["items"]]
        self.assertEqual(len(indices), len(set(indices)))
        self.assertTrue(all(0 <= i < 300 for i in indices))


if __name__ == "__main__":
    unittest.main()
