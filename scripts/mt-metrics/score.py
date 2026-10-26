"""COMET-22 and MetricX-24 for Japanese→English, validated before anything is ranked with them.

    python scripts/mt-metrics/score.py --validate
    python scripts/mt-metrics/score.py --review
    python scripts/mt-metrics/score.py --compare
    python scripts/mt-metrics/score.py --tally

Separate steps, run in that order:

- `--validate` asks whether each metric can tell meaning from wording at all.
- `--review` lays the control set out for a person, with the digest their approval must carry.
- `--compare` scores the one comparison family `controls.json` fixes. It refuses to run until that
  has been approved, and the approval names a digest of everything it covers, so an edit voids it.
  Its blind sheets go into a review package named by that digest, written once, never overwritten.
- `--tally` reads the control judgement sheet back, and reports nothing until every translation
  has an explicit yes, no or unsure.

Everything written goes to `target/evaluation/mt/`, which is ignored: it holds corpus text and
translations.

Inputs, all under `target/evaluation/mt/`:

- `pooled.tsv` — index, Japanese, human English: the 300 pooled sentences, written by
  `cargo test --release -p sakatsu-core --test translation_export -- --ignored`.
- `candidates/<label>.tsv` — index, translation. FuguMT's is written by the same test; an
  OpenAI-compatible server's by `generate.py`.

Environment:

- `HF_HOME` — where `fetch.py` put the models. Nothing is downloaded here: the Hub is set offline.
- `SAKATSU_TATOEBA` — the Tatoeba export directory (jpn_sentences, eng_sentences, jpn-eng_links,
  `.tsv.bz2`), for the two-translations probe and the contrast set. See README.md.
"""
import argparse
import bz2
import functools
import hashlib
import json
import math
import os
import pathlib
import random
import re
import statistics
import sys
import time
import unicodedata

os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
OUT = ROOT / "target" / "evaluation" / "mt"

COMET = ("Unbabel/wmt22-comet-da", "2760a223ac957f30acfb18c8aa649b01cf1d75f2")
XLMR = ("FacebookAI/xlm-roberta-large", "c23d21b0620b635a76227c604d44e43a9f0ee389")
METRICX = ("google/metricx-24-hybrid-large-v2p6-bfloat16", "febb720e29a059df2e8af3ffd71dcdc9e0a24910")
MT5 = ("google/mt5-large", "50b7223e98fcd124b0cabb1ec81bc6324c7df107")

# Pre-registered before any candidate was scored: a sentence where the two systems differ by less
# than this is a tie. COMET is 0–1, higher better; MetricX-24 is 0–25 error, lower better.
TIE = {"comet": 0.01, "metricx": 0.25}
HIGHER_IS_BETTER = {"comet": True, "metricx": False}
PROBE_PAIRS = 200
BOOTSTRAP = 10_000


def snapshot(repo, revision):
    path = (pathlib.Path(os.environ["HF_HOME"]) / "hub" / ("models--" + repo.replace("/", "--"))
            / "snapshots" / revision)
    if not path.is_dir():
        sys.exit(f"{repo}@{revision} not found under HF_HOME — run fetch.py")
    return path


# ── The metrics ─────────────────────────────────────────────────────────────────────────────

class Comet:
    """COMET-22, loaded from the pinned checkpoint with its encoder config read from disk."""

    name = "comet"

    def __init__(self):
        from comet.models.regression.regression_metric import RegressionMetric
        checkpoint = snapshot(*COMET) / "checkpoints" / "model.ckpt"
        # The checkpoint names its encoder `xlm-roberta-large`; pointing that at the pinned local
        # snapshot is the only change from comet's own loader, and `check` compares the two.
        self.model = RegressionMetric.load_from_checkpoint(
            checkpoint, load_pretrained_weights=False, map_location="cpu", strict=False,
            pretrained_model=str(snapshot(*XLMR)), local_files_only=True)
        self.model.eval()

    def score(self, rows):
        data = [{"src": s, "mt": h, "ref": r} for s, h, r in rows]
        # num_workers left at comet's default: on Windows it warns that worker processes can
        # return scores of 0.
        out = self.model.predict(data, batch_size=16, gpus=0, accelerator="cpu", progress_bar=False)
        return [float(x) for x in out.scores]


class MetricX:
    """MetricX-24 hybrid, reference-based, reproducing google-research/metricx@fc4978eb.

    `MT5ForRegression` there is mT5 run for one decoder step from token 0, reading the logit of
    `<extra_id_10>` (250089) and clamping it to 0–25. With `tie_word_embeddings` false — as this
    checkpoint's config has it — that is exactly `MT5ForConditionalGeneration`'s logits, so the
    stock class is used and `check` compares it with the reference class.
    """

    name = "metricx"
    TOKEN = 250089

    def __init__(self):
        import torch
        import transformers
        self.torch = torch
        self.tokenizer = transformers.AutoTokenizer.from_pretrained(str(snapshot(*MT5)))
        model, info = transformers.MT5ForConditionalGeneration.from_pretrained(
            str(snapshot(*METRICX)), output_loading_info=True)
        if model.config.tie_word_embeddings:
            sys.exit("MetricX config ties embeddings; the reference rescales — not reproduced here")
        loose = [k for k in info["missing_keys"] + info["unexpected_keys"] if "position" not in k]
        if loose:
            sys.exit(f"MetricX weights did not load cleanly: {loose[:5]}")
        self.model = model.eval()

    def score(self, rows, batch=8):
        torch = self.torch
        out = []
        for start in range(0, len(rows), batch):
            texts = [f"source: {s} candidate: {h} reference: {r}" for s, h, r in rows[start:start + batch]]
            encoded = [self.tokenizer(t, max_length=1536, truncation=True)["input_ids"][:-1] for t in texts]
            width = max(len(e) for e in encoded)
            ids = torch.tensor([e + [0] * (width - len(e)) for e in encoded])
            mask = torch.tensor([[1] * len(e) + [0] * (width - len(e)) for e in encoded])
            with torch.no_grad():
                logits = self.model(input_ids=ids, attention_mask=mask,
                                    decoder_input_ids=torch.zeros((len(encoded), 1), dtype=torch.long)).logits
            out.extend(float(x) for x in torch.clamp(logits[:, 0, self.TOKEN], 0, 25))
        return out


