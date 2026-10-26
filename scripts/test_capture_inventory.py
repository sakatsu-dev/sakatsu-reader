"""Synthetic inventory checks; no capture corpus, images or network are needed.

Every fixture here is written by the test. The real corpus is copyrighted, gitignored and partly
adult, so nothing in it may become a committed test fixture — which is also why the inventory's
own JSON output stays under ignored `scratch/`.
"""
import importlib.util
import json
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "capture_inventory", pathlib.Path(__file__).with_name("capture-inventory.py")
)
inventory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inventory)


class Naming(unittest.TestCase):
    """Suffixes are removed longest-first, because the tiers share prefixes."""

    def test_a_draft_is_not_read_as_a_transcript(self):
        self.assertEqual(inventory.split_name("app_A.txt.mine"), ("app_A", ".txt.mine"))
        self.assertEqual(inventory.split_name("app_A.txt"), ("app_A", ".txt"))
        self.assertEqual(
            inventory.split_name("app_A.txt.suggested"), ("app_A", ".txt.suggested")
        )
        self.assertEqual(inventory.split_name("app_A.tok.mine"), ("app_A", ".tok.mine"))

    def test_the_three_non_capture_files_are_excluded_by_name(self):
        for name in ("RESULTS.md", "REVIEW.md", "reading-gold.json"):
            self.assertIsNone(inventory.split_name(name), name)

    def test_a_bare_suffix_is_not_a_stem(self):
        self.assertIsNone(inventory.split_name(".txt"))
        self.assertIsNone(inventory.split_name(".tok.mine"))

    def test_an_unknown_extension_is_not_a_capture(self):
        self.assertIsNone(inventory.split_name("notes.md"))
        self.assertIsNone(inventory.split_name("archive.zip"))

    def test_the_application_may_contain_underscores(self):
        """`nexon_client_X` must file under `nexon_client`, beside its own siblings."""
        self.assertEqual(inventory.source_app("nexon_client_7ZQrspK8k8"), "nexon_client")
        self.assertEqual(inventory.source_app("firefox_4fsadNiA2a"), "firefox")
        self.assertEqual(inventory.source_app("ssr-stove-shield_MW187yw4vZ"), "ssr-stove-shield")

    def test_a_stem_with_no_underscore_is_its_own_application(self):
        self.assertEqual(inventory.source_app("loose"), "loose")


class Precedence(unittest.TestCase):
    def test_a_promoted_transcript_wins_over_the_draft_beside_it(self):
        self.assertEqual(
            inventory.select_transcript({".txt": 1, ".txt.mine": 1, ".txt.suggested": 1}), ".txt"
        )

    def test_the_draft_is_used_when_nothing_was_promoted(self):
        self.assertEqual(inventory.select_transcript({".txt.mine": 1, ".png": 1}), ".txt.mine")

    def test_a_suggestion_is_never_a_transcript(self):
        """Machine output. Mining it turns an OCR error into a colloquial finding."""
        self.assertIsNone(inventory.select_transcript({".txt.suggested": 1, ".png": 1}))
        self.assertIsNone(inventory.select_transcript({".tok.mine": 1, ".png": 1}))


class Suggestions(unittest.TestCase):
    def test_instruction_comments_are_not_engines(self):
        text = (
            "# Correct this against the IMAGE, then rename to .txt\n"
            "# The engines disagree - this sample is worth keeping.\n"
            "# manga-ocr\n"
            "ねこがいる\n"
            "# pp-ocrv5\n"
            "ねこがいた\n"
        )
        self.assertEqual(
            inventory.parse_suggested(text),
            {"manga-ocr": "ねこがいる", "pp-ocrv5": "ねこがいた"},
        )

    def test_a_trailing_heading_with_no_reading_is_not_an_engine(self):
        self.assertEqual(inventory.parse_suggested("# manga-ocr\n"), {})

    def test_a_file_with_no_comments_offers_no_engines(self):
        self.assertEqual(inventory.parse_suggested("ねこがいる\n"), {})

    def test_a_reading_may_be_several_lines(self):
        """`corpus_bootstrap.rs` emits `text.trim()`, and a capture with a speaker's name above
        their line comes back as three. Keeping only the first truncated every such reading."""
        text = (
            "# Correct this against the IMAGE, then rename to .txt\n"
            "# The engines disagree - this sample is worth keeping.\n"
            "# manga-ocr\n"
            "ユズハ「そういえば」\n"
            "# pp-ocrv5\n"
            "ユズハ\n"
            "「そういえば\n"
            "8\n"
        )
        self.assertEqual(
            inventory.parse_suggested(text),
            {"manga-ocr": "ユズハ「そういえば」", "pp-ocrv5": "ユズハ\n「そういえば\n8"},
        )

    def test_a_no_reading_marker_is_kept_without_inferring_why(self):
        """`# <name>: no reading` is information, and read as an instruction it vanished entirely.

        **It does not say the engine ran.** `corpus_bootstrap.rs::read_with` returns nothing both
        when the engine is unavailable — in which case it never ran — and when recognition failed.
        The marker means *no output available, reason not encoded*; the parser keeps it and draws
        no conclusion from it.
        """
        text = (
            "# Correct this against the IMAGE, then rename to .txt\n"
            "# manga-ocr\n"
            "ねこ\n"
            "# pp-ocrv5: no reading\n"
        )
        parsed = inventory.parse_suggested(text)
        self.assertEqual(parsed["manga-ocr"], "ねこ")
        self.assertIn("pp-ocrv5", parsed)
        self.assertIsNone(parsed["pp-ocrv5"], "an empty result is recorded, not dropped")

    def test_the_preamble_may_be_one_line_or_two(self):
        """It carries the second line only when the engines disagreed, so nothing may be skipped
        by position."""
        one = "# Correct this against the IMAGE, then rename to .txt\n# manga-ocr\nねこ\n"
        self.assertEqual(inventory.parse_suggested(one), {"manga-ocr": "ねこ"})


