"""Synthetic checks for chat acquisition, coverage and partitioning; no network or corpus needed."""
import hashlib
import importlib.util
import json
import pathlib
import tempfile
import unittest


def load(name, module):
    spec = importlib.util.spec_from_file_location(module, pathlib.Path(__file__).with_name(name))
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


fetch = load("fetch-chat-corpora.py", "fetch_chat_corpora")
partition = load("chat-partition.py", "chat_partition")
freeze = load("chat-freeze.py", "chat_freeze")
identity = load("chat-identity-audit.py", "chat_identity_audit")

RPC = fetch.SOURCES["real-persona-chat"]


class Selection(unittest.TestCase):
    """What the archive filter takes, and the two things it must refuse."""

    def test_dialogue_json_is_taken(self):
        self.assertTrue(fetch.wanted(RPC, "repo-sha/real_persona_chat/dialogues/13770.json"))

    def test_generated_dialogues_are_refused(self):
        """`llm_dialogue_system/` holds GPT-4 and GPT-3.5 output beside the human corpus. Taking
        it would put machine-written Japanese into a corpus about how people write."""
        for path in (
            "repo-sha/llm_dialogue_system/GPT-4.json",
            "repo-sha/llm_dialogue_system/GPT-4+persona+personality.json",
            "repo-sha/llm_dialogue_system/prompts.json",
        ):
            self.assertFalse(fetch.wanted(RPC, path), path)

    def test_participant_profiles_are_refused(self):
        """Persona and personality inventories about real people, which the pilot has no use for."""
        self.assertFalse(fetch.wanted(RPC, "repo-sha/real_persona_chat/interlocutors.json"))

    def test_non_json_and_unrelated_paths_are_refused(self):
        for path in ("repo-sha/README.md", "repo-sha/Moonshot_Logo.png", "repo-sha/VERSION"):
            self.assertFalse(fetch.wanted(RPC, path), path)


class Normalization(unittest.TestCase):
    def test_only_the_needed_fields_survive(self):
        record = fetch.normalize_dialogue(
            "real-persona-chat",
            "real_persona_chat/dialogues/1.json",
            {
                "dialogue_id": 1,
                "interlocutors": ["AA", "BB"],
                "evaluations": [{"interlocutor_id": "AA", "satisfaction": 5}],
                "utterances": [
                    {
                        "utterance_id": 0,
                        "interlocutor_id": "AA",
                        "text": "こんにちは",
                        "timestamp": "2022-10-04T14:25:59",
                    }
                ],
            },
        )
        self.assertEqual(record["dialogue_id"], "1")
        self.assertEqual(record["speakers"], ["AA", "BB"])
        self.assertNotIn("evaluations", record)
        utterance = record["utterances"][0]
        self.assertEqual(utterance["text"], "こんにちは")
        self.assertNotIn("timestamp", utterance, "a timestamp is not needed and is not imported")
        self.assertEqual(utterance["sha256"], fetch.digest("こんにちは".encode("utf-8")))

    def test_source_order_is_recorded_independently_of_the_id(self):
        record = fetch.normalize_dialogue(
            "s",
            "p",
            {"dialogue_id": "d", "utterances": [{"utterance_id": 7, "text": "あ"}, {"text": "い"}]},
        )
        self.assertEqual([u["order"] for u in record["utterances"]], [0, 1])
        self.assertEqual([u["utterance_id"] for u in record["utterances"]], ["7", "1"])

    def test_the_condition_is_kept_where_the_corpus_states_one(self):
        record = fetch.normalize_dialogue(
            "s", "p", {"dialogue_id": "d", "dialogue_type": "Family", "utterances": []}
        )
        self.assertEqual(record["condition"], "Family")