def metrics():
    torch_threads = os.environ.get("SAKATSU_MT_THREADS")
    if torch_threads:
        import torch
        torch.set_num_threads(int(torch_threads))
    return [Comet(), MetricX()]


def worse(metric, x, than):
    """Is score x worse than `than`, in this metric's direction? Strict: equal is not worse."""
    return x < than if HIGHER_IS_BETTER[metric] else x > than


# ── Inputs ──────────────────────────────────────────────────────────────────────────────────

def read_pooled():
    path = OUT / "pooled.tsv"
    if not path.is_file():
        sys.exit(f"{path} missing — run the translation_export test first")
    rows = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        index, japanese, english = line.split("\t")
        rows[int(index)] = (japanese, english)
    return rows


def digest16(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]


def read_controls(pooled):
    controls = json.loads((HERE / "controls.json").read_text(encoding="utf-8"))
    for item in controls["items"] + controls["clause_drops"]:
        reference = pooled[item["index"]][1]
        if digest16(reference) != item["reference_sha256_16"]:
            sys.exit(f"controls.json item {item['index']}: the pooled corpus has changed under it")
    return controls


def family(controls):
    """The one comparison the approval covers: baseline, candidates, metrics, correction.

    Fixed in `controls.json` and checked for internal consistency, so the Bonferroni family cannot
    shrink by running a candidate on its own.
    """
    spec = controls.get("comparison")
    if not spec:
        sys.exit("controls.json names no comparison family")
    tests = len(spec["candidates"]) * len(spec["metrics"])
    if spec["tests"] != tests or abs(spec["interval_level"] - (1 - 0.05 / tests)) > 1e-12:
        sys.exit(f"controls.json comparison: {len(spec['candidates'])} candidates × {len(spec['metrics'])} "
                 f"metrics is {tests} tests at {1 - 0.05 / tests}, not {spec['tests']} at {spec['interval_level']}")
    if spec["primary"] not in spec["metrics"]:
        sys.exit("controls.json comparison: the primary metric is not among the metrics")
    return spec


def approval_digest(controls):
    """What a reviewer approves, byte for byte.

    Every control and the comparison family; the corpus the controls point into; the contrast set
    the metric validation rests on; and the exact translations of every engine in the family. A
    regenerated translation, an edited control or a changed family each void the approval.
    """
    content = {k: v for k, v in controls.items() if k != "reviewed"}
    parts = [json.dumps(content, ensure_ascii=False, sort_keys=True).encode("utf-8"),
             (OUT / "pooled.tsv").read_bytes(),
             (HERE / "contrast.json").read_bytes()]
    spec = family(controls)
    for label in [spec["baseline"]] + spec["candidates"]:
        path = OUT / "candidates" / f"{label}.tsv"
        if not path.is_file():
            sys.exit(f"{path} missing — every engine in the comparison family must exist before review")
        parts += [label.encode("utf-8"), path.read_bytes()]
    return hashlib.sha256(b"\0".join(parts)).hexdigest()


def read_candidate(label, pooled):
    path = OUT / "candidates" / f"{label}.tsv"
    if not path.is_file():
        sys.exit(f"{path} missing")
    said = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        index, _, text = line.partition("\t")
        said[int(index)] = text
    if set(said) != set(pooled):
        sys.exit(f"{label}: {len(said)} rows for {len(pooled)} sentences")
    return said


@functools.lru_cache(maxsize=1)
def tatoeba():
    """Japanese sentences, English sentences, and the Japanese→English translation links."""
    root = pathlib.Path(os.environ.get("SAKATSU_TATOEBA", ""))
    names = ["jpn_sentences.tsv.bz2", "eng_sentences.tsv.bz2", "jpn-eng_links.tsv.bz2"]
    if not all((root / n).is_file() for n in names):
        sys.exit("SAKATSU_TATOEBA must name a directory holding " + ", ".join(names))

    def rows(name):
        with bz2.open(root / name, "rt", encoding="utf-8") as handle:
            for line in handle:
                yield line.rstrip("\n").split("\t")

    japanese = {r[0]: r[2] for r in rows(names[0])}
    english = {r[0]: " ".join(r[2].split()) for r in rows(names[1])}
    links = {}
    for a, b in rows(names[2]):
        if a in japanese and b in english:
            links.setdefault(a, []).append(b)
    return japanese, english, links


def translations_of(jid):
    """Tatoeba's distinct English translations of one Japanese sentence, sorted."""
    _, english, links = tatoeba()
    return sorted({english[t] for t in links.get(jid, [])})


def tatoeba_pairs(exclude):
    """Japanese sentences Tatoeba holds two different human English translations of.

    Two human translations are *labelled* correct, not proven so — the control self-test has
    already found one that drops a word of meaning — so results on this probe are conditional on
    Tatoeba's labels. Drawn deterministically — the lowest sha256 of the Japanese sentence's
    Tatoeba id — and disjoint from the pooled corpus.
    """
    japanese, _, links = tatoeba()
    pool = []
    for jid in links:
        if japanese[jid] in exclude:
            continue
        distinct = translations_of(jid)
        if len(distinct) >= 2:
            pool.append((hashlib.sha256(jid.encode()).hexdigest(), japanese[jid], distinct[0], distinct[1]))
    pool.sort()
    return [(j, a, b) for _, j, a, b in pool[:PROBE_PAIRS]]


def read_contrast(excluded=False):
    """The same-source contrast set: a rewording labelled correct, and a corruption of one reference.

    Each pair was read and kept only if the second translation looked right, but that is one
    reader's check, recorded for review in `--review`'s sheet. Until a person approves it, contrast
    results are conditional on these labels.

    Items excluded on review stay in the file with the reason; they are returned only when
    `excluded=True`, for the review sheet, and never scored as part of the active set.
    """
    japanese, _, _ = tatoeba()
    contrast = json.loads((HERE / "contrast.json").read_text(encoding="utf-8"))
    rows = []
    for item in contrast["items"]:
        if (item.get("status") == "excluded") != excluded:
            continue
        a, b = translations_of(item["tatoeba_jpn"])[:2]
        if (digest16(a), digest16(b)) != (item["a_sha256_16"], item["b_sha256_16"]):
            sys.exit(f"contrast.json {item['tatoeba_jpn']}: the Tatoeba export has changed under it")
        old, new = item["corrupt"]
        rows.append((item["kind"], japanese[item["tatoeba_jpn"]], a, b, a.replace(old, new), item))
    return rows


