"""Frozen Japanese BERT target states + work-split Aozora reading classifiers.

Test-only. Uses preregistered settings and gates for the target-reading experiment.
No evaluation reading trains a classifier. No weights or predictions enter the application.
"""
import collections
import argparse
import csv
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import time
import unicodedata

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "scratch/context-next"
sys.path.insert(0, str(WORK / "python"))
REVISION = "01256ee45494fea11d9fc3d0eb2cee88d5340266"

def sentence_key(text):
    return "".join(unicodedata.normalize("NFKC", text).split())

def target_parts(row):
    raw = row["text"].encode("utf-8")
    start, end = row["start"], row["end"]
    assert raw[start:end].decode("utf-8") == row["surface"]
    return raw[:start].decode("utf-8"), row["surface"], raw[end:].decode("utf-8")

def select_reading(candidates, labels, support, scores):
    if len(candidates) < 2:
        return None, "fewer_than_two_candidates"
    if any(c not in support or support[c]["examples"] < 3 or support[c]["works"] < 2 for c in candidates):
        return None, "insufficient_candidate_support"
    ranked = sorted(((float(scores[labels.index(c)]), c) for c in candidates), reverse=True)
    if ranked[0][0] - ranked[1][0] < .2:
        return None, "margin_below_0.2"
    return ranked[0][1], "decided"

def observe(rows, predictions, label, np):
    aligned = [r for r in rows if r.get("baseline") is not None]
    eligible = [r for r in aligned if r.get("eligible")]
    decided = fixed = broken = right = baseline_right = 0
    groups = collections.defaultdict(lambda: [0, 0])
    details = []
    for row in aligned:
        was = row["baseline"] in row["accepted"]
        baseline_right += was
        prediction = predictions.get(row["id"]) if row.get("eligible") else None
        now = was if prediction is None else prediction in row["accepted"]
        if prediction is not None:
            decided += 1
            right += now
            fixed += now and not was
            broken += was and not now
        group = row.get("group", row["surface"])
        groups[group][0] += int(now) - int(was)
        groups[group][1] += 1
        details.append({"id": row["id"], "prediction": prediction, "before_right": was, "after_right": now})
    ci = [0.0, 0.0]
    if groups:
        grouped = np.array(list(groups.values()), dtype=float)
        indices = np.random.default_rng(20260906).integers(len(grouped), size=(2000, len(grouped)))
        samples = grouped[indices].sum(axis=1)
        ci = np.quantile(samples[:, 0] / samples[:, 1], [.025, .975]).tolist()
    missing = sum(not any(c["reading"] in r["accepted"] for c in r["candidates"]) for r in aligned)
    result = {"label": label, "aligned": len(aligned), "eligible": len(eligible),
        "baseline_right": baseline_right, "decisions": decided, "model_right": right,
        "fixed": fixed, "broken": broken, "net": fixed-broken,
        "fallback_right": baseline_right+fixed-broken, "gold_candidate_missing": missing,
        "groups": len(groups), "grouped_net_rate_95ci": ci,
        "numerical_gate": decided >= 30 and fixed > 0 and fixed >= 5*broken and ci[0] > 0,
        "details": details}
    print(json.dumps({k:v for k,v in result.items() if k != "details"}), flush=True)
    return result

