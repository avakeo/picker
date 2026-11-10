import random
import unittest

from analyze_blind_scores import average_precision, bootstrap_difference, ndcg, recall_at_k


class AnalyzeBlindScoresTest(unittest.TestCase):
    def test_metrics_treat_missing_as_zero(self) -> None:
        labels = {"best": 2.0, "relevant": 1.0}
        pool = {"best", "relevant", "missing"}
        ranked = ["best", "missing", "relevant"]
        self.assertAlmostEqual(ndcg(ranked, labels, pool, 3), 0.9639404333)
        self.assertAlmostEqual(average_precision(ranked, labels, pool), 5 / 6)
        self.assertEqual(recall_at_k(ranked, labels, pool, 2), 0.5)

    def test_bootstrap_reports_aligned_difference(self) -> None:
        point, low, high = bootstrap_difference(
            [1.0, 1.0, 1.0],
            [0.5, 0.5, 0.5],
            random.Random(42),
            100,
        )
        self.assertEqual((point, low, high), (0.5, 0.5, 0.5))


if __name__ == "__main__":
    unittest.main()
