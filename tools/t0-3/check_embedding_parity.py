#!/usr/bin/env python3
"""Compare fixed-sample Python embeddings with split ONNX sessions."""

from __future__ import annotations

import argparse
import csv
import json
import os
import resource
from pathlib import Path

import numpy as np
import onnxruntime as ort
import torch
from PIL import Image
from pillow_heif import register_heif_opener
from transformers import AutoModel, AutoProcessor


FIXED_TEXTS = ["犬の写真", "夕暮れの海辺", "赤い車"]


def cosine_rows(reference: np.ndarray, actual: np.ndarray) -> list[float]:
    return [
        float(np.dot(left, right) / (np.linalg.norm(left) * np.linalg.norm(right)))
        for left, right in zip(reference, actual, strict=True)
    ]


def providers(name: str):
    if name == "cpu":
        return ["CPUExecutionProvider"]
    return [
        ("CoreMLExecutionProvider", {"ModelFormat": "NeuralNetwork", "MLComputeUnits": "ALL"}),
        "CPUExecutionProvider",
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--candidate-id", required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--onnx-dir", type=Path, required=True)
    parser.add_argument("--images-csv", type=Path, required=True)
    parser.add_argument("--image-root", type=Path, required=True)
    parser.add_argument("--provider", choices=("cpu", "coreml"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    register_heif_opener()
    with args.images_csv.open(encoding="utf-8", newline="") as handle:
        rows = list(csv.DictReader(handle))
    if len(rows) < 2:
        raise ValueError("at least two fixed images are required")
    image_rows = [rows[0], next((row for row in rows if row["path"].lower().endswith(".heic")), rows[1])]
    images = [Image.open(args.image_root / row["path"]).convert("RGB") for row in image_rows]

    processor = AutoProcessor.from_pretrained(args.source, use_fast=False)
    model = AutoModel.from_pretrained(args.source).eval()
    inputs = processor(text=FIXED_TEXTS, images=images, padding="max_length", return_tensors="pt")
    with torch.inference_mode():
        reference = model(**inputs)

    threshold = 0.999
    result: dict[str, object] = {
        "candidate_id": args.candidate_id,
        "provider": args.provider,
        "threshold": threshold,
        "fixed_image_ids": [row["image_id"] for row in image_rows],
        "fixed_texts": FIXED_TEXTS,
        "python_token_ids": inputs["input_ids"].tolist(),
        "peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
        "ort_version": ort.__version__,
    }
    try:
        image_session = ort.InferenceSession(str(args.onnx_dir / "image.onnx"), providers=providers(args.provider))
        text_session = ort.InferenceSession(str(args.onnx_dir / "text.onnx"), providers=providers(args.provider))
        image_actual = image_session.run(None, {"pixel_values": inputs["pixel_values"].numpy()})[0]
        text_actual = text_session.run(None, {"input_ids": inputs["input_ids"].numpy()})[0]
        image_cosines = cosine_rows(reference.image_embeds.numpy(), image_actual)
        text_cosines = cosine_rows(reference.text_embeds.numpy(), text_actual)
        result.update({
            "image_cosines": image_cosines,
            "text_cosines": text_cosines,
            "minimum_cosine": min(image_cosines + text_cosines),
            "passed": min(image_cosines + text_cosines) >= threshold,
            "active_image_providers": image_session.get_providers(),
            "active_text_providers": text_session.get_providers(),
        })
    except Exception as error:
        result.update({"passed": False, "error": str(error)})
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0 if result["passed"] else 2


if __name__ == "__main__":
    os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
    raise SystemExit(main())
