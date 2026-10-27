#!/usr/bin/env python3
"""Split a joint image/text ONNX model and optionally convert both halves to f16."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import onnx
from onnx.utils import extract_model
from onnxruntime.transformers.float16 import convert_float_to_float16


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--precision", choices=("fp32", "fp16"), required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)

    written = []
    for name, inputs, outputs in (
        ("image", ["pixel_values"], ["image_embeds"]),
        ("text", ["input_ids"], ["text_embeds"]),
    ):
        target = args.output / f"{name}.onnx"
        if args.precision == "fp32":
            extract_model(str(args.input), str(target), inputs, outputs)
        else:
            temporary = args.output / f".{name}.fp32.onnx"
            extract_model(str(args.input), str(temporary), inputs, outputs)
            converted = convert_float_to_float16(onnx.load(temporary), keep_io_types=True)
            onnx.save(converted, target)
            temporary.unlink()
        written.append({"path": str(target), "bytes": target.stat().st_size})

    print(json.dumps({"precision": args.precision, "files": written}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
