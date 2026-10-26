"""Synthetic parser/eligibility checks; no Aozora text or network is needed."""
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location("aozora", pathlib.Path(__file__).with_name("harvest-aozora-readings.py"))
aozora = importlib.util.module_from_spec(spec)
spec.loader.exec_module(aozora)


class ReadingExtraction(unittest.TestCase):
    def test_explicit_and_implicit_ruby_keep_the_plain_sentence(self):
        self.assertEqual(list(aozora.extract("その｜方《カタ》は誰ですか。", {"方"})), [("方", "かた", "その方は誰ですか。")])
        self.assertEqual(list(aozora.extract("この方《ほう》がよさそうだ。", {"方"})), [("方", "ほう", "この方がよさそうだ。")])

    def test_a_longer_base_does_not_annotate_its_suffix(self):
        self.assertEqual(list(aozora.extract("その彼方《かなた》には山がある。", {"方"})), [])

    def test_two_occurrences_cannot_claim_one_reading_for_the_sentence(self):
        self.assertEqual(list(aozora.extract("あの方《かた》はその方《ほう》を向いた。", {"方"})), [])

    def test_notes_and_source_credits_are_not_exemplars(self):
        text = "--------------------\n方《ほう》は注記の説明です。\n--------------------\nその方《かた》は［＃傍点］先生でした。\n底本：\nその方《ほう》は書誌情報です。"
        self.assertEqual(list(aozora.extract(text, {"方"})), [("方", "かた", "その方は先生でした。")])

    def test_translations_and_in_copyright_people_are_excluded(self):
        row = {"作品ID": "1", "作品著作権フラグ": "なし", "人物著作権フラグ": "なし", "役割フラグ": "著者", "文字遣い種別": "新字新仮名", "分類番号": "NDC 913", "テキストファイルURL": "https://www.aozora.gr.jp/a.zip"}
        self.assertEqual(len(aozora.eligible([row])), 1)
        self.assertEqual(aozora.eligible([row, {**row, "役割フラグ": "翻訳者"}]), [])
        self.assertEqual(aozora.eligible([{**row, "人物著作権フラグ": "あり"}]), [])


class Limits(unittest.TestCase):
    """The limits are the experiment's variable, so they are checked rather than assumed."""

    def test_the_defaults_are_the_first_run_that_could_not_decide(self):
        # The 300/24/3 run produced 1,565 examples and a classifier that abstained on all 119
        # eligible targets. Changing a default silently would make a later run incomparable.
        args = aozora.parse_args([])
        self.assertEqual((args.max_books, args.per_reading, args.per_book), (300, 24, 3))
        self.assertEqual(args.max_bytes, 100 * 1024 * 1024)
        self.assertFalse(args.download)

    def test_a_wider_run_must_raise_the_transfer_budget_deliberately(self):
        args = aozora.parse_args(["--max-books", "5369", "--per-reading", "48", "--max-bytes", "268435456"])
        self.assertEqual((args.max_books, args.per_reading, args.max_bytes), (5369, 48, 268435456))

    def test_no_limit_may_be_zero_or_negative(self):
        for argv in (["--max-books", "0"], ["--per-reading", "0"], ["--per-book", "0"], ["--max-bytes", "-1"]):
            with self.subTest(argv=argv), self.assertRaises(SystemExit):
                aozora.parse_args(argv)


if __name__ == "__main__":
    unittest.main()