class Components(unittest.TestCase):
    def dialogue(self, identity, speakers):
        return {"id": identity, "source": "s", "speakers": speakers, "utterances": 1}

    def test_speakers_sharing_a_conversation_are_one_component(self):
        groups = partition.components(
            [self.dialogue("d1", ["a", "b"]), self.dialogue("d2", ["c", "d"])]
        )
        self.assertEqual(sorted(len(g["dialogues"]) for g in groups), [1, 1])

    def test_the_relation_is_transitive(self):
        """A talks to B and B to C, so all three move together — which is why a speaker-disjoint
        split is a property of the corpus rather than a choice."""
        groups = partition.components(
            [self.dialogue("d1", ["a", "b"]), self.dialogue("d2", ["b", "c"])]
        )
        self.assertEqual(len(groups), 1)
        self.assertEqual(len(groups[0]["dialogues"]), 2)

    def sized(self, *sizes):
        return [
            {"dialogues": [f"c{i}_{j}" for j in range(size)], "speakers": [f"s{i}"]}
            for i, size in enumerate(sizes)
        ]

    def test_a_valid_split_is_found_where_greedy_reported_none(self):
        """**Reproduces a defect.** Largest-first takes 14, stops at 14% and calls the range
        unreachable — while 13 + 12 is exactly 25% and valid. A component is indivisible, so this
        is a subset-sum, not a greedy walk."""
        split = partition.feasible_split(self.sized(61, 14, 13, 12), 100)
        self.assertIsNotNone(split)
        self.assertEqual(split["dialogues"], 25)
        self.assertEqual(split["component_sizes"], [13, 12])

    def test_the_split_closest_to_the_target_wins_over_the_first_that_fits(self):
        """**Reproduces a defect.** Greedy takes 25 and stops, when 12 + 8 is 20% and hits the
        target exactly."""
        split = partition.feasible_split(self.sized(55, 25, 12, 8), 100)
        self.assertEqual(split["dialogues"], 20)
        self.assertEqual(split["component_sizes"], [12, 8])

    def test_the_answer_does_not_depend_on_component_order(self):
        import itertools

        answers = {
            tuple(sorted(partition.feasible_split(list(perm), 100)["component_sizes"]))
            for perm in itertools.permutations(self.sized(55, 25, 12, 8))
        }
        self.assertEqual(len(answers), 1)

    def test_a_dominant_component_makes_the_target_range_unreachable(self):
        groups = [
            {"dialogues": [f"d{i}" for i in range(90)], "speakers": ["a"]},
            {"dialogues": ["x", "y", "z"], "speakers": ["b"]},
        ]
        self.assertIsNone(partition.feasible_split(groups, 93))

    def test_a_split_is_reported_when_whole_components_can_form_one(self):
        groups = [
            {"dialogues": [f"d{i}" for i in range(80)], "speakers": ["a"]},
            {"dialogues": [f"e{i}" for i in range(20)], "speakers": ["b"]},
        ]
        split = partition.feasible_split(groups, 100)
        self.assertIsNotNone(split)
        self.assertEqual(split["dialogues"], 20)
        self.assertAlmostEqual(split["share"], 0.20)

    def test_the_split_is_deterministic(self):
        groups = [
            {"dialogues": [f"a{i}" for i in range(10)], "speakers": ["a"]},
            {"dialogues": [f"b{i}" for i in range(10)], "speakers": ["b"]},
            {"dialogues": [f"c{i}" for i in range(80)], "speakers": ["c"]},
        ]
        first = partition.feasible_split(groups, 100)
        second = partition.feasible_split(list(reversed(groups)), 100)
        self.assertEqual(first, second)


class Allocation(unittest.TestCase):
    """What the capture weighting is allowed to do to a family, and what it is not."""

    def test_a_zero_weight_family_is_thinned_not_dropped(self):
        """Regional has zero capture attestation. Weighting alone would give it nothing, which
        would make 'no dialect found' a consequence of not looking."""
        allocation = freeze.allocate({"a": 29, "b": 0}, 20, floor=6)
        self.assertEqual(allocation["b"], 6)
        self.assertEqual(sum(allocation.values()), 20)

    def test_a_fixed_reservation_is_not_weighted(self):
        """The regional allocation is a decision, not a derivation — so it must survive whatever
        the weights say, including weights that would otherwise swamp it."""
        allocation = freeze.allocate(
            {"big": 1000, "regional": 0}, 20, floor=6, fixed={"regional": 8}
        )
        self.assertEqual(allocation["regional"], 8)
        self.assertEqual(allocation["big"], 12)

    def test_the_allocation_sums_exactly(self):
        allocation = freeze.allocate(
            freeze.CAPTURE_ATTESTATION,
            freeze.FAMILY_BUDGET,
            freeze.FAMILY_FLOOR,
            fixed={"regional": freeze.REGIONAL_DISCOVERY},
        )
        self.assertEqual(sum(allocation.values()), freeze.FAMILY_BUDGET)
        self.assertEqual(allocation["regional"], freeze.REGIONAL_DISCOVERY)
        self.assertTrue(all(count >= freeze.FAMILY_FLOOR for count in allocation.values()))

    def test_the_allocation_does_not_depend_on_key_order(self):
        forward = freeze.allocate({"a": 29, "b": 27, "c": 9, "d": 7}, 40, floor=6)
        backward = freeze.allocate({"d": 7, "c": 9, "b": 27, "a": 29}, 40, floor=6)
        self.assertEqual(forward, backward)

    def test_an_impossible_floor_is_refused_rather_than_silently_shrunk(self):
        with self.assertRaises(ValueError):
            freeze.allocate({"a": 1, "b": 1, "c": 1}, 10, floor=6)


