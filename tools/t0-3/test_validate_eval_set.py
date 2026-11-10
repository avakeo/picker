import csv
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from validate_eval_set import ValidationError, validate


class ValidateEvalSetTest(unittest.TestCase):
    def write_fixture(
        self,
        root: Path,
        *,
        query_count: int = 20,
        tag_count: int = 20,
        similarity_count: int = 10,
        custom_tag_count: int = 3,
    ) -> Path:
        image_root = root / "images"
        image_root.mkdir()
        for index in range(12):
            (image_root / f"image-{index}.jpg").write_bytes(f"fixture-{index}".encode())
        (root / "images.csv").write_text(
            "image_id,path,notes\n" + "\n".join(f"img-{i},image-{i}.jpg," for i in range(12)) + "\n",
            encoding="utf-8",
        )
        with (root / "queries.csv").open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["query_id", "query_ja", "query_en", "notes"])
            for index in range(query_count):
                writer.writerow([f"q-{index}", f"検索 {index}", f"query {index}", ""])
        with (root / "tags.csv").open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["tag_id", "tag_ja", "tag_en", "category", "notes"])
            for index in range(tag_count):
                writer.writerow([f"tag-{index}", f"タグ {index}", f"tag {index}", "object", ""])
        with (root / "similarity-queries.csv").open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["query_image_id", "image_sha256", "notes"])
            for index in range(similarity_count):
                digest = hashlib.sha256((image_root / f"image-{index}.jpg").read_bytes()).hexdigest()
                writer.writerow([f"img-{index}", digest, ""])
        custom_rows: list[list[str]] = []
        for tag_index in range(custom_tag_count):
            custom_rows.extend(
                [
                    [f"tag-{tag_index}", "img-0", "1", "train", ""],
                    [f"tag-{tag_index}", "img-1", "1", "train", ""],
                    [f"tag-{tag_index}", "img-2", "1", "train", ""],
                    [f"tag-{tag_index}", "img-3", "1", "test", ""],
                    [f"tag-{tag_index}", "img-4", "0", "test", ""],
                ]
            )
        with (root / "custom-labels.csv").open("w", encoding="utf-8", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["tag_id", "image_id", "label", "split", "notes"])
            writer.writerows(custom_rows)
        manifest = {
            "schema_version": "0.2",
            "image_root": str(image_root),
            "images": "images.csv",
            "queries": "queries.csv",
            "tags": "tags.csv",
            "similarity_queries": "similarity-queries.csv",
            "custom_labels": "custom-labels.csv",
            "random_seed": 42,
        }
        manifest_path = root / "manifest.json"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        return manifest_path

    def test_valid_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            summary = validate(self.write_fixture(Path(directory)))
        self.assertEqual(summary["queries"], 20)
        self.assertEqual(summary["tags"], 20)
        self.assertEqual(summary["similarity_queries"], 10)
        self.assertEqual(summary["custom_tags"], 3)
        self.assertEqual(len(str(summary["definition_sha256"])), 64)

    def test_rejects_minimum_counts(self) -> None:
        cases = [
            {"query_count": 19},
            {"tag_count": 19},
            {"similarity_count": 9},
            {"custom_tag_count": 2},
        ]
        for options in cases:
            with self.subTest(options=options), tempfile.TemporaryDirectory() as directory:
                with self.assertRaises(ValidationError):
                    validate(self.write_fixture(Path(directory), **options))

    def test_rejects_similarity_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = self.write_fixture(root)
            path = root / "similarity-queries.csv"
            rows = path.read_text(encoding="utf-8").splitlines()
            columns = rows[1].split(",")
            columns[1] = "0" * 64
            rows[1] = ",".join(columns)
            path.write_text("\n".join(rows) + "\n", encoding="utf-8")
            with self.assertRaises(ValidationError):
                validate(manifest)


if __name__ == "__main__":
    unittest.main()
