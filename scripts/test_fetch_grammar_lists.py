"""The grammar-list parser keeps names, levels and links, and nothing else.

Synthetic markup in each site's shape; the Japanese in it is grammar-point *names*, which is all
the fetcher reads. See `fetch-grammar-lists.py`.
"""
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "fetch_grammar_lists", pathlib.Path(__file__).with_name("fetch-grammar-lists.py"))
lists = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lists)


class Titles(unittest.TestCase):
    def test_a_stated_level_is_read_from_the_title(self):
        self.assertEqual(lists.split_title("〜し〜し①（並列）｜日本語能力試験　JLPT N4文法"),
                         ("〜し〜し①（並列）", ["N4"]))

    def test_full_width_digits_and_two_levels(self):
        self.assertEqual(lists.split_title("〜ようと思う｜日本語能力試験　JLPT　N２・N３文法"),
                         ("〜ようと思う", ["N2", "N3"]))

    def test_a_title_without_a_level_states_none(self):
        self.assertEqual(lists.split_title("〜じゃん"), ("〜じゃん", []))


class JlptGrammarList(unittest.TestCase):
    PAGE = (
        '<div class="grammar-list n4"><a id="n4"><h1>N4</h1></a>'
        '<div class="item"><span class="term">が <sup>2</sup></span><span class="common"></span>but; however'
        '<div class="japanese-sentence"><a href="x.mp3"></a>EXAMPLE SENTENCE</div>'
        '<div class="english-meaning">EXAMPLE TRANSLATION</div></div>'
        '</div>'
    )

    class Page:
        def __init__(self, text):
            self.text = text

        def get(self, url):
            return self.text

    def test_names_levels_and_glosses_are_kept_and_the_example_is_not(self):
        rows = lists.jlpt_grammar_list(self.Page(self.PAGE))
        self.assertEqual(len(rows), 1)
        source, kind, name, sense, level, level_from, url, gloss = rows[0]
        self.assertEqual((name, sense, level, gloss), ("が", "2", "N4", "but; however"))
        self.assertNotIn("EXAMPLE", "".join(rows[0]))


if __name__ == "__main__":
    unittest.main()