def dialogue(identity, texts, source="s", speakers=None, condition=None):
    return {
        "source": source,
        "dialogue_id": identity,
        "key": f"{source}:{identity}",
        "condition": condition,
        "speakers": speakers or ["x", "y"],
        "utterances": [
            {
                "utterance_id": str(i),
                "order": i,
                "speaker": (speakers or ["x", "y"])[i % 2],
                "text": text,
            }
            for i, text in enumerate(texts)
        ],
    }


class Partitioning(unittest.TestCase):
    def test_identical_dialogues_land_on_the_same_side(self):
        """Two dialogues with the same content must not straddle the split, or the holdout
        contains material the development set has already been fitted to."""
        pair = [dialogue("d1", ["おはよう", "げんき"]), dialogue("d2", ["おはよう", "げんき"])]
        others = [dialogue(f"o{i}", [f"文{i}"]) for i in range(50)]
        corpus = pair + others
        duplicates = freeze.duplicate_dialogues(corpus)
        self.assertEqual(len(duplicates), 1)
        assignment = freeze.partition(corpus, duplicates)
        self.assertEqual(assignment["s:d1"], assignment["s:d2"])

    def test_adding_a_dialogue_does_not_reassign_the_others(self):
        """Threshold assignment, not 'take the first N of a shuffle' — otherwise the frozen
        partition would silently change the moment the corpus did."""
        corpus = [dialogue(f"d{i}", [f"文{i}"]) for i in range(60)]
        before = freeze.partition(corpus, {})
        grown = corpus + [dialogue("new", ["あたらしい"])]
        after = freeze.partition(grown, freeze.duplicate_dialogues(grown))
        for key, side in before.items():
            self.assertEqual(after[key], side, key)

    def test_speaker_overlap_is_measured_not_assumed_away(self):
        corpus = [
            dialogue("d1", ["あ"], speakers=["p", "q"]),
            dialogue("d2", ["い"], speakers=["p", "q"]),
        ]
        assignment = {"s:d1": "development", "s:d2": "holdout"}
        overlap = freeze.speaker_overlap(corpus, assignment)
        self.assertEqual(overlap["speakers"], 2)
        self.assertEqual(overlap["in_both_partitions"], 2)


