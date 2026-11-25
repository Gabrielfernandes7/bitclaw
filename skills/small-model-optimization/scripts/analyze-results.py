#!/usr/bin/env python3

import argparse
import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path

from benchmark import load_results


def mean(records, field):
    return statistics.mean(record[field] for record in records)


def success_rate(records):
    return sum(record["success"] for record in records) / len(records)


def reduction(baseline, candidate):
    if baseline == 0:
        return None

    return round((baseline - candidate) / baseline * 100, 2)


def analyze(records, baseline_condition):
    groups = defaultdict(list)

    for record in records:
        groups[(record["model"], record["condition"])].append(record)

    models = sorted({record["model"] for record in records})
    comparisons = []

    for model in models:
        baseline = groups.get((model, baseline_condition))

        if not baseline:
            print(
                f"Aviso: modelo {model} não possui condição "
                f"'{baseline_condition}'. Comparação ignorada.",
                file=sys.stderr,
            )
            continue

        baseline_success = success_rate(baseline)
        baseline_tokens = mean(
            [
                {
                    **record,
                    "total_tokens": (
                        record["input_tokens"] + record["output_tokens"]
                    ),
                }
                for record in baseline
            ],
            "total_tokens",
        )
        baseline_tools = mean(baseline, "tool_calls")
        baseline_errors = mean(baseline, "errors")
        baseline_time = mean(baseline, "execution_time_ms")

        conditions = sorted(
            condition
            for (group_model, condition) in groups
            if group_model == model and condition != baseline_condition
        )

        for condition in conditions:
            candidate = groups[(model, condition)]

            candidate_success = success_rate(candidate)
            candidate_tokens = statistics.mean(
                record["input_tokens"] + record["output_tokens"]
                for record in candidate
            )
            candidate_tools = mean(candidate, "tool_calls")
            candidate_errors = mean(candidate, "errors")
            candidate_time = mean(candidate, "execution_time_ms")

            comparisons.append({
                "model": model,
                "baseline": baseline_condition,
                "condition": condition,
                "baseline_runs": len(baseline),
                "candidate_runs": len(candidate),
                "success_rate": {
                    "baseline": round(baseline_success * 100, 2),
                    "candidate": round(candidate_success * 100, 2),
                    "delta_percentage_points": round(
                        (candidate_success - baseline_success) * 100, 2
                    ),
                },
                "total_tokens_reduction_percent": reduction(
                    baseline_tokens, candidate_tokens
                ),
                "tool_calls_reduction_percent": reduction(
                    baseline_tools, candidate_tools
                ),
                "errors_reduction_percent": reduction(
                    baseline_errors, candidate_errors
                ),
                "execution_time_reduction_percent": reduction(
                    baseline_time, candidate_time
                ),
            })

    return comparisons


def main():
    parser = argparse.ArgumentParser(
        description="Compara condições experimentais do BitClaw."
    )
    parser.add_argument("input", type=Path, help="Arquivo de resultados JSONL")
    parser.add_argument(
        "--baseline",
        default="baseline",
        help="Nome da condição de referência (padrão: baseline)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="Arquivo JSON para salvar as comparações",
    )
    args = parser.parse_args()

    try:
        records = load_results(args.input)
    except (OSError, ValueError) as error:
        print(f"Erro: {error}", file=sys.stderr)
        return 1

    report = {
        "baseline": args.baseline,
        "comparisons": analyze(records, args.baseline),
    }

    output = json.dumps(report, indent=2, ensure_ascii=False)

    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output + "\n", encoding="utf-8")
        print(f"Análise salva em: {args.output}")
    else:
        print(output)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
