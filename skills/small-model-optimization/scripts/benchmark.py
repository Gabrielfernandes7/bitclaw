#!/usr/bin/env python3

import argparse
import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path

REQUIRED_FIELDS = {
    "experiment_id": str,
    "run_id": str,
    "task_id": str,
    "model": str,
    "condition": str,
    "success": bool,
    "input_tokens": int,
    "output_tokens": int,
    "tool_calls": int,
    "errors": int,
    "execution_time_ms": int,
}

NUMERIC_FIELDS = (
    "input_tokens",
    "output_tokens",
    "tool_calls",
    "errors",
    "execution_time_ms",
)


def load_results(path):
    results = []

    with path.open("r", encoding="utf-8") as file:
        for line_number, line in enumerate(file, start=1):
            if not line.strip():
                continue

            try:
                record = json.loads(line)
            except json.JSONDecodeError as error:
                raise ValueError(
                    f"Linha {line_number}: JSON inválido: {error}"
                ) from error

            if not isinstance(record, dict):
                raise ValueError(f"Linha {line_number}: esperado um objeto JSON")

            for field, expected_type in REQUIRED_FIELDS.items():
                if field not in record:
                    raise ValueError(
                        f"Linha {line_number}: campo ausente: {field}"
                    )

                value = record[field]

                if expected_type is int:
                    valid = type(value) is int
                else:
                    valid = isinstance(value, expected_type)

                if not valid:
                    raise ValueError(
                        f"Linha {line_number}: tipo inválido para {field}"
                    )

            for field in ("experiment_id", "run_id", "task_id", "model", "condition"):
                if not record[field].strip():
                    raise ValueError(
                        f"Linha {line_number}: {field} não pode ser vazio"
                    )

            for field in NUMERIC_FIELDS:
                if record[field] < 0:
                    raise ValueError(
                        f"Linha {line_number}: {field} não pode ser negativo"
                    )

            results.append(record)

    if not results:
        raise ValueError("Nenhum resultado encontrado.")

    return results


def summarize(records):
    total_runs = len(records)
    successful_runs = sum(record["success"] for record in records)

    total_tokens = [
        record["input_tokens"] + record["output_tokens"]
        for record in records
    ]

    return {
        "runs": total_runs,
        "success_rate": successful_runs / total_runs,
        "avg_input_tokens": statistics.mean(
            record["input_tokens"] for record in records
        ),
        "avg_output_tokens": statistics.mean(
            record["output_tokens"] for record in records
        ),
        "avg_total_tokens": statistics.mean(total_tokens),
        "median_total_tokens": statistics.median(total_tokens),
        "avg_tool_calls": statistics.mean(
            record["tool_calls"] for record in records
        ),
        "avg_errors": statistics.mean(
            record["errors"] for record in records
        ),
        "avg_execution_time_ms": statistics.mean(
            record["execution_time_ms"] for record in records
        ),
    }


def main():
    parser = argparse.ArgumentParser(
        description="Valida e resume resultados de benchmarks do BitClaw."
    )
    parser.add_argument("input", type=Path, help="Arquivo de resultados JSONL")
    parser.add_argument(
        "--output",
        type=Path,
        help="Arquivo JSON para salvar o relatório",
    )
    args = parser.parse_args()

    try:
        results = load_results(args.input)
    except (OSError, ValueError) as error:
        print(f"Erro: {error}", file=sys.stderr)
        return 1

    groups = defaultdict(list)

    for record in results:
        key = (record["model"], record["condition"])
        groups[key].append(record)

    report = {
        "total_runs": len(results),
        "groups": [
            {
                "model": model,
                "condition": condition,
                **summarize(records),
            }
            for (model, condition), records in sorted(groups.items())
        ],
    }

    output = json.dumps(report, indent=2, ensure_ascii=False)

    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output + "\n", encoding="utf-8")
        print(f"Relatório salvo em: {args.output}")
    else:
        print(output)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