class Sampling(unittest.TestCase):
    FAMILIES = {"quotation": ["って"], "regional": ["やん"]}

    def corpus(self, count=200):
        return [
            dialogue(f"d{i}", [f"これって{i}です", f"ほんまやん{i}", f"ふつうの文{i}"])
            for i in range(count)
        ]

    def test_only_development_dialogues_are_sampled(self):
        corpus = self.corpus()
        assignment = {d["key"]: ("holdout" if i < 100 else "development")
                      for i, d in enumerate(corpus)}
        held = {d["key"] for i, d in enumerate(corpus) if i < 100}
        cases, _ = freeze.sample(corpus, assignment, self.FAMILIES, {"quotation": 5, "regional": 5})
        self.assertTrue(cases)
        for case in cases:
            self.assertNotIn(f"{case['source']}:{case['dialogue_id']}", held)

    def test_no_utterance_and_no_repeated_text_is_sampled_twice(self):
        corpus = [dialogue(f"d{i}", ["おはよう", f"これって{i}"]) for i in range(100)]
        assignment = {d["key"]: "development" for d in corpus}
        cases, _ = freeze.sample(corpus, assignment, self.FAMILIES, {"quotation": 10, "regional": 0})
        identities = [(c["dialogue_id"], c["utterance_id"]) for c in cases]
        self.assertEqual(len(identities), len(set(identities)))
        texts = [c["text"] for c in cases]
        self.assertEqual(len(texts), len(set(texts)), "the same text was sampled twice")

    def test_an_overlong_utterance_is_excluded_rather_than_truncated(self):
        """A truncated utterance is a different sentence, and would be scored as the original."""
        long_text = "って" * (freeze.MAX_SCALARS)
        corpus = [dialogue("d1", [long_text]), dialogue("d2", ["これって短い"])]
        assignment = {d["key"]: "development" for d in corpus}
        cases, _ = freeze.sample(corpus, assignment, self.FAMILIES, {"quotation": 5, "regional": 0})
        self.assertEqual([c["text"] for c in cases], ["これって短い"])
        self.assertTrue(all(len(c["text"]) <= freeze.MAX_SCALARS for c in cases))

    def test_context_stops_at_the_conversation_edges(self):
        corpus = [dialogue("d1", ["これって一"])]
        assignment = {"s:d1": "development"}
        cases, _ = freeze.sample(corpus, assignment, self.FAMILIES, {"quotation": 1, "regional": 0})
        self.assertEqual(cases[0]["context_before"], [])
        self.assertEqual(cases[0]["context_after"], [])

    def test_context_is_the_turns_either_side_not_a_window_of_text(self):
        corpus = [dialogue("d1", ["一", "二", "これって三", "四", "五"])]
        assignment = {"s:d1": "development"}
        cases, _ = freeze.sample(corpus, assignment, self.FAMILIES, {"quotation": 1, "regional": 0})
        case = next(c for c in cases if c["text"] == "これって三")
        self.assertEqual([t["text"] for t in case["context_before"]], ["一", "二"])
        self.assertEqual([t["text"] for t in case["context_after"]], ["四"])

    def test_a_shortage_is_reported_rather_than_backfilled(self):
        """A thin family must show up as a shortage. Quietly topping it up from a plentiful one
        would report an allocation that was never drawn."""
        corpus = [dialogue("d1", ["ほんまやん", "これって"])]
        assignment = {"s:d1": "development"}
        cases, shortages = freeze.sample(
            corpus, assignment, self.FAMILIES, {"quotation": 1, "regional": 9}
        )
        regional = [s for s in shortages if s["stratum"] == "family:regional"]
        self.assertEqual(len(regional), 1)
        self.assertEqual(regional[0], {"source": "s", "stratum": "family:regional",
                                       "wanted": 9, "got": 1, "pool": 1})
        self.assertEqual(sum(1 for c in cases if c["family"] == "regional"), 1)

    def test_a_scarce_family_picks_before_a_plentiful_one(self):
        """An utterance matching both families is claimed once. The scarce family must get first
        refusal, or its allocation is decided by what the plentiful one left behind."""
        corpus = [dialogue("d1", ["これってほんまやん"])] + [
            dialogue(f"p{i}", [f"これって{i}"]) for i in range(20)
        ]
        assignment = {d["key"]: "development" for d in corpus}
        cases, _ = freeze.sample(corpus, assignment, self.FAMILIES, {"quotation": 5, "regional": 1})
        both = [c for c in cases if c["text"] == "これってほんまやん"]
        self.assertEqual(len(both), 1)
        self.assertEqual(both[0]["family"], "regional")

    def test_the_sample_is_reproducible(self):
        corpus = self.corpus()
        assignment = {d["key"]: "development" for d in corpus}
        first, _ = freeze.sample(corpus, assignment, self.FAMILIES, {"quotation": 9, "regional": 9})
        second, _ = freeze.sample(
            list(reversed(corpus)), assignment, self.FAMILIES, {"quotation": 9, "regional": 9}
        )
        self.assertEqual([c["case_id"] for c in first], [c["case_id"] for c in second])