# ── Small statistics ─────────────────────────────────────────────────────────────────────────

def sign_test(wins, losses):
    """Exact two-sided binomial sign test."""
    n = wins + losses
    if n == 0:
        return 1.0
    tail = sum(math.comb(n, k) for k in range(max(wins, losses), n + 1)) / 2 ** n
    return min(1.0, 2 * tail)


def better(metric, a, b):
    """+1 if a beats b beyond the tie band, -1 if b beats a, 0 for a tie."""
    gap = (a - b) if HIGHER_IS_BETTER[metric] else (b - a)
    return 0 if abs(gap) < TIE[metric] else (1 if gap > 0 else -1)


def bootstrap_interval(gains, level, seed="mt-metrics"):
    """Percentile bootstrap interval for the mean of paired gains (positive = candidate better)."""
    chooser = random.Random(seed)
    n = len(gains)
    means = sorted(sum(gains[chooser.randrange(n)] for _ in range(n)) / n for _ in range(BOOTSTRAP))
    tail = (1 - level) / 2
    return means[int(tail * BOOTSTRAP)], means[min(BOOTSTRAP - 1, int((1 - tail) * BOOTSTRAP))]


# ── Screening the control set ────────────────────────────────────────────────────────────────

def phrase(pattern):
    head = r"(?<![\w])" if re.match(r"\w", pattern) else ""
    tail = r"(?![\w])" if re.search(r"\w$", pattern) or pattern.endswith("?") else ""
    return re.compile(head + "(?:" + pattern + ")" + tail, re.IGNORECASE)


def screen(item, text):
    """Why this translation needs a person's eyes on the critical element; empty if nothing found.

    A phrase screen, not a judgement: it can miss a wrong translation that happens to contain the
    right words and flag a right one worded unexpectedly. Verdicts come from the blind sheet.
    """
    text = text.replace("’", "'")
    reasons = [f"missing ({' / '.join(group)})" for group in item["accept"]
               if not any(phrase(p).search(text) for p in group)]
    reasons += [f"contains ({p})" for p in item.get("reject", []) if phrase(p).search(text)]
    return reasons


def accepted(item, text):
    return not screen(item, text)


def selftest(controls, pooled):
    """Every control must pass its reference and its reviewed alternatives and flag its corruption.

    A screen that passes its own corruption, or flags a correct rewording a person has signed off,
    cannot be used even as a flag. Tatoeba's other translations of the same sentence, where they
    exist, are checked as alternatives too.
    """
    japanese_ids = {}
    for jid, text in tatoeba()[0].items():
        japanese_ids.setdefault(text, []).append(jid)
    failures = []
    for item in controls["items"]:
        japanese, english = pooled[item["index"]]
        old, new = item["corrupt"]
        if screen(item, english):
            failures.append((item["index"], "reference flagged", english, screen(item, english)))
        corrupted = english.replace(old, new)
        if not screen(item, corrupted):
            failures.append((item["index"], "own corruption passes", corrupted, []))
        others = {t for jid in japanese_ids.get(japanese, []) for t in translations_of(jid)} - {english}
        # A human translation can itself be wrong; one is excluded only by name, with the reason.
        excluded = {entry["sha256_16"] for entry in item.get("excluded_alternatives", [])}
        others = {t for t in others if digest16(t) not in excluded}
        for alternative in item.get("alternatives", []) + sorted(others):
            if screen(item, alternative):
                failures.append((item["index"], "alternative flagged", alternative, screen(item, alternative)))
    return failures


def require_selftest(controls, pooled):
    failures = selftest(controls, pooled)
    if failures:
        for index, what, text, reasons in failures:
            print(f"  control {index}: {what}: {text}  {reasons}")
        sys.exit(f"{len(failures)} control self-test failures — fix controls.json before using it")


# ── Flags ─────────────────────────────────────────────────────────────────────────────────────

JAPANESE = re.compile(r"[぀-ヿ㐀-䶿一-鿿]")


def flags(source, text):
    found = []
    if JAPANESE.search(text):
        found.append("japanese-left")
    digits = set(re.findall(r"\d+", unicodedata.normalize("NFKC", source)))
    if digits and not digits <= set(re.findall(r"\d+", text.replace(",", ""))):
        found.append("digits-differ")
    return found


# ── --validate ────────────────────────────────────────────────────────────────────────────────