class Segmentation(unittest.TestCase):
    def test_segments_reassemble_their_transcript(self):
        segments = inventory.segments_of("暁光|の|女神|は|、|その|姿|を|消してしまった|。")
        self.assertEqual(len(segments), 10)
        self.assertTrue(
            inventory.segmentation_matches(segments, "暁光の女神は、その姿を消してしまった。")
        )

    def test_a_disagreement_is_reported_rather_than_normalised(self):
        """Which of the two drafts is wrong is a question for a person, not for this."""
        segments = inventory.segments_of("暁光|の|女神")
        self.assertFalse(inventory.segmentation_matches(segments, "時光の女神"))

    def test_empty_separators_do_not_become_tokens(self):
        self.assertEqual(inventory.segments_of("あ||い|"), ["あ", "い"])

    def test_a_line_break_is_a_token_and_reassembles_as_a_newline(self):
        """Four of the seventeen real segmentations are multi-line and use this.

        A speaker's name sits above their line, and `/` stands for the break. Reassembling without
        knowing that reports faithful drafts as corrupt — which it did, before this.
        """
        segments = inventory.segments_of("ユズハ|/|「|あ|、|お兄ちゃん|じゃん|」")
        self.assertIn("/", segments)
        self.assertTrue(
            inventory.segmentation_matches(segments, "ユズハ\n「あ、お兄ちゃんじゃん」")
        )

    def test_a_space_inside_a_line_is_part_of_the_text(self):
        segments = inventory.segments_of("そういえば| |お兄ちゃん")
        self.assertEqual(segments, ["そういえば", " ", "お兄ちゃん"])
        self.assertTrue(inventory.segmentation_matches(segments, "そういえば お兄ちゃん"))


class Scanning(unittest.TestCase):
    def test_a_pattern_is_counted_per_occurrence(self):
        found = inventory.scan("行くって言ってた")
        self.assertEqual(found["quotation"]["って"], 2)

    def test_a_text_with_nothing_reports_nothing(self):
        self.assertEqual(inventory.scan("山が高い"), {})

    def test_the_patterns_retrieve_and_do_not_classify(self):
        """**Documented, not regretted.**

        `なんとか` contains the explanatory family's `なん` and the quotation family's `とか`, and is
        neither. The scan is a retrieval hint whose output a reviewer adjudicates; a test that
        pretended otherwise would be asserting a precision this cannot have.
        """
        found = inventory.scan("なんとかしないと")
        self.assertEqual(found["explanatory"]["なん"], 1)
        self.assertEqual(found["quotation"]["とか"], 1)


