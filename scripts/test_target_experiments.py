"""Guard evidence boundaries and abstention in the test-only experiments."""
import importlib.util
import json
from pathlib import Path
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


readings = load('evaluate-target-readings')
entries = load('evaluate-target-entries')


class EvidenceTests(unittest.TestCase):
    def test_target_is_exact_utf8_occurrence(self):
        row = {'text': '生と生。', 'surface': '生', 'start': 6, 'end': 9}
        self.assertEqual(readings.target_parts(row), ('生と', '生', '。'))
        with self.assertRaises((AssertionError, UnicodeDecodeError)):
            readings.target_parts({**row, 'start': 7})

    def test_missing_rival_cannot_lose_by_default(self):
        labels = ['a', 'b']
        support = {'a': {'examples': 10, 'works': 3}, 'b': {'examples': 2, 'works': 2}}
        self.assertEqual(readings.select_reading(labels, labels, support, [1, 0]),
                         (None, 'insufficient_candidate_support'))
        support['b']['examples'] = 3
        self.assertEqual(readings.select_reading(labels, labels, support, [.5, .4]),
                         (None, 'margin_below_0.2'))
        self.assertEqual(readings.select_reading(labels, labels, support, [.8, .2]), ('a', 'decided'))

    def test_prompt_hides_evaluation_labels_and_reverses_candidates(self):
        case = {'text': '生と生。', 'surface': '生', 'start': 6, 'end': 9,
                'gold': 'SECRET_GOLD', 'source_right': 'SECRET_BASELINE', 'reading_right': False,
                'candidates': [{'id': 123, 'readings': ['a'], 'glosses': ['fresh']},
                               {'id': 456, 'readings': ['b'], 'glosses': ['life']}]}
        request, mapping = entries.request_for(case)
        reverse, reversed_mapping = entries.request_for(case, True)
        self.assertNotIn('SECRET', json.dumps(request))
        self.assertNotIn('123', request['prompt'])
        self.assertIn('生と<target>生</target>。', request['prompt'])
        self.assertEqual(list(mapping.values()), list(reversed(list(reversed_mapping.values()))))
        self.assertNotEqual(request['prompt'], reverse['prompt'])


if __name__ == '__main__':
    unittest.main()
