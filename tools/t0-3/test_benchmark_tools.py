import csv
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import score_results
from benchmark_onnx import model_files_or_error


class ModelFilesTest(unittest.TestCase):
    def test_empty_list_is_an_error(self) -> None:
        with self.assertRaises(SystemExit):
            model_files_or_error([])

    def test_missing_file_is_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            present = Path(directory) / "model.onnx"
            present.write_bytes(b"x")
            with self.assertRaises(SystemExit) as raised:
                model_files_or_error([present, Path(directory) / "missing.onnx"])
            self.assertIn("missing.onnx", str(raised.exception))
            self.assertEqual(model_files_or_error([present]), [present])


class ScoreResultsTest(unittest.TestCase):
    def test_scores_keep_provider_and_precision(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "raw.csv"
            with raw.open("w", newline="") as handle:
                writer = csv.DictWriter(
                    handle,
                    fieldnames=[
                        "candidate_id",
                        "precision",
                        "provider",
                        "eligible",
                        "speed",
                    ],
                )
                writer.writeheader()
                writer.writerow(
                    {
                        "candidate_id": "a",
                        "precision": "fp16",
                        "provider": "cpu",
                        "eligible": "true",
                        "speed": "10",
                    }
                )
                writer.writerow(
                    {
                        "candidate_id": "a",
                        "precision": "fp16",
                        "provider": "coreml",
                        "eligible": "true",
                        "speed": "5",
                    }
                )
            config = root / "config.json"
            config.write_text(
                json.dumps(
                    {
                        "hard_limits": {},
                        "zero_range_score": 100,
                        "metrics": [
                            {"column": "speed", "direction": "lower", "weight": 1}
                        ],
                    }
                )
            )
            output = root / "scores.csv"
            argv = [
                "score_results.py",
                "--raw",
                str(raw),
                "--config",
                str(config),
                "--output",
                str(output),
            ]
            with mock.patch.object(sys, "argv", argv):
                self.assertEqual(score_results.main(), 0)
            with output.open(newline="") as handle:
                rows = list(csv.DictReader(handle))
            self.assertEqual(
                [(row["provider"], row["precision"]) for row in rows],
                [("coreml", "fp16"), ("cpu", "fp16")],
            )


if __name__ == "__main__":
    unittest.main()
