#!/usr/bin/env python3
"""Compute pooled retrieval metrics, self-consistency, and bootstrap CIs."""

from __future__ import annotations

import argparse
import csv
import json
import math
import random
from collections import defaultdict
from pathlib import Path
from statistics import mean

from validate_eval_set import ValidationError, read_csv


def dcg(values: list[float], k: int = 10) -> float:
    return sum((2**value - 1) / math.log2(index + 2) for index, value in enumerate(values[:k]))


def ndcg(ranked_ids: list[str], labels: dict[str, float], pool_ids: set[str], k: int = 10) -> float:
    actual = dcg([labels.get(item_id, 0.0) for item_id in ranked_ids], k)
    ideal = dcg(sorted((labels.get(item_id, 0.0) for item_id in pool_ids), reverse=True), k)
    return 0.0 if ideal == 0 else actual / ideal


def average_precision(ranked_ids: list[str], labels: dict[str, float], pool_ids: set[str]) -> float:
    relevant_total = sum(labels.get(item_id, 0.0) >= 1.0 for item_id in pool_ids)
    if relevant_total == 0:
        return 0.0
    hits = 0
    precision_sum = 0.0
    for rank, item_id in enumerate(ranked_ids, start=1):
        if labels.get(item_id, 0.0) >= 1.0:
            hits += 1
            precision_sum += hits / rank
    return precision_sum / relevant_total


def recall_at_k(ranked_ids: list[str], labels: dict[str, float], pool_ids: set[str], k: int = 10) -> float:
    relevant = {item_id for item_id in pool_ids if labels.get(item_id, 0.0) >= 1.0}
    return 0.0 if not relevant else len(relevant & set(ranked_ids[:k])) / len(relevant)


def percentile(values: list[float], probability: float) -> float:
    ordered = sorted(values)
    position = (len(ordered) - 1) * probability
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] * (upper - position) + ordered[upper] * (position - lower)


def bootstrap_difference(left: list[float], right: list[float], rng: random.Random, iterations: int) -> tuple[float, float, float]:
    if len(left) != len(right) or not left:
        raise ValidationError("bootstrap inputs must be non-empty and aligned")
    differences = []
    for _ in range(iterations):
        indices = [rng.randrange(len(left)) for _ in left]
        differences.append(mean(left[index] - right[index] for index in indices))
    point = mean(a - b for a, b in zip(left, right, strict=True))
    return point, percentile(differences, 0.025), percentile(differences, 0.975)