def validate(args):
    import sacrebleu
    pooled = read_pooled()
    controls = read_controls(pooled)
    indices = sorted(pooled)
    pairs = tatoeba_pairs({j for j, _ in pooled.values()})
    if len(pairs) < PROBE_PAIRS:
        sys.exit(f"only {len(pairs)} two-translation pairs found")
    contrast = [row[:5] for row in read_contrast()]

    # Three probes over the pooled references, one over Tatoeba's doubly-translated sentences.
    probes = {
        "itself": [(pooled[i][0], pooled[i][1], pooled[i][1]) for i in indices],
        "two human translations": [(j, b, a) for j, a, b in pairs],
        "unrelated": [(pooled[i][0], pooled[k][1], pooled[i][1]) for i, k in zip(indices, reversed(indices))],
    }
    surface = [sacrebleu.sentence_chrf(b, [a]).score for _, a, b in pairs]
    reworded = [k for k, s in enumerate(surface) if s < 50]

    corrupted = []
    for item in controls["items"]:
        japanese, english = pooled[item["index"]]
        old, new = item["corrupt"]
        corrupted.append((item["kind"], item["index"], (japanese, english.replace(old, new), english)))
    for item in controls["clause_drops"]:
        japanese, english = pooled[item["index"]]
        corrupted.append(("clause dropped", item["index"], (japanese, english.replace(item["drop"], ""), english)))

    report = {"probe_pairs": len(pairs), "reworded_pairs": len(reworded),
              "contrast_items_active": len(contrast),
              "contrast_items_excluded": len(read_contrast(excluded=True)),
              "contrast_sha256": hashlib.sha256((HERE / "contrast.json").read_bytes()).hexdigest(),
              "metrics": {}}
    lines = []
    for metric in metrics():
        started = time.time()
        m = metric.name
        scores = {name: metric.score(rows) for name, rows in probes.items()}
        bad = metric.score([rows for _, _, rows in corrupted])
        good_rewording = metric.score([(j, b, a) for _, j, a, b, _ in contrast])
        bad_literal = metric.score([(j, c, a) for _, j, a, _, c in contrast])
        top, good, bottom = (statistics.mean(scores[n]) for n in probes)
        reworded_mean = statistics.mean(scores["two human translations"][k] for k in reworded)
        # Where "two correct translations" sits between perfect and unrelated: 1.0 is perfect.
        resolve = (good - bottom) / (top - bottom) if top != bottom else float("nan")
        resolve_reworded = (reworded_mean - bottom) / (top - bottom) if top != bottom else float("nan")
        median_good = statistics.median(scores["two human translations"])
        median_reworded = statistics.median(scores["two human translations"][k] for k in reworded)
        own = dict(zip(indices, scores["itself"]))

        # 1. Paired: does the corruption score below the very reference it was made from?
        # 2. Diagnostic only: is the penalty large enough to fall below a typical correct rewording
        #    of *other* sentences? That is a question about the size of the penalty, not whether
        #    there is one, and the two must not be confused.
        by_kind = {}
        for (kind, index, _), value in zip(corrupted, bad):
            by_kind.setdefault(kind, []).append((value, own[index]))
        corruptions = {kind: {
            "n": len(v),
            "mean": statistics.mean(x for x, _ in v),
            "below_own_original": sum(1 for x, o in v if worse(m, x, o)),
            "mean_change_from_own_original": statistics.mean(x - o for x, o in v),
            "diagnostic_below_median_rewording_of_other_sentences": sum(1 for x, _ in v if worse(m, x, median_reworded)),
        } for kind, v in by_kind.items()}

        # 3. The ranking question, on one source sentence at a time: a correct rewording against
        #    the reference's own words with the critical element broken.
        contrast_by_kind = {}
        for (kind, *_), g, b in zip(contrast, good_rewording, bad_literal):
            contrast_by_kind.setdefault(kind, []).append((g, b))
        contrasts = {kind: {
            "n": len(v),
            "rewording_preferred": sum(1 for g, b in v if worse(m, b, g)),
            "mean_margin": statistics.mean((g - b) if HIGHER_IS_BETTER[m] else (b - g) for g, b in v),
        } for kind, v in contrast_by_kind.items()}

        report["metrics"][m] = {
            "itself": top, "two_human": good, "two_human_reworded": reworded_mean, "unrelated": bottom,
            "resolution": resolve, "resolution_reworded": resolve_reworded,
            "two_human_median": median_good, "two_human_reworded_median": median_reworded,
            "corruptions": corruptions, "contrast": contrasts, "seconds": time.time() - started,
            "per_corruption": [{"kind": k, "index": i, "score": s, "own_original": own[i]}
                               for (k, i, _), s in zip(corrupted, bad)],
            "per_contrast": [{"kind": c[0], "rewording": g, "corrupted_literal": b}
                             for c, g, b in zip(contrast, good_rewording, bad_literal)],
            "per_probe": scores,
        }
        arrow = "higher better" if HIGHER_IS_BETTER[m] else "lower better"
        lines += [f"\n=== {m} ({arrow}) ===",
                  f"  reference itself              {top:8.3f}",
                  f"  two human translations        {good:8.3f}   ({len(pairs)} Tatoeba pairs)",
                  f"    of which reworded (chrF<50) {reworded_mean:8.3f}   ({len(reworded)} pairs)",
                  f"  unrelated sentence            {bottom:8.3f}",
                  f"  resolution (1 = two correct translations score as the reference itself, "
                  f"0 = as an unrelated sentence): {resolve:.2f}, reworded {resolve_reworded:.2f}",
                  "  corrupted control references, scored against their own original:"]
        for kind, c in corruptions.items():
            lines.append(f"    {kind:<16} n={c['n']:>2}  below own original {c['below_own_original']:>2}/{c['n']}"
                         f"  mean change {c['mean_change_from_own_original']:+7.3f}"
                         f"   (diagnostic: below median rewording of other sentences "
                         f"{c['diagnostic_below_median_rewording_of_other_sentences']:>2}/{c['n']})")
        lines.append(f"  contrast set, {len(contrast)} active items ({report['contrast_items_excluded']} excluded on "
                     "review) — same source: correct rewording preferred over the corrupted reference wording:")
        for kind, c in contrasts.items():
            lines.append(f"    {kind:<16} {c['rewording_preferred']:>2}/{c['n']}   mean margin {c['mean_margin']:+7.3f}")
        total = sum(c["rewording_preferred"] for c in contrasts.values())
        lines.append(f"    {'all':<16} {total:>2}/{len(contrast)}")
    print("\n".join(lines))
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "validation.json").write_text(json.dumps(report, indent=1), encoding="utf-8")
    print(f"\nwritten: {OUT / 'validation.json'}")


# ── --compare ─────────────────────────────────────────────────────────────────────────────────

def require_approval(controls):
    reviewed = controls.get("reviewed")
    if not isinstance(reviewed, dict) or not reviewed.get("by") or not reviewed.get("on"):
        sys.exit("controls.json has not been reviewed — no candidate is scored before it is "
                 "(run --review for what to approve)")
    if reviewed.get("digest") != approval_digest(controls):
        sys.exit("the controls, the comparison family, the corpus, the contrast set or a candidate's "
                 "translations changed after review — the approval no longer covers them; review again")


def package_dir(controls):
    """Where one approval's review package lives: named by the digest, so never shared."""
    return OUT / f"review-{approval_digest(controls)[:16]}"


