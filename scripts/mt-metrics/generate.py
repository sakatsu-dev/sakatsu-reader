"""Translate the pooled corpus through an OpenAI-compatible server, for score.py to read.

    python scripts/mt-metrics/generate.py --label lfm2-t0 --alias lfm2-enjp --temperature 0 --passes 3

Every call is fresh — the server's prompt cache is off and nothing here memoises — so running
`--passes` more than once measures whether the same input gives the same output, which the app's
translation memo assumes and temperature 0 does not guarantee. The first pass is the candidate;
the others are compared with it. Latency is wall-clock per sentence, one request at a time.

Writes `target/evaluation/mt/candidates/<label>.tsv` and `<label>.run.json`.
"""
import argparse
import json
import pathlib
import statistics
import sys
import time

import requests

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
OUT = ROOT / "target" / "evaluation" / "mt"

parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
parser.add_argument("--label", required=True)
parser.add_argument("--endpoint", default="http://127.0.0.1:8141")
parser.add_argument("--alias", required=True, help="the model alias the server must report — anything else is refused")
parser.add_argument("--system", default="Translate to English.", help="LFM2-ENJP-MT's card: required, exactly this")
parser.add_argument("--temperature", type=float, required=True)
parser.add_argument("--min-p", type=float, default=None)
parser.add_argument("--repeat-penalty", type=float, default=None)
parser.add_argument("--seed", type=int, default=0)
parser.add_argument("--max-tokens", type=int, default=512)
parser.add_argument("--passes", type=int, default=1)
args = parser.parse_args()

models = requests.get(f"{args.endpoint}/v1/models", timeout=10).json()
served = [m.get("id") for m in models.get("data", [])]
if args.alias not in served:
    sys.exit(f"{args.endpoint} serves {served}, not {args.alias!r} — refusing a server that is not ours")

pooled = []
for line in (OUT / "pooled.tsv").read_text(encoding="utf-8").splitlines():
    index, japanese, _ = line.split("\t")
    pooled.append((int(index), japanese))

settings = {"temperature": args.temperature, "seed": args.seed, "max_tokens": args.max_tokens,
            "cache_prompt": False}
if args.min_p is not None:
    settings["min_p"] = args.min_p
if args.repeat_penalty is not None:
    settings["repeat_penalty"] = args.repeat_penalty


def translate(japanese):
    body = {"model": args.alias, "messages": [{"role": "system", "content": args.system},
                                               {"role": "user", "content": japanese}], **settings}
    started = time.perf_counter()
    reply = requests.post(f"{args.endpoint}/v1/chat/completions", json=body, timeout=120)
    reply.raise_for_status()
    text = reply.json()["choices"][0]["message"]["content"] or ""
    return " ".join(text.split()), time.perf_counter() - started


passes, seconds = [], []
for n in range(args.passes):
    rows = []
    for index, japanese in pooled:
        text, took = translate(japanese)
        rows.append(text)
        seconds.append(took)
    passes.append(rows)
    print(f"pass {n + 1}: {len(rows)} sentences, {statistics.mean(seconds[-len(rows):]):.3f}s mean", flush=True)

OUT.joinpath("candidates").mkdir(parents=True, exist_ok=True)
with open(OUT / "candidates" / f"{args.label}.tsv", "w", encoding="utf-8", newline="\n") as handle:
    for (index, _), text in zip(pooled, passes[0]):
        handle.write(f"{index}\t{text}\n")
identical = [sum(a == b for a, b in zip(passes[0], other)) for other in passes[1:]]
run = {"label": args.label, "endpoint": args.endpoint, "alias": args.alias, "system": args.system,
       "settings": settings, "sentences": len(pooled), "passes": args.passes,
       "identical_to_first_pass": identical,
       "latency_seconds": {"mean": statistics.mean(seconds), "median": statistics.median(seconds),
                           "p95": sorted(seconds)[int(0.95 * (len(seconds) - 1))]},
       "empty": sum(1 for t in passes[0] if not t)}
(OUT / "candidates" / f"{args.label}.run.json").write_text(json.dumps(run, indent=1), encoding="utf-8")
print(json.dumps(run, indent=1))
