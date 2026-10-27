import csv
import tempfile
import unittest
from pathlib import Path

from build_blind_scoring import build
from test_validate_eval_set import ValidateEvalSetTest


class BuildBlindScoringTest(unittest.TestCase):
    def test_builds_anonymous_offline_page_with_duplicates(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ValidateEvalSetTest().write_fixture(root)
            with (root / "candidate-map.csv").open("w", encoding="utf-8", newline="") as handle:
                writer = csv.writer(handle)
                writer.writerow(["run_id", "candidate_id"])
                writer.writerow(["R001", "SecretModelAlpha"])
                writer.writerow(["R002", "SecretModelBeta"])
            with (root / "pool-rankings.csv").open("w", encoding="utf-8", newline="") as handle:
                writer = csv.writer(handle)
                writer.writerow(["task", "prompt_id", "run_id", "rank", "image_id"])
                for index in range(6):
                    writer.writerow(["natural_language", "q-0", "R001", index + 1, f"img-{index}"])
                    writer.writerow(["natural_language", "q-0", "R002", index + 1, f"img-{index + 6}"])

            html_path, map_path, count = build(root)
            page = html_path.read_text(encoding="utf-8")
            with map_path.open(encoding="utf-8", newline="") as handle:
                map_rows = list(csv.DictReader(handle))

        self.assertEqual(count, 13)
        self.assertEqual(len(map_rows), 13)
        self.assertNotIn("SecretModelAlpha", page)
        self.assertNotIn("SecretModelBeta", page)
        self.assertNotIn("R001", page)
        self.assertNotIn("R002", page)
        self.assertNotIn("http://", page)
        self.assertNotIn("https://", page)
        self.assertIn("blind-scores.csv", page)
        self.assertIn("localStorage", page)
        groups = [row["duplicate_group"] for row in map_rows]
        self.assertEqual(len(groups) - len(set(groups)), 1)


if __name__ == "__main__":
    unittest.main()