def compare(args):
    mirror_root()  # refuse before any scoring, not after
    pooled = read_pooled()
    controls = read_controls(pooled)
    require_approval(controls)
    require_selftest(controls, pooled)
    spec = family(controls)
    labels = [spec["baseline"]] + spec["candidates"]
    baseline, tests, level = spec["baseline"], spec["tests"], spec["interval_level"]
    indices = sorted(pooled)
    candidates = {label: read_candidate(label, pooled) for label in labels}
    scores = {}
    for metric in metrics():
        for label, said in candidates.items():
            scores[(metric.name, label)] = metric.score(
                [(pooled[i][0], said[i], pooled[i][1]) for i in indices])

    lines = [f"\n=== {len(indices)} sentences; paired against {baseline}; the approved family: "
             f"{tests} tests, intervals at {level:.4f}; primary {spec['primary']} ==="]
    summary = {"family": spec, "candidates": {}}
    for label, said in candidates.items():
        row = {"screen_flagged": {}, "flags": {}}
        for item in controls["items"]:
            reasons = screen(item, said[item["index"]])
            if reasons:
                row["screen_flagged"][item["index"]] = reasons
        for i in indices:
            for flag in flags(pooled[i][0], said[i]):
                row["flags"].setdefault(flag, []).append(i)
        for metric in spec["metrics"]:
            values = scores[(metric, label)]
            row[metric] = statistics.mean(values)
            if label != baseline:
                base = scores[(metric, baseline)]
                calls = [better(metric, a, b) for a, b in zip(values, base)]
                wins, losses = calls.count(1), calls.count(-1)
                p = sign_test(wins, losses)
                gains = [(a - b) if HIGHER_IS_BETTER[metric] else (b - a) for a, b in zip(values, base)]
                low, high = bootstrap_interval(gains, level)
                row[f"{metric}_paired"] = {"wins": wins, "ties": calls.count(0), "losses": losses,
                                           "p": p, "significant": p * tests < 0.05,
                                           "mean_gain": statistics.mean(gains), "interval": [low, high]}
        summary["candidates"][label] = row
        lines.append(f"  {label:<14} COMET {row['comet']:.4f}   MetricX {row['metricx']:.3f}   "
                     f"screen flags {len(row['screen_flagged'])}/{len(controls['items'])}   "
                     + "  ".join(f"{k} {len(v)}" for k, v in row["flags"].items()))
        for metric in spec["metrics"]:
            paired = row.get(f"{metric}_paired")
            if paired:
                lines.append(f"      {metric:<8} {paired['wins']} wins, {paired['ties']} ties, {paired['losses']} losses"
                             f"  p={paired['p']:.4f}{'  significant after correction' if paired['significant'] else ''}"
                             f"   mean gain {paired['mean_gain']:+.4f}  [{paired['interval'][0]:+.4f}, {paired['interval'][1]:+.4f}]")

    disagreements = {}
    for label in spec["candidates"]:
        disagreements[label] = [i for k, i in enumerate(indices)
                                if better("comet", scores[("comet", label)][k], scores[("comet", baseline)][k])
                                * better("metricx", scores[("metricx", label)][k], scores[("metricx", baseline)][k]) < 0]
        summary["candidates"][label]["metrics_disagree"] = disagreements[label]
        lines.append(f"  {label}: the metrics disagree on {len(disagreements[label])} sentences")

    # The review package: written once per approval and never overwritten, so a rerun cannot
    # erase answers a person has already given.
    package = package_dir(controls)
    if (package / "manifest.json").is_file():
        lines.append(f"  review package {package.name} already exists — left untouched, answers preserved")
    else:
        if package.exists() and any(package.iterdir()):
            sys.exit(f"{package} holds files but no manifest — an interrupted run; look before removing it")
        package.mkdir(parents=True, exist_ok=True)
        manifest = {"approval_digest": approval_digest(controls), "family": spec,
                    "candidate_sha256": {label: hashlib.sha256((OUT / "candidates" / f"{label}.tsv").read_bytes()).hexdigest()
                                         for label in labels},
                    "controls": write_control_sheet(package, controls, pooled, candidates),
                    "disagreements": {label: write_sheet(package, label, baseline, disagreements[label], pooled, candidates)
                                      for label in spec["candidates"]}}
        manifest["controls_skeleton_sha256"] = hashlib.sha256(
            skeleton((package / "controls-judgement.md").read_text(encoding="utf-8")).encode("utf-8")).hexdigest()
        (package / "manifest.json").write_text(json.dumps(manifest, indent=1), encoding="utf-8")
        lines.append(f"  review package written: {package} — controls-judgement.md is where the control "
                     f"verdicts come from (read back by --tally); sheet-<label>.md for the disagreements")

    print("\n".join(lines))
    (package / "comparison.json").write_text(json.dumps(summary, indent=1), encoding="utf-8")
    with open(package / "segment-scores.tsv", "w", encoding="utf-8", newline="\n") as handle:
        handle.write("index\t" + "\t".join(f"{m}:{l}" for m in spec["metrics"] for l in labels) + "\n")
        for k, i in enumerate(indices):
            handle.write(f"{i}\t" + "\t".join(f"{scores[(m, l)][k]:.4f}" for m in spec["metrics"]
                                             for l in labels) + "\n")
    print(f"\nwritten: {package / 'comparison.json'}, {package / 'segment-scores.tsv'}")
    mirror()


def write_sheet(package, label, baseline, indices, pooled, candidates):
    """A blind side-by-side: A and B shuffled per sentence. Returns the key, kept in the manifest."""
    chooser = random.Random(f"sheet:{label}:{baseline}")
    sheet, key = [f"# Which translation is better? ({len(indices)} sentences)\n",
                  "After each `verdict:` write A, B, same, or unsure. The human reference is given for "
                  "orientation, not as the answer.\n"], []
    for n, i in enumerate(indices, 1):
        japanese, reference = pooled[i]
        pair = [(baseline, candidates[baseline][i]), (label, candidates[label][i])]
        chooser.shuffle(pair)
        key.append({"n": n, "index": i, "A": pair[0][0], "B": pair[1][0]})
        sheet.append(f"\n## {n}\n\n{japanese}\n\n*reference:* {reference}\n\n- **A:** {pair[0][1]}\n- **B:** {pair[1][1]}\n- verdict: \n")
    (package / f"sheet-{label}.md").write_text("".join(sheet), encoding="utf-8")
    return key


