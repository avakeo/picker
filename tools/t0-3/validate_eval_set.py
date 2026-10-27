#!/usr/bin/env python3
"""Validate the local T0-3 blind-evaluation inputs without copying images."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
from pathlib import Path
from typing import Iterable


MIN_QUERIES = 20
MIN_TAGS = 20
MAX_TAGS = 30
MIN_SIMILARITY_QUERIES = 10
MIN_CUSTOM_TAGS = 3
MAX_CUSTOM_TAGS = 5
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")


class ValidationError(Exception):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_csv(path: Path, required: Iterable[str]) -> list[dict[str, str]]:
    if not path.is_file():
        raise ValidationError(f"missing CSV: {path}")
    with path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        fields = set(reader.fieldnames or [])
        missing = sorted(set(required) - fields)
        if missing:
            raise ValidationError(f"{path}: missing columns: {', '.join(missing)}")
        return list(reader)


def unique_ids(rows: list[dict[str, str]], column: str, name: str) -> set[str]:
    values = [row.get(column, "").strip() for row in rows]
    if any(not value for value in values):
        raise ValidationError(f"{name}: {column} must not be empty")
    if len(values) != len(set(values)):
        raise ValidationError(f"{name}: duplicate {column}")
    return set(values)


def validate(manifest_path: Path) -> dict[str, int | str]:
    with manifest_path.open("r", encoding="utf-8") as handle:
        manifest = json.load(handle)

    required_manifest = {
        "schema_version",
        "image_root",
        "images",
        "queries",
        "tags",
        "similarity_queries",
        "custom_labels",
        "random_seed",
    }
    missing = sorted(required_manifest - set(manifest))
    if missing:
        raise ValidationError(f"manifest: missing keys: {', '.join(missing)}")

    image_root = Path(manifest["image_root"]).expanduser().resolve()
    if not image_root.is_dir():
        raise ValidationError(f"image_root is not a directory: {image_root}")

    def local_file(key: str) -> Path:
        path = (manifest_path.parent / manifest[key]).resolve()
        if path.parent != manifest_path.parent.resolve():
            raise ValidationError(f"manifest {key} must be beside manifest.json")
        return path

    images_path = local_file("images")
    queries_path = local_file("queries")
    tags_path = local_file("tags")
    similarity_path = local_file("similarity_queries")
    custom_path = local_file("custom_labels")

    images = read_csv(images_path, {"image_id", "path", "notes"})
    queries = read_csv(queries_path, {"query_id", "query_ja", "query_en", "notes"})
    tags = read_csv(tags_path, {"tag_id", "tag_ja", "tag_en", "category", "notes"})
    similarity = read_csv(similarity_path, {"query_image_id", "image_sha256", "notes"})
    custom = read_csv(custom_path, {"tag_id", "image_id", "label", "split", "notes"})

    image_ids = unique_ids(images, "image_id", "images.csv")
    unique_ids(queries, "query_id", "queries.csv")
    tag_ids = unique_ids(tags, "tag_id", "tags.csv")
    similarity_ids = unique_ids(similarity, "query_image_id", "similarity-queries.csv")

    if len(queries) < MIN_QUERIES:
        raise ValidationError(f"queries.csv: need at least {MIN_QUERIES} queries")
    if not MIN_TAGS <= len(tags) <= MAX_TAGS:
        raise ValidationError(f"tags.csv: need {MIN_TAGS} to {MAX_TAGS} tags")
    if len(similarity) < MIN_SIMILARITY_QUERIES:
        raise ValidationError(f"similarity-queries.csv: need at least {MIN_SIMILARITY_QUERIES} images")
    if not similarity_ids <= image_ids:
        raise ValidationError("similarity-queries.csv references an unknown image_id")

    image_paths: dict[str, Path] = {}
    for row in images:
        path = (image_root / row["path"]).resolve()
        if image_root not in path.parents:
            raise ValidationError(f"image escapes image_root: {row['path']}")
        if not path.is_file():
            raise ValidationError(f"missing image: {path}")
        image_paths[row["image_id"]] = path

    for row in similarity:
        expected = row["image_sha256"].strip().lower()
        if not SHA256_PATTERN.fullmatch(expected):
            raise ValidationError(f"similarity query {row['query_image_id']}: invalid SHA-256")
        actual = sha256_file(image_paths[row["query_image_id"]])
        if actual != expected:
            raise ValidationError(f"similarity query {row['query_image_id']}: SHA-256 mismatch")

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

    if not MIN_CUSTOM_TAGS <= len(splits) <= MAX_CUSTOM_TAGS:
        raise ValidationError(f"custom-labels.csv: need {MIN_CUSTOM_TAGS} to {MAX_CUSTOM_TAGS} tags")
    for tag_id, by_split in splits.items():
        if by_split["train"] & by_split["test"]:
            raise ValidationError(f"custom tag {tag_id}: train/test image overlap")
        train_positive = sum(row["tag_id"] == tag_id and row["split"] == "train" and row["label"] == "1" for row in custom)
        test_positive = sum(row["tag_id"] == tag_id and row["split"] == "test" and row["label"] == "1" for row in custom)
        test_negative = sum(row["tag_id"] == tag_id and row["split"] == "test" and row["label"] == "0" for row in custom)
        if train_positive < 3 or test_positive < 1 or test_negative < 1:
            raise ValidationError(f"custom tag {tag_id}: need 3 train positives and positive/negative test rows")

    definition_digest = hashlib.sha256()
    hashes: dict[str, str] = {}
    for name, path in (("queries", queries_path), ("tags", tags_path), ("similarity_queries", similarity_path)):
        value = sha256_file(path)
        hashes[f"{name}_sha256"] = value
        definition_digest.update(bytes.fromhex(value))

    return {
        "images": len(images),
        "queries": len(queries),
        "tags": len(tags),
        "similarity_queries": len(similarity),
        "custom_tags": len(splits),
        "custom_label_rows": len(custom),
        "definition_sha256": definition_digest.hexdigest(),
        **hashes,
    }


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
