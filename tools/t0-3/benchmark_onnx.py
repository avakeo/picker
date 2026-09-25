#!/usr/bin/env python3
"""Measure ONNX load/inference without creating or retaining rankings."""

from __future__ import annotations

import argparse
import csv
import json
import os
import resource
import subprocess
import sys
import time
from pathlib import Path
from statistics import median


def percentile(values: list[float], probability: float) -> float:
    ordered = sorted(values)
    position = (len(ordered) - 1) * probability
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    fraction = position - lower
    return ordered[lower] * (1 - fraction) + ordered[upper] * fraction


def provider_spec(name: str):
    if name == "cpu":
        return ["CPUExecutionProvider"]
    return [
        ("CoreMLExecutionProvider", {"ModelFormat": "NeuralNetwork", "MLComputeUnits": "ALL"}),
        "CPUExecutionProvider",
    ]


def worker(args: argparse.Namespace) -> int:
    # Remote-code image processors need a writable module cache even when the
    # model source itself is local. Keep it with the gitignored evaluation data.
    os.environ.setdefault("HF_MODULES_CACHE", str(args.hf_modules_cache.resolve()))
    import numpy as np
    import onnxruntime as ort
    from PIL import Image
    from pillow_heif import register_heif_opener
    from transformers import AutoImageProcessor

    register_heif_opener()
    processor = AutoImageProcessor.from_pretrained(args.source, trust_remote_code=True, use_fast=False)
    with args.images_csv.open(encoding="utf-8", newline="") as handle:
        rows = list(csv.DictReader(handle))
    arrays = []
    for row in rows:
        with Image.open(args.image_root / row["path"]) as image:
            arrays.append(processor(images=image.convert("RGB"), return_tensors="np")["pixel_values"])

    started = time.perf_counter_ns()
    session = ort.InferenceSession(str(args.image_model), providers=provider_spec(args.provider))
    load_ms = (time.perf_counter_ns() - started) / 1_000_000
    for _ in range(args.warmup):
        session.run(None, {args.image_input_name: arrays[0]})
    inference_ms = []
    for array in arrays:
        started = time.perf_counter_ns()
        output = session.run(None, {args.image_input_name: array})[0]
        inference_ms.append((time.perf_counter_ns() - started) / 1_000_000)
        del output
    print(json.dumps({
        "load_ms": load_ms,
        "inference_ms": inference_ms,
        "image_count": len(arrays),
        "active_providers": session.get_providers(),
        "peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        "ort_version": ort.__version__,
    }, sort_keys=True))
    return 0


def coordinator(args: argparse.Namespace) -> int:
    runs = []
    for _ in range(args.load_repetitions):
        command = [
            sys.executable, str(Path(__file__).resolve()), "--worker",
            "--source", str(args.source), "--image-model", str(args.image_model),
            "--images-csv", str(args.images_csv), "--image-root", str(args.image_root),
            "--provider", args.provider, "--warmup", str(args.warmup),
            "--image-input-name", args.image_input_name,
            "--hf-modules-cache", str(args.hf_modules_cache),
        ]
        completed = subprocess.run(command, check=True, capture_output=True, text=True, env=os.environ)
        runs.append(json.loads(completed.stdout.strip().splitlines()[-1]))
    inference = [value for run in runs for value in run["inference_ms"]]
    model_files = [path for path in args.model_files if path.is_file()]
    result = {
        "candidate_id": args.candidate_id,
        "precision": args.precision,
        "provider": args.provider,
        "image_count": runs[0]["image_count"],
        "batch_size": 1,
        "warmup_iterations": args.warmup,
        "load_repetitions": args.load_repetitions,
        "load_raw_ms": [run["load_ms"] for run in runs],
        "load_p50_ms": median(run["load_ms"] for run in runs),
        "load_p95_ms": percentile([run["load_ms"] for run in runs], 0.95),
        "inference_raw_ms": inference,
        "inference_p50_ms": median(inference),
        "inference_p95_ms": percentile(inference, 0.95),
        "peak_rss_bytes": max(run["peak_rss_bytes"] for run in runs),
        "model_total_bytes": sum(path.stat().st_size for path in model_files),
        "model_files": [str(path) for path in model_files],
        "active_providers": runs[0]["active_providers"],
        "ort_version": runs[0]["ort_version"],
        "rankings_generated": False,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--worker", action="store_true")
    parser.add_argument("--candidate-id")
    parser.add_argument("--precision")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--image-model", type=Path, required=True)
    parser.add_argument("--images-csv", type=Path, required=True)
    parser.add_argument("--image-root", type=Path, required=True)
    parser.add_argument("--provider", choices=("cpu", "coreml"), required=True)
    parser.add_argument("--image-input-name", default="pixel_values")
    parser.add_argument("--hf-modules-cache", type=Path, default=Path("eval-data/hf-modules-cache"))
    parser.add_argument("--warmup", type=int, default=5)
    parser.add_argument("--load-repetitions", type=int, default=5)
    parser.add_argument("--model-files", type=Path, nargs="*", default=[])
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


if __name__ == "__main__":
    parsed = parse_args()
    raise SystemExit(worker(parsed) if parsed.worker else coordinator(parsed))