def write_control_sheet(package, controls, pooled, candidates):
    """Every distinct translation of every control item, shuffled and unlabelled, for a person.

    Identical translations are shown once, so a reader judges each distinct wording one time.
    Returns the key — item, choice, which engines said it, and the exact text — for the manifest.
    """
    chooser = random.Random("controls-judgement")
    sheet = ["# Does each translation keep the critical element?\n",
             "After each `keeps:` write **yes**, **no**, or **unsure**. Judge only the named element — not "
             "style, not the rest of the sentence. The reference is for orientation. Unsure is a real "
             "answer: it is kept as unresolved, never counted either way. Do not open manifest.json "
             "until every answer is in: it holds the key.\n",
             "\njudged by: \n"]
    key = []
    for n, item in enumerate(controls["items"], 1):
        japanese, reference = pooled[item["index"]]
        wordings = {}
        for label, said in candidates.items():
            wordings.setdefault(said[item["index"]], []).append(label)
        shown = list(wordings.items())
        chooser.shuffle(shown)
        note = f"judge: {item['judge_note']}  \n" if item.get("judge_note") else ""
        sheet.append(f"\n## {n}\n\n{japanese}\n\nelement: `{item['element']}` ({item['kind']})  \n{note}"
                     f"*reference:* {reference}\n")
        for k, (text, labels) in enumerate(shown, 1):
            sheet.append(f"\n{k}. {text}  \n   keeps: \n")
            key.append({"n": n, "index": item["index"], "choice": k, "labels": labels, "text": text})
    # The copy the answers are checked against; the working copy is the one a judge fills in.
    (package / "controls-judgement.original.md").write_text("".join(sheet), encoding="utf-8")
    (package / "controls-judgement.md").write_text("".join(sheet), encoding="utf-8")
    return key


# ── --mirror: a verified copy outside build output ──────────────────────────────────────────────

def mirror_root():
    root = os.environ.get("SAKATSU_MT_MIRROR")
    if not root:
        sys.exit("SAKATSU_MT_MIRROR must name a directory outside target/ for a verified copy of the "
                 "evaluation — target/ has been wiped once already")
    root = pathlib.Path(root).resolve()
    if root.is_relative_to((ROOT / "target").resolve()):
        sys.exit("SAKATSU_MT_MIRROR is inside target/, which is exactly what it exists to survive")
    return root


def mirror():
    """Copy the corpus, the candidates and every review package out of target/, checked by hash.

    Nothing is overwritten: a file that already exists with different bytes is kept, and the new
    version is written beside it with a timestamp, so answers given in a package survive a
    mirror of an older copy and the other way round. SHA256SUMS lists what the mirror holds.
    """
    root = mirror_root()
    sources = [OUT / "pooled.tsv", *sorted((OUT / "candidates").glob("*")),
               *sorted(p for d in OUT.glob("review-*") for p in d.rglob("*") if p.is_file())]
    copied = kept = 0
    for source in sources:
        if not source.is_file():
            continue
        target = root / source.relative_to(OUT)
        target.parent.mkdir(parents=True, exist_ok=True)
        data = source.read_bytes()
        if target.is_file():
            if target.read_bytes() == data:
                kept += 1
                continue
            # A changed version is named by its own content, so two versions can never share a
            # name however quickly they follow each other, and the same version is stored once.
            target = target.with_name(f"{target.stem}.{hashlib.sha256(data).hexdigest()[:16]}{target.suffix}")
            if target.is_file():
                if target.read_bytes() != data:
                    sys.exit(f"{target}: holds different bytes under this content's name — stopping")
                kept += 1
                continue
        with open(target, "xb") as handle:  # exclusive: never replaces an existing file
            handle.write(data)
        if target.read_bytes() != data:
            sys.exit(f"{target}: the copy does not match its source")
        copied += 1
    sums = sorted(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(root).as_posix()}"
                  for p in root.rglob("*") if p.is_file() and p.name != "SHA256SUMS")
    (root / "SHA256SUMS").write_text("\n".join(sums) + "\n", encoding="utf-8")
    print(f"mirror {root}: {copied} copied, {kept} already identical, {len(sums)} files listed in SHA256SUMS")


# ── --tally: reading the judgement sheet back ─────────────────────────────────────────────────

YES, NO, UNSURE = {"y", "yes"}, {"n", "no"}, {"unsure", "not sure", "?", "unclear", "can't tell"}


def classify(answer):
    """yes / no / unsure, or None for blank, or 'invalid' — never a guess from a first letter."""
    text = answer.strip().strip(".").strip().lower()
    if not text:
        return None
    if text in YES:
        return "yes"
    if text in NO:
        return "no"
    if text in UNSURE:
        return "unsure"
    return "invalid"


def judged_by(sheet):
    """Who answered the sheet — a person or an agent — stated on it, never assumed."""
    for line in sheet.splitlines():
        if line.startswith("judged by:"):
            return line.split(":", 1)[1].strip() or None
    return None


def skeleton(sheet):
    """The sheet with every answer blanked: everything a judge must not change."""
    out = []
    for line in sheet.splitlines():
        if line.startswith("judged by:"):
            line = "judged by:"
        elif line.strip().startswith("keeps:"):
            line = line[:line.index("keeps:")] + "keeps:"
        out.append(line.rstrip())
    return "\n".join(out).rstrip() + "\n"


