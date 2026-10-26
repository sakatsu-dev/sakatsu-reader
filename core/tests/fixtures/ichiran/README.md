# Ichiran fixtures

Real `ichiran-cli -f` output, captured by `scripts/capture-ichiran-fixtures.sh`.
`project notes` §10 requires the Phase 1 tests run against genuine analyzer output, so
**do not hand-edit these files** — regenerate them with the script. Each case exists
to pin one rule; the script's comments say which.

## Provenance and licence

Per `project notes` §8, data travels with its own licence:

- **Ichiran** (the analyzer that produced the JSON structure) — MIT.
- **JMdict** (the glosses and part-of-speech tags inside it) — © the Electronic
  Dictionary Research and Development Group, licensed **CC BY-SA 4.0**.

These are small excerpts kept for regression testing, attributed here. They are test
data, not a redistributed dictionary: the shipped dictionaries are fetched at
runtime, never committed (§6).