def analyze(eval_root: Path, iterations: int = 10_000) -> tuple[Path, Path, Path]:
    eval_root = eval_root.resolve()
    with (eval_root / "manifest.json").open("r", encoding="utf-8") as handle:
        manifest = json.load(handle)
    blind_map = read_csv(eval_root / "blind-map.csv", {"blind_id", "task", "prompt_id", "image_id", "duplicate_group"})
    score_rows = read_csv(eval_root / "blind-scores.csv", {"blind_id", "score"})
    rankings = read_csv(eval_root / "pool-rankings.csv", {"task", "prompt_id", "run_id", "rank", "image_id"})
    candidate_map = read_csv(eval_root / "candidate-map.csv", {"run_id", "candidate_id"})

    score_by_blind = {row["blind_id"]: int(row["score"]) if row["score"].strip() else 0 for row in score_rows}
    by_group: dict[str, list[int]] = defaultdict(list)
    label_samples: dict[tuple[str, str, str], list[int]] = defaultdict(list)
    for row in blind_map:
        score = score_by_blind.get(row["blind_id"], 0)
        by_group[row["duplicate_group"]].append(score)
        label_samples[(row["task"], row["prompt_id"], row["image_id"])].append(score)

    duplicate_groups = [values for values in by_group.values() if len(values) > 1]
    exact_matches = sum(len(set(values)) == 1 for values in duplicate_groups)
    consistency = 0.0 if not duplicate_groups else exact_matches / len(duplicate_groups)
    labels_by_prompt: dict[tuple[str, str], dict[str, float]] = defaultdict(dict)
    for (task, prompt_id, image_id), values in label_samples.items():
        labels_by_prompt[(task, prompt_id)][image_id] = mean(values)

    ranked: dict[tuple[str, str, str], list[tuple[int, str]]] = defaultdict(list)
    for row in rankings:
        ranked[(row["task"], row["prompt_id"], row["run_id"])].append((int(row["rank"]), row["image_id"]))
    run_to_candidate = {row["run_id"]: row["candidate_id"] for row in candidate_map}

    per_prompt: dict[tuple[str, str], dict[str, float]] = defaultdict(dict)
    for (task, prompt_id, run_id), values in ranked.items():
        ordered = [image_id for _, image_id in sorted(values)]
        prompt_labels = labels_by_prompt[(task, prompt_id)]
        pool_ids = set(prompt_labels)
        candidate = run_to_candidate[run_id]
        if task == "natural_language":
            per_prompt[(candidate, "search_ndcg10")][prompt_id] = ndcg(ordered, prompt_labels, pool_ids)
        elif task == "zero_shot":
            per_prompt[(candidate, "zero_shot_mean_ap")][prompt_id] = average_precision(ordered, prompt_labels, pool_ids)
        elif task == "similarity":
            per_prompt[(candidate, "similarity_recall10")][prompt_id] = recall_at_k(ordered, prompt_labels, pool_ids)
            per_prompt[(candidate, "similarity_ndcg10")][prompt_id] = ndcg(ordered, prompt_labels, pool_ids)

    metrics_path = eval_root / "blind-metrics.csv"
    with metrics_path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["candidate_id", "metric", "prompt_count", "mean"])
        for (candidate, metric), values in sorted(per_prompt.items()):
            writer.writerow([candidate, metric, len(values), f"{mean(values.values()):.8f}"])

    comparison_path = eval_root / "blind-comparisons.csv"
    rng = random.Random(int(manifest["random_seed"]))
    candidates = sorted(set(run_to_candidate.values()))
    metrics = sorted({metric for _, metric in per_prompt})
    with comparison_path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["metric", "candidate_a", "candidate_b", "difference", "ci95_low", "ci95_high", "significant"])
        for metric in metrics:
            for left_index, left_candidate in enumerate(candidates):
                for right_candidate in candidates[left_index + 1 :]:
                    left_values = per_prompt.get((left_candidate, metric), {})
                    right_values = per_prompt.get((right_candidate, metric), {})
                    prompt_ids = sorted(set(left_values) & set(right_values))
                    if not prompt_ids:
                        continue
                    point, low, high = bootstrap_difference(
                        [left_values[prompt] for prompt in prompt_ids],
                        [right_values[prompt] for prompt in prompt_ids],
                        rng,
                        iterations,
                    )
                    writer.writerow([metric, left_candidate, right_candidate, f"{point:.8f}", f"{low:.8f}", f"{high:.8f}", str(low > 0 or high < 0).lower()])

    consistency_path = eval_root / "blind-consistency.json"
    consistency_path.write_text(
        json.dumps(
            {
                "duplicate_groups": len(duplicate_groups),
                "exact_matches": exact_matches,
                "self_consistency_rate": consistency,
                "unscored_policy": "score 0",
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    return metrics_path, comparison_path, consistency_path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--eval-root", type=Path, required=True)
    parser.add_argument("--bootstrap-iterations", type=int, default=10_000)
    args = parser.parse_args()
    try:
        outputs = analyze(args.eval_root, args.bootstrap_iterations)
    except (OSError, ValueError, json.JSONDecodeError, ValidationError) as error:
        print(f"analysis failed: {error}")
        return 1
    print(json.dumps({"outputs": [str(path) for path in outputs]}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