def parse_judgements(sheet, key, original_skeleton_sha256=None):
    """Map every expected (item, choice) to its answer; refuse anything missing, extra or doubled.

    An answer counts only beside the exact translation the key recorded for it, so a translation
    edited or moved on the sheet cannot carry a verdict to the wrong engine. With the package's
    skeleton hash, any change to the sheet other than the answers is refused as well.
    """
    found, shown, item, choice = {}, {}, None, None
    problems = [] if judged_by(sheet) else ["the sheet does not say who judged it (`judged by:`)"]
    if original_skeleton_sha256 is not None:
        if hashlib.sha256(skeleton(sheet).encode("utf-8")).hexdigest() != original_skeleton_sha256:
            problems.append("the sheet's text differs from the package as written — only the answers "
                            "may change (compare it with controls-judgement.original.md)")
    for line in sheet.splitlines():
        if line.startswith("## "):
            item, choice = int(line[3:].strip()), None
        elif re.match(r"^\d+\. ", line):
            choice = int(line.split(".", 1)[0])
            shown[(item, choice)] = line.split(". ", 1)[1].rstrip()
        elif line.strip().startswith("keeps:"):
            if (item, choice) in found:
                problems.append(f"item {item} choice {choice} answered twice")
            found[(item, choice)] = line.split(":", 1)[1]
    expected = {(k["n"], k["choice"]) for k in key}
    problems += [f"item {n} choice {c} is missing from the sheet" for n, c in sorted(expected - set(found))]
    problems += [f"item {n} choice {c} is not in the key" for n, c in sorted(set(found) - expected)]
    for k in key:
        spot = (k["n"], k["choice"])
        if spot in shown and shown[spot] != k["text"]:
            problems.append(f"item {spot[0]} choice {spot[1]}: the translation shown is not the one the key "
                            "recorded — the answer cannot be attributed")
    answers = {}
    for spot, raw in found.items():
        verdict = classify(raw)
        if verdict is None:
            problems.append(f"item {spot[0]} choice {spot[1]} not yet answered")
        elif verdict == "invalid":
            problems.append(f"item {spot[0]} choice {spot[1]}: {raw.strip()!r} is not yes, no or unsure")
        answers[spot] = verdict
    return answers, problems


def check_key_labels(key, labels):
    """Every engine the key names is in the family, and every engine in the family is named."""
    named = {name for entry in key for name in entry["labels"]}
    if named - set(labels):
        sys.exit(f"the key names engines outside the approved family: {sorted(named - set(labels))}")
    if set(labels) - named:
        sys.exit(f"the key never names {sorted(set(labels) - named)} — the sheet does not cover the family")


def control_verdict(kept, lost, unresolved, base_kept, base_lost, base_unresolved):
    """Does the candidate keep at least as many elements as the baseline — on every resolution?

    Each unresolved answer could go either way, so the verdict is decided only if it holds in the
    candidate's worst case against the baseline's best (passes), or fails even in the candidate's
    best case against the baseline's worst (fails). Anything between is undetermined.
    """
    if len(kept) >= len(base_kept) + len(base_unresolved):
        return "passes"
    if len(kept) + len(unresolved) < len(base_kept):
        return "fails"
    return "undetermined"


def tally(args):
    # The answers are copied out of target/ before they are read, whatever the tally then finds.
    mirror()
    pooled = read_pooled()
    controls = read_controls(pooled)
    require_approval(controls)
    package = package_dir(controls)
    manifest = json.loads((package / "manifest.json").read_text(encoding="utf-8"))
    spec = manifest["family"]
    labels = [spec["baseline"]] + spec["candidates"]
    for label in labels:
        have = hashlib.sha256((OUT / "candidates" / f"{label}.tsv").read_bytes()).hexdigest()
        if have != manifest["candidate_sha256"][label]:
            sys.exit(f"{label}'s translations changed after the sheet was written")
    check_key_labels(manifest["controls"], labels)
    sheet = (package / "controls-judgement.md").read_text(encoding="utf-8")
    answers, problems = parse_judgements(sheet, manifest["controls"], manifest["controls_skeleton_sha256"])
    if problems:
        for problem in problems[:30]:
            print("  " + problem)
        sys.exit(f"{len(problems)} problems in {package / 'controls-judgement.md'} — no totals until every "
                 "translation has an explicit answer")
    print(f"  control verdicts judged by: {judged_by(sheet)}")
    outcome = {label: {"yes": [], "no": [], "unsure": []} for label in labels}
    for entry in manifest["controls"]:
        for label in entry["labels"]:
            outcome[label][answers[(entry["n"], entry["choice"])]].append(entry["index"])
    expected = len(controls["items"])
    base = outcome[spec["baseline"]]
    for label in labels:
        o = outcome[label]
        counted = len(o["yes"]) + len(o["no"]) + len(o["unsure"])
        if counted != expected:
            sys.exit(f"{label}: {counted} answers for {expected} control items")
        line = f"  {label:<14} keeps {len(o['yes'])}, loses {len(o['no'])}, unresolved {len(o['unsure'])} of {expected}"
        if label != spec["baseline"]:
            verdict = control_verdict(o["yes"], o["no"], o["unsure"], base["yes"], base["no"], base["unsure"])
            regressed = sorted(set(o["no"]) & set(base["yes"]))
            maybe = sorted((set(o["no"]) | set(o["unsure"])) & (set(base["yes"]) | set(base["unsure"])) - set(regressed))
            line += (f"   against {spec['baseline']}: {verdict}; loses where it keeps: {regressed or 'none'}"
                     + (f"; unresolved either side: {maybe}" if maybe else ""))
        print(line)


# ── --review: everything the approval covers, laid out for the person who signs it off ────────