def main():
    import numpy as np
    import torch
    from transformers import BertJapaneseTokenizer, BertModel
    import psutil
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmark", action="store_true", help="measure 20 warm single-target CPU requests")
    args = parser.parse_args()

    torch.set_num_threads(4)
    fixture = json.loads((WORK / "reading-pipeline.json").read_text(encoding="utf-8"))["cases"]
    captures = json.loads((WORK / "capture-pipeline.json").read_text(encoding="utf-8"))["cases"]
    external = fixture + captures
    excluded = {sentence_key(r["text"]) for r in external}
    bank_path = ROOT / "scratch/context-plan/aozora/readings.tsv"
    bank = list(csv.DictReader(bank_path.open(encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE))
    train, validation = [], []
    for row in bank:
        start = len(row["sentence"].split(row["surface"], 1)[0].encode("utf-8"))
        case = {"id": f"aozora-{row['work_id']}-{start}-{len(train)+len(validation)}",
            "text": row["sentence"], "surface": row["surface"], "start": start,
            "end": start+len(row["surface"].encode()), "reading": row["reading"], "work_id": row["work_id"]}
        if sentence_key(case["text"]) in excluded:
            continue
        split = int(hashlib.sha256(row["work_id"].encode()).hexdigest(),16) % 5
        (validation if split == 0 else train).append(case)
    validation_keys = {sentence_key(r["text"]) for r in validation}
    train = [r for r in train if sentence_key(r["text"]) not in validation_keys]
    print(f"work split: train {len(train)}, validation {len(validation)}; external {len(external)}", flush=True)

    encoder = WORK / "encoder"
    spec = importlib.util.spec_from_file_location("encoder_fetch", Path(__file__).with_name("fetch-reading-encoder.py"))
    fetch = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fetch)
    assert all(fetch.verify(encoder / name, expected) for name, expected in fetch.FILES.items()), "encoder provenance changed"
    tokenizer = BertJapaneseTokenizer.from_pretrained(encoder, local_files_only=True)
    model = BertModel.from_pretrained(encoder, local_files_only=True, use_safetensors=True).eval()
    cache = WORK / "target-vectors"
    cache.mkdir(exist_ok=True)
    vectors = {}
    elapsed = []
    rows = train + validation + external
    def token_ids(row):
        left, target, right = target_parts(row)
        left = tokenizer.encode(left, add_special_tokens=False)
        target = tokenizer.encode(target, add_special_tokens=False)
        right = tokenizer.encode(right, add_special_tokens=False)
        assert target and len(target) < 190, "target cannot be empty or truncated"
        room = 190 - len(target)
        left = left[-min(len(left), room//2):] if left and room//2 else []
        right = right[:room-len(left)]
        return ([tokenizer.cls_token_id]+left+target+right+[tokenizer.sep_token_id],
                1+len(left), 1+len(left)+len(target))
    if args.benchmark:
        samples = []
        # Fixed training inputs keep this cost check independent of evaluation correctness.
        for i in range(21):
            began = time.perf_counter()
            ids, start, end = token_ids(train[i % len(train)])
            with torch.inference_mode():
                states = model(input_ids=torch.tensor([ids])).last_hidden_state
                states[0, start:end].mean(dim=0).numpy().copy()
            if i: samples.append(time.perf_counter()-began)
        benchmark = {"requests":20,"threads":4,"device":"cpu","warmup_requests":1,
            "includes":"tokenization, single-sequence encoder forward, target pooling; excludes model load and IPC",
            "median_seconds":float(np.median(samples)),"p95_seconds":float(np.quantile(samples,.95)),
            "peak_working_set_bytes":getattr(psutil.Process().memory_info(),"peak_wset",None)}
        (WORK/'reading-encoder-benchmark.json').write_text(json.dumps(benchmark,indent=2),encoding='utf-8')
    def key(row):
        return hashlib.sha256(json.dumps([REVISION,"target-parts-v1",row["text"],row["start"],row["end"]],ensure_ascii=False).encode()).hexdigest()
    pending = {}
    for row in rows:
        k=key(row)
        path=cache/f"{k}.npy"
        if path.exists():
            value=np.load(path,allow_pickle=False)
            assert value.shape == (768,) and np.isfinite(value).all()
            vectors[k]=value
        else:
            pending[k]=row
    pending=list(pending.items())
    for at in range(0,len(pending),16):
        chunk=pending[at:at+16]
        records=[]
        for k,row in chunk:
            ids,start,end=token_ids(row)
            records.append((k,ids,start,end))
        width=max(len(r[1]) for r in records)
        ids=torch.tensor([r[1]+[tokenizer.pad_token_id]*(width-len(r[1])) for r in records])
        mask=ids.ne(tokenizer.pad_token_id).long()
        began=time.perf_counter()
        with torch.inference_mode():
            states=model(input_ids=ids,attention_mask=mask).last_hidden_state
        elapsed.append((time.perf_counter()-began)/len(records))
        for i,(k,_,start,end) in enumerate(records):
            vector=states[i,start:end].mean(dim=0).numpy().copy()
            assert np.isfinite(vector).all()
            vectors[k]=vector
            np.save(cache/f"{k}.npy",vector,allow_pickle=False)
        if at % 160 == 0:
            print(f"target vectors {min(at+16,len(pending))}/{len(pending)} missing",flush=True)
    def feature(row):
        vector=vectors[key(row)]
        return np.r_[vector/(np.linalg.norm(vector)+1e-12),1.0]

    grouped=collections.defaultdict(list)
    for row in train:
        grouped[row["surface"]].append(row)
    models={}
    for surface,examples in sorted(grouped.items()):
        labels=sorted({r["reading"] for r in examples})
        if len(labels)<2:
            continue
        support={label:{"examples":sum(r["reading"]==label for r in examples),
            "works":len({r["work_id"] for r in examples if r["reading"]==label})} for label in labels}
        x=np.array([feature(r) for r in examples])
        y=np.eye(len(labels))[[labels.index(r["reading"]) for r in examples]]
        weights=np.array([1/support[r["reading"]]["examples"] for r in examples])**.5
        xw=x*weights[:,None]; yw=y*weights[:,None]
        w=xw.T @ np.linalg.solve(xw@xw.T+np.eye(len(examples)),yw)
        models[surface]=(labels,w,support)

    def predict(row):
        if row["surface"] not in models:
            return None, "no_multi_reading_classifier"
        labels,w,support=models[row["surface"]]
        candidates=[c["reading"] for c in row["candidates"]]
        scores=feature(row)@w
        return select_reading(candidates, labels, support, scores)

    candidate_map={r["surface"]:r["candidates"] for r in fixture}
    validation_rows=[]
    for row in validation:
        examples=grouped.get(row["surface"],[])
        if not examples: continue
        counts=collections.Counter(r["reading"] for r in examples)
        options=candidate_map.get(row["surface"],[{"reading":r} for r in sorted({b["reading"] for b in bank if b["surface"]==row["surface"]})])
        validation_rows.append({**row,"candidates":options,"accepted":[row["reading"]],
            "baseline":sorted(counts,key=lambda r:(-counts[r],r))[0],"eligible":True,"group":row["work_id"]})
    results=[]
    for label,items in [("Aozora held-out works vs training majority",validation_rows),
                        ("displayed heteronym fixture",fixture),("source-ruby captures",captures)]:
        decisions={r["id"]:predict(r) for r in items if r["eligible"]}
        result = observe(items,{k:v[0] for k,v in decisions.items()},label,np)
        result["decision_reasons"] = dict(collections.Counter(v[1] for v in decisions.values()))
        # "Rare" here means below the spelling's most frequent training label; it is a bank
        # statistic, not a claim about general Japanese usage. Never tune from these groups.
        frequency_groups = collections.defaultdict(lambda: {"cases":0,"decisions":0,"fixed":0,"broken":0})
        for row in items:
            if row.get("baseline") is None: continue
            counts = collections.Counter(r["reading"] for r in grouped.get(row["surface"], []))
            best = max(counts.values(), default=0)
            gold_support = max((counts.get(r, 0) for r in row["accepted"]), default=0)
            group = "unseen" if not gold_support else "majority_or_tied" if gold_support == best else "minority"
            stats = frequency_groups[group]
            stats["cases"] += 1
            prediction = decisions.get(row["id"], (None,))[0]
            if prediction is not None:
                before, after = row["baseline"] in row["accepted"], prediction in row["accepted"]
                stats["decisions"] += 1
                stats["fixed"] += int(after and not before)
                stats["broken"] += int(before and not after)
        result["training_frequency_groups"] = dict(frequency_groups)
        print(json.dumps({"label":label,"decision_reasons":result["decision_reasons"]}),flush=True)
        results.append(result)
    report={"model":REVISION,"training_examples":len(train),"validation_examples":len(validation),
        "classifiers":len(models),"encoder_file_bytes":(encoder/'model.safetensors').stat().st_size,
        "process_peak_working_set_bytes":getattr(psutil.Process().memory_info(),"peak_wset",None),
        "timing_note":"amortized batch-16 CPU forward time, not interactive latency; null on cache replay",
        "new_embedding_seconds_per_target_median":float(np.median(elapsed)) if elapsed else None,
        "new_embedding_seconds_per_target_p95":float(np.quantile(elapsed,.95)) if elapsed else None,
        "support":{s:m[2] for s,m in models.items()},"results":results,
        "integration_gate":"NOT PASSED: capture domain coverage and independent review are required regardless of numerical results"}
    (WORK/'target-reading-results.json').write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(report['integration_gate'],flush=True)

if __name__ == '__main__':
    main()
