#!/usr/bin/env python3
"""Validate the local, user-owned T0-3 evaluation set without copying images."""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path
from typing import Iterable


class ValidationError(Exception):
    pass


def read_csv(path: Path, required: Iterable[str]) -> list[dict[str, str]]:
    if not path.is_file():
        raise ValidationError(f"missing CSV: {path}")
    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        fields = set(reader.fieldnames or [])
        missing = sorted(set(required) - fields)
        if missing:
            raise ValidationError(f"{path}: missing columns: {', '.join(missing)}")
        rows = list(reader)
    return rows


def unique_ids(rows: list[dict[str, str]], column: str, name: str) -> set[str]:
    values = [row.get(column, "").strip() for row in rows]
    if any(not value for value in values):
        raise ValidationError(f"{name}: {column} must not be empty")
    if len(values) != len(set(values)):
        raise ValidationError(f"{name}: duplicate {column}")
    return set(values)


def require_true(rows: list[dict[str, str]], column: str, name: str) -> None:
    invalid = [row.get("query_id", row.get("tag_id", "")) for row in rows if row.get(column, "").lower() != "true"]
    if invalid:
        raise ValidationError(f"{name}: {column}=true is required for: {', '.join(invalid)}")


def validate(manifest_path: Path) -> dict[str, int]:
    with manifest_path.open("r", encoding="utf-8") as handle:
        manifest = json.load(handle)

    required_manifest = {"schema_version", "image_root", "images", "queries", "relevance", "tags", "custom_labels"}
    missing = sorted(required_manifest - set(manifest))
    if missing:
        raise ValidationError(f"manifest: missing keys: {', '.join(missing)}")

    image_root = Path(manifest["image_root"]).expanduser().resolve()
    if not image_root.is_dir():
        raise ValidationError(f"image_root is not a directory: {image_root}")

    def relative_file(key: str) -> Path:
        path = (manifest_path.parent / manifest[key]).resolve()
        if path.parent != manifest_path.parent.resolve():
            raise ValidationError(f"manifest {key} must be beside manifest.json")
        return path

    images = read_csv(relative_file("images"), {"image_id", "path", "notes"})
    queries = read_csv(relative_file("queries"), {"query_id", "query_ja", "query_en", "reviewed_all_images", "notes"})
    relevance = read_csv(relative_file("relevance"), {"query_id", "image_id", "relevance", "notes"})
    tags = read_csv(relative_file("tags"), {"tag_id", "tag_ja", "tag_en", "category", "reviewed_all_images", "notes"})
    custom = read_csv(relative_file("custom_labels"), {"tag_id", "image_id", "label", "split", "notes"})

    image_ids = unique_ids(images, "image_id", "images.csv")
    query_ids = unique_ids(queries, "query_id", "queries.csv")
    tag_ids = unique_ids(tags, "tag_id", "tags.csv")
    require_true(queries, "reviewed_all_images", "queries.csv")
    require_true(tags, "reviewed_all_images", "tags.csv")

    for row in images:
        path = (image_root / row["path"]).resolve()
        if image_root not in path.parents:
            raise ValidationError(f"image escapes image_root: {row['path']}")
        if not path.is_file():
            raise ValidationError(f"missing image: {path}")

    for row in relevance:
        if row["query_id"] not in query_ids or row["image_id"] not in image_ids:
            raise ValidationError("relevance.csv references an unknown query_id or image_id")
        if row["relevance"] not in {"0", "1", "2"}:
            raise ValidationError("relevance.csv relevance must be 0, 1, or 2")

    seen_pairs: set[tuple[str, str]] = set()
    splits: dict[str, dict[str, set[str]]] = {}
    for row in custom:
        pair = (row["tag_id"], row["image_id"])
        if pair in seen_pairs:
            raise ValidationError("custom-labels.csv contains duplicate tag_id/image_id")
        seen_pairs.add(pair)
        if row["tag_id"] not in tag_ids or row["image_id"] not in image_ids:
            raise ValidationError("custom-labels.csv references an unknown tag_id or image_id")
        if row["label"] not in {"0", "1"} or row["split"] not in {"train", "test"}:
            raise ValidationError("custom-labels.csv label must be 0/1 and split must be train/test")
        splits.setdefault(row["tag_id"], {"train": set(), "test": set()})[row["split"]].add(row["image_id"])

    for tag_id, by_split in splits.items():
        if by_split["train"] & by_split["test"]:
            raise ValidationError(f"custom tag {tag_id}: train/test image overlap")
        train_positive = sum(row["tag_id"] == tag_id and row["split"] == "train" and row["label"] == "1" for row in custom)
        test_positive = sum(row["tag_id"] == tag_id and row["split"] == "test" and row["label"] == "1" for row in custom)
        test_negative = sum(row["tag_id"] == tag_id and row["split"] == "test" and row["label"] == "0" for row in custom)
        if train_positive < 3 or test_positive < 1 or test_negative < 1:
            raise ValidationError(f"custom tag {tag_id}: need 3 train positives and positive/negative test rows")

    return {"images": len(images), "queries": len(queries), "relevance_rows": len(relevance), "tags": len(tags), "custom_label_rows": len(custom)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()
    try:
        summary = validate(args.manifest.expanduser().resolve())
    except (OSError, json.JSONDecodeError, ValidationError) as error:
        print(f"validation failed: {error}")
        return 1
    print(json.dumps({"status": "ok", **summary}, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