def review(args):
    pooled = read_pooled()
    controls = read_controls(pooled)
    spec = family(controls)
    failures = selftest(controls, pooled)
    lines = ["# Control set, comparison family and contrast set — for review\n",
             "The approval covers everything on this page, plus the exact translations of every engine "
             "in the family.\n",
             "\n## The comparison family\n\n"
             f"- baseline: `{spec['baseline']}`; candidates: {', '.join(f'`{c}`' for c in spec['candidates'])}\n"
             f"- metrics: {', '.join(spec['metrics'])}; primary: **{spec['primary']}**\n"
             f"- Bonferroni over {spec['tests']} tests; intervals at {spec['interval_level']:.4f}\n",
             "\n## The control set\n\n"
             "Each item: the sentence, its critical element, the phrase screen (every accept group must "
             "match; no reject phrase may), the corruption `--validate` tests the metrics with, and "
             "correct alternative wordings the screen must pass. The screen only flags translations for "
             "reading; verdicts on the controls come from the blind judgement sheet `--compare` writes.\n",
             f"\n**Self-test:** {'passes' if not failures else str(len(failures)) + ' failures — listed at the end'}.\n"]
    for item in controls["items"]:
        japanese, english = pooled[item["index"]]
        old, new = item["corrupt"]
        groups = " **and** ".join("(" + " / ".join(group) + ")" for group in item["accept"])
        rejects = " / ".join(item.get("reject", [])) or "—"
        alternatives = "".join(f"  \nalternative: {a}" for a in item.get("alternatives", []))
        excluded = "".join(f"  \nexcluded Tatoeba translation: {e['text']} — {e['why']}"
                           for e in item.get("excluded_alternatives", []))
        excluded += f"  \njudge: {item['judge_note']}" if item.get("judge_note") else ""
        lines.append(f"\n**{item['index']}** · {item['kind']} · `{item['element']}`  \n{japanese}  \n"
                     f"*{english}*  \naccept: {groups}  \nreject: {rejects}  \n"
                     f"corrupted: {english.replace(old, new)}{alternatives}{excluded}\n")
    lines.append("\n### Clause drops\n")
    for item in controls["clause_drops"]:
        japanese, english = pooled[item["index"]]
        note = f"  \nnote: {item['note']}" if item.get("note") else ""
        lines.append(f"\n**{item['index']}** {japanese}  \n*{english}*  \n→ {english.replace(item['drop'], '')}{note}\n")
    lines.append("\n## The contrast set\n\nEach: the reference A, the rewording B labelled correct, and "
                 "the corruption A′. The metric validation assumes B is correct and A′ is wrong; check both.\n")
    for kind, japanese, a, b, corrupted, item in read_contrast():
        added = f"  \n*added {item['added_on']}, replacing {item['replaces']}*" if item.get("replaces") else ""
        lines.append(f"\n{kind} · {japanese}  \nA: {a}  \nB: {b}  \nA′: {corrupted}{added}\n")
    excluded = read_contrast(excluded=True)
    if excluded:
        lines.append("\n### Excluded on review — kept for the record, not scored\n")
        for kind, japanese, a, b, corrupted, item in excluded:
            lines.append(f"\n{kind} · {japanese} (Tatoeba {item['tatoeba_jpn']}, excluded {item['excluded_on']}, "
                         f"replaced by {item['replaced_by']})  \nA: {a}  \nB: {b}  \nA′: {corrupted}  \n"
                         f"why: {item['why']}\n")
    if failures:
        lines.append("\n## Self-test failures\n")
        lines += [f"\n- {index}: {what}: {text} {reasons}\n" for index, what, text, reasons in failures]
    digest = approval_digest(controls)
    lines.append(f"\n## Approval\n\nTo approve exactly this content, set in `controls.json`:\n\n"
                 f"```json\n\"reviewed\": {{\"by\": \"<name>\", \"on\": \"<YYYY-MM-DD>\", \"digest\": \"{digest}\"}}\n```\n\n"
                 "Any later edit to the controls, the family, the corpus, the contrast set or a candidate's "
                 "translations changes the digest and voids the approval.\n")
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "controls-review.md").write_text("".join(lines), encoding="utf-8")
    print(f"self-test: {'passes' if not failures else f'{len(failures)} failures'}")
    for index, what, text, reasons in failures:
        print(f"  control {index}: {what}: {text}  {reasons}")
    print(f"approval digest: {digest}")
    print(f"written: {OUT / 'controls-review.md'}")


# ── --check: the loaders here against the libraries' own ──────────────────────────────────────

def check(args):
    """Same inputs through comet's stock loader and the reference MT5ForRegression; must agree.

    Run with HF_HUB_OFFLINE=0 and TRANSFORMERS_OFFLINE=0: comet's stock loader looks its encoder
    up by name (`xlm-roberta-large`) and fetches that small config once.
    """
    import torch
    pooled = read_pooled()
    rows = [(pooled[i][0], pooled[(i + 7) % len(pooled)][1] if i % 3 == 0 else pooled[i][1], pooled[i][1])
            for i in sorted(pooled)[:24]]
    ours = Comet().score(rows)
    from comet import load_from_checkpoint
    stock = load_from_checkpoint(str(snapshot(*COMET) / "checkpoints" / "model.ckpt"))
    theirs = [float(x) for x in stock.predict([{"src": s, "mt": h, "ref": r} for s, h, r in rows], batch_size=16,
                                                gpus=0, accelerator="cpu", progress_bar=False).scores]
    print("COMET   max |ours - stock| =", max(abs(a - b) for a, b in zip(ours, theirs)))
    ours = MetricX().score(rows)
    sys.path.insert(0, str(pathlib.Path(args.reference)))
    from metricx24 import models as reference  # google-research/metricx@fc4978eb, read-only
    model = reference.MT5ForRegression.from_pretrained(str(snapshot(*METRICX))).eval()
    tokenizer = MetricX().tokenizer
    theirs = []
    for s, h, r in rows:
        ids = tokenizer(f"source: {s} candidate: {h} reference: {r}", max_length=1536, truncation=True)["input_ids"][:-1]
        with torch.no_grad():
            # No cache: the reference predates transformers' cache objects, and a single decoder
            # step has nothing to cache anyway.
            theirs.append(float(model(input_ids=torch.tensor([ids]), attention_mask=torch.ones(1, len(ids), dtype=torch.long),
                                      use_cache=False).predictions[0]))
    print("MetricX max |ours - reference| =", max(abs(a - b) for a, b in zip(ours, theirs)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--validate", action="store_true")
    parser.add_argument("--compare", action="store_true", help="score the approved comparison family")
    parser.add_argument("--tally", action="store_true", help="read the approved package's judgement sheet back")
    parser.add_argument("--check", dest="reference", help="a checkout of google-research/metricx@fc4978eb")
    parser.add_argument("--review", action="store_true", help="write the control set out for review")
    parser.add_argument("--mirror", action="store_true", help="copy the evaluation to SAKATSU_MT_MIRROR")
    args = parser.parse_args()
    if args.review:
        review(args)
    elif args.mirror:
        mirror()
    elif args.validate:
        validate(args)
    elif args.compare:
        compare(args)
    elif args.tally:
        tally(args)
    elif args.reference:
        check(args)
    else:
        parser.print_help()