class CanonicalIdentity(unittest.TestCase):
    """What identifies a source record, and what merely agrees with one."""

    def dialogue(self, path, identity, texts, speakers=("a", "b"), condition="First time"):
        return {
            "source": "s",
            "path": path,
            "dialogue_id": identity,
            "condition": condition,
            "speakers": list(speakers),
            "utterances": [
                {"utterance_id": str(i), "order": i, "speaker": speakers[i % 2], "text": text}
                for i, text in enumerate(texts)
            ],
        }

    def records(self, *dialogues):
        return [("s", "rev1", dialogue) for dialogue in dialogues]

    def test_two_files_declaring_one_id_are_two_canonical_identities(self):
        """**The defect this exists for.** RealPersonaChat ships `dialogues/10279.json` declaring
        `dialogue_id: 10577`, so a key of `source:dialogue_id` counts two files once."""
        records = self.records(
            self.dialogue("d/10279.json", "10577", ["おはよう"]),
            self.dialogue("d/10577.json", "10577", ["おはよう"]),
        )
        declared, canonical, collisions = identity.audit(records)
        self.assertEqual(len(declared), 1, "the declared key collapses them")
        self.assertEqual(len(canonical), 2, "the canonical identity keeps them apart")
        self.assertEqual(collisions, [], "two distinct paths do not collide canonically")

    def test_identical_text_with_different_context_does_not_agree(self):
        """**Text agreement is not identity, and the audit must be able to say so.** Same words,
        different speakers, is a different source record and a very different finding."""
        members = self.records(
            self.dialogue("d/1.json", "7", ["おはよう"], speakers=("a", "b")),
            self.dialogue("d/2.json", "7", ["おはよう"], speakers=("c", "d")),
        )
        agreement = identity.compare(members)
        self.assertTrue(agreement["text_agrees"])
        self.assertFalse(agreement["speakers_agrees"], "different speakers must not read as agreement")

    def test_a_differing_condition_is_reported_separately_from_text(self):
        members = self.records(
            self.dialogue("d/1.json", "7", ["おはよう"], condition="Family"),
            self.dialogue("d/2.json", "7", ["おはよう"], condition="First time"),
        )
        agreement = identity.compare(members)
        self.assertTrue(agreement["text_agrees"])
        self.assertFalse(agreement["condition_agrees"])

    def test_conflicting_text_is_reported_as_disagreement(self):
        members = self.records(
            self.dialogue("d/1.json", "7", ["おはよう"]),
            self.dialogue("d/2.json", "7", ["こんばんは"]),
        )
        self.assertFalse(identity.compare(members)["text_agrees"])

    def test_the_audit_does_not_depend_on_file_order(self):
        first = self.records(
            self.dialogue("d/a.json", "7", ["あ"]),
            self.dialogue("d/b.json", "7", ["あ"]),
        )
        second = list(reversed(first))
        self.assertEqual(
            sorted(identity.audit(first)[1]),
            sorted(identity.audit(second)[1]),
            "canonical identities are a property of the files, not of the read order",
        )

    def test_the_same_path_twice_is_a_canonical_collision_and_is_refused(self):
        """One file counted twice is a corrupt corpus, not a publisher's id quirk."""
        records = self.records(
            self.dialogue("d/1.json", "7", ["あ"]),
            self.dialogue("d/1.json", "8", ["い"]),
        )
        _, _, collisions = identity.audit(records)
        self.assertEqual(len(collisions), 1)


class Authentication(unittest.TestCase):
    """A revision label is a claim; the bytes are the evidence."""

    def corpus(self, directory, lines):
        path = pathlib.Path(directory) / "s.jsonl"
        body = "".join(json.dumps(line, ensure_ascii=False) + "\n" for line in lines)
        path.write_text(body, encoding="utf-8")
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def freeze_for(self, sha):
        return {"sources": {"s": {"revision": "rev1", "normalized": {"sha256": sha}}}}

    def line(self, path="d/1.json"):
        return {
            "source": "s",
            "path": path,
            "dialogue_id": "1",
            "condition": None,
            "speakers": ["a"],
            "utterances": [{"utterance_id": "0", "order": 0, "speaker": "a", "text": "あ"}],
        }

    def test_a_matching_file_is_read(self):
        with tempfile.TemporaryDirectory() as directory:
            sha = self.corpus(directory, [self.line()])
            records, problems = identity.authenticated_records(directory, self.freeze_for(sha))
            self.assertEqual(problems, [])
            self.assertEqual(len(records), 1)

    def test_a_modified_file_contributes_nothing_and_says_why(self):
        """**Fails closed.** Resolving against whatever is on disk would make the audit a
        statement about the disk rather than about the corpus the sample was drawn from."""
        with tempfile.TemporaryDirectory() as directory:
            self.corpus(directory, [self.line()])
            records, problems = identity.authenticated_records(
                directory, self.freeze_for("0" * 64)
            )
            self.assertEqual(records, [], "no record may be taken from an unauthenticated file")
            self.assertEqual(len(problems), 1)
            self.assertIn("does not match the frozen digest", problems[0])

    def test_an_absent_file_is_reported_rather_than_skipped(self):
        with tempfile.TemporaryDirectory() as directory:
            records, problems = identity.authenticated_records(
                directory, self.freeze_for("0" * 64)
            )
            self.assertEqual(records, [])
            self.assertIn("absent", problems[0])

    def test_a_freeze_with_no_digest_refuses_rather_than_trusting_the_revision(self):
        with tempfile.TemporaryDirectory() as directory:
            self.corpus(directory, [self.line()])
            records, problems = identity.authenticated_records(
                directory, {"sources": {"s": {"revision": "rev1", "normalized": {}}}}
            )
            self.assertEqual(records, [])
            self.assertIn("pins no digest", problems[0])


if __name__ == "__main__":
    unittest.main()
