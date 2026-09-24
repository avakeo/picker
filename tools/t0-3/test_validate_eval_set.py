import json
import tempfile
import unittest
from pathlib import Path

from validate_eval_set import ValidationError, validate


class ValidateEvalSetTest(unittest.TestCase):
    def write_fixture(self, root: Path) -> Path:
        image_root = root / "images"
        image_root.mkdir()
        for index in range(5):
            (image_root / f"image-{index}.jpg").write_bytes(b"fixture")
        (root / "images.csv").write_text(
            "image_id,path,notes\n"
            + "\n".join(f"img-{i},image-{i}.jpg," for i in range(5))
            + "\n",
            encoding="utf-8",
        )
        (root / "queries.csv").write_text("query_id,query_ja,query_en,reviewed_all_images,notes\nq-1,海辺,beach,true,\n", encoding="utf-8")
        (root / "relevance.csv").write_text("query_id,image_id,relevance,notes\nq-1,img-0,2,\n", encoding="utf-8")
        (root / "tags.csv").write_text("tag_id,tag_ja,tag_en,category,reviewed_all_images,notes\ntag-1,海,sea,scene,true,\n", encoding="utf-8")
        custom_rows = [
            "tag-1,img-0,1,train,",
            "tag-1,img-1,1,train,",
            "tag-1,img-2,1,train,",
            "tag-1,img-3,1,test,",
            "tag-1,img-4,0,test,",
        ]
        (root / "custom-labels.csv").write_text(
            "tag_id,image_id,label,split,notes\n" + "\n".join(custom_rows) + "\n",
            encoding="utf-8",
        )
        manifest = {
            "schema_version": "0.1",
            "image_root": str(image_root),
            "images": "images.csv",
            "queries": "queries.csv",
            "relevance": "relevance.csv",
            "tags": "tags.csv",
            "custom_labels": "custom-labels.csv",
            "unlisted_label": 0,
            "random_seed": 42,
        }
        manifest_path = root / "manifest.json"
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        return manifest_path

    def test_valid_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            summary = validate(self.write_fixture(Path(directory)))
        self.assertEqual(summary["images"], 5)
        self.assertEqual(summary["custom_label_rows"], 5)

    def test_rejects_train_test_overlap(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = self.write_fixture(root)
            labels = root / "custom-labels.csv"
            labels.write_text(labels.read_text(encoding="utf-8").replace("tag-1,img-4,0,test,", "tag-1,img-0,0,test,"), encoding="utf-8")
            with self.assertRaises(ValidationError):
                validate(manifest)


if __name__ == "__main__":
    unittest.main()