class Building(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)

    def write(self, name, text="", image=False):
        path = self.dir / name
        if image:
            path.write_bytes(b"\x89PNG" + name.encode("utf-8"))
        else:
            path.write_text(text, encoding="utf-8")

    def test_a_promoted_capture_is_read_from_the_promoted_tier(self):
        self.write("app_A.png", image=True)
        self.write("app_A.txt", "行くって言ってた")
        self.write("app_A.txt.mine", "行くって言っでた")
        built = inventory.build(self.dir)
        (record,) = built["captures"]
        self.assertEqual(record["status"], "promoted")
        self.assertEqual(record["transcript_tier"], ".txt")
        self.assertEqual(record["text"], "行くって言ってた")

    def test_an_image_with_no_transcript_is_pending_and_warned_about(self):
        self.write("app_B.png", image=True)
        self.write("app_B.txt.suggested", "# manga-ocr\nねこ\n")
        built = inventory.build(self.dir)
        (record,) = built["captures"]
        self.assertEqual(record["status"], "pending transcription")
        self.assertNotIn("text", record)
        self.assertEqual(record["engines"], {"manga-ocr": "ねこ"})
        self.assertTrue(any("no transcript" in w for w in built["warnings"]))

    def test_text_without_an_image_is_not_a_capture_and_says_so(self):
        """The two stray `.txt` files in the real corpus reach this path."""
        self.write("ocr-to-text-corpus.txt", "notes")
        built = inventory.build(self.dir)
        self.assertEqual(built["captures"], [])
        self.assertTrue(any("no image" in w for w in built["warnings"]))

    def test_the_non_capture_files_are_ignored_not_counted(self):
        self.write("app_C.png", image=True)
        self.write("app_C.txt", "ねこ")
        for name in ("RESULTS.md", "REVIEW.md", "reading-gold.json"):
            self.write(name, "{}")
        built = inventory.build(self.dir)
        self.assertEqual(len(built["captures"]), 1)
        self.assertEqual(sorted(built["ignored_files"]), ["RESULTS.md", "REVIEW.md", "reading-gold.json"])

    def test_a_segmentation_that_disagrees_with_its_text_is_warned_about(self):
        self.write("app_D.png", image=True)
        self.write("app_D.txt.mine", "時光の女神")
        self.write("app_D.tok.mine", "暁光|の|女神")
        built = inventory.build(self.dir)
        (record,) = built["captures"]
        self.assertFalse(record["segmentation_matches_text"])
        self.assertTrue(any("do not reassemble" in w for w in built["warnings"]))

    def test_duplicate_text_is_grouped_and_every_source_is_kept(self):
        for stem in ("app_E", "app_F"):
            self.write(f"{stem}.png", image=True)
            self.write(f"{stem}.txt", "同じ文です")
        self.write("app_G.png", image=True)
        self.write("app_G.txt", "違う文です")
        built = inventory.build(self.dir)
        groups = inventory.duplicates(built["captures"])
        self.assertEqual(len(built["captures"]), 3, "no capture is dropped for being a duplicate")
        self.assertEqual(len(groups["text"]), 1)
        self.assertEqual(sorted(next(iter(groups["text"].values()))), ["app_E", "app_F"])
        self.assertEqual(groups["image"], {}, "distinct images are not duplicates")

    def test_captures_in_subdirectories_are_found(self):
        """A corpus that grows tends to grow into folders, and a top-level-only scan reports a
        shrinking corpus as a stable one."""
        (self.dir / "2026-09").mkdir()
        self.write("2026-09/app_K.png", image=True)
        self.write("2026-09/app_K.txt", "ねこ")
        built = inventory.build(self.dir)
        (record,) = built["captures"]
        self.assertEqual(record["stem"], "2026-09/app_K")
        self.assertEqual(record["source_app"], "app", "the folder is not part of the application")

    def test_the_same_stem_in_two_directories_is_two_captures(self):
        """Identity is relative directory **plus** stem. Keyed on the stem alone these merge, and
        one of the two transcripts is silently lost."""
        for folder in ("a", "b"):
            (self.dir / folder).mkdir()
            self.write(f"{folder}/app_L.png", image=True)
            self.write(f"{folder}/app_L.txt", f"文{folder}")
        built = inventory.build(self.dir)
        self.assertEqual(len(built["captures"]), 2)
        self.assertEqual(
            sorted(record["stem"] for record in built["captures"]), ["a/app_L", "b/app_L"]
        )
        self.assertEqual(
            sorted(record["text"] for record in built["captures"]), ["文a", "文b"]
        )

    def test_a_top_level_capture_keeps_its_bare_stem(self):
        """So existing ids and every artifact that quotes one stay valid."""
        self.write("app_M.png", image=True)
        self.write("app_M.txt", "ねこ")
        (record,) = inventory.build(self.dir)["captures"]
        self.assertEqual(record["stem"], "app_M")

    def test_identical_images_are_detected_separately_from_identical_text(self):
        for stem in ("app_H", "app_I"):
            (self.dir / f"{stem}.png").write_bytes(b"\x89PNGsame")
            self.write(f"{stem}.txt", f"文{stem}")
        groups = inventory.duplicates(inventory.build(self.dir)["captures"])
        self.assertEqual(groups["text"], {})
        self.assertEqual(len(groups["image"]), 1)

    def test_the_report_names_no_capture_text(self):
        """The corpus is copyrighted and partly adult; counts may travel, sentences may not.

        The stdout report is the part that ends up quoted in a ledger or a handoff, so it carries
        counts, applications and tiers and **never a captured sentence**. The full text stays in
        the JSON, which is written under ignored `scratch/` and goes nowhere.
        """
        self.write("app_J.png", image=True)
        self.write("app_J.txt", "行くって言ってた")
        import io

        out = io.StringIO()
        inventory.report(inventory.build(self.dir), out=out)
        printed = out.getvalue()
        self.assertNotIn("行くって言ってた", printed, "a captured sentence reached the report")
        self.assertNotIn("app_J", printed, "the report summarises by application, not by capture")
        # It did measure the capture, though — the count and the pattern are both there.
        self.assertIn("app ", printed)
        self.assertIn("って", printed)


if __name__ == "__main__":
    unittest.main()
