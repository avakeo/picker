#!/usr/bin/env python3
"""Recalculate provisional T0-3 scores from immutable raw measurements."""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path


def number(row: dict[str, str], column: str) -> float:
    value = row.get(column, "").strip()
    if not value:
        raise ValueError(f"missing raw metric: {column}")
    return float(value)


def normalized(value: float, values: list[float], direction: str, zero_range_score: float) -> float:
    minimum = min(values)
    maximum = max(values)
    if maximum == minimum:
        return zero_range_score
    if direction == "higher":
        return 100.0 * (value - minimum) / (maximum - minimum)
    if direction == "lower":
        return 100.0 * (maximum - value) / (maximum - minimum)
    raise ValueError(f"unknown direction: {direction}")


def hard_failures(row: dict[str, str], limits: dict[str, float]) -> list[str]:
    return [column for column, limit in limits.items() if number(row, column) > float(limit)]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    config = json.loads(args.config.read_text(encoding="utf-8"))
    with args.raw.open("r", encoding="utf-8", newline="") as handle:
        rows = list(csv.DictReader(handle))

    eligible = [row for row in rows if row.get("eligible", "").lower() == "true"]
    if not eligible:
        raise SystemExit("no eligible candidates in raw results")

    limits = config["hard_limits"]
    passing = [row for row in eligible if not hard_failures(row, limits)]
    if not passing:
        raise SystemExit("no candidates pass hard limits")
    metric_values = {
        metric["column"]: [number(row, metric["column"]) for row in passing]
        for metric in config["metrics"]
    }
    scored: list[dict[str, str]] = []
    for row in eligible:
        failures = hard_failures(row, limits)
        total = 0.0
        # One candidate has a row per provider and precision; keep both so
        # each score can be traced to its run.
        output = {
            "candidate_id": row["candidate_id"],
            "precision": row.get("precision", ""),
            "provider": row.get("provider", ""),
            "hard_fail": str(bool(failures)).lower(),
        }
        if failures:
            output["hard_failures"] = ";".join(failures)
            output["total_score"] = "0.000000"
            for metric in config["metrics"]:
                output[f"{metric['column']}_normalized"] = ""
                output[f"{metric['column']}_contribution"] = ""
            scored.append(output)
            continue
        for metric in config["metrics"]:
            column = metric["column"]
            score = normalized(
                number(row, column),
                metric_values[column],
                metric["direction"],
                float(config["zero_range_score"]),
            )
            contribution = score / 100.0 * float(metric["weight"])
            output[f"{column}_normalized"] = f"{score:.6f}"
            output[f"{column}_contribution"] = f"{contribution:.6f}"
            total += contribution
        output["hard_failures"] = ""
        output["total_score"] = f"{total:.6f}"
        scored.append(output)

    scored.sort(key=lambda row: float(row["total_score"]), reverse=True)
    fieldnames = list(scored[0])
    with args.output.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(scored)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
