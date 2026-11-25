
# Evaluation Methodology

## Purpose

Evaluate whether optimization strategies improve
small language model task execution.

## Experimental Groups

### Baseline

Execute tasks without the optimization skill.

### Treatment

Execute equivalent tasks with the optimization skill.

## Controlled Variables

Keep consistent:

- Model and model version.
- System instructions.
- User tasks.
- Available tools.
- Tool permissions.
- Inference parameters.
- Execution environment.

## Metrics

### Task Success Rate

Percentage of tasks completed correctly.

### Token Consumption

Total input and output tokens per task.

### Tool Calls

Number of tool invocations per task.

### Error Rate

Percentage of executions containing errors.

### Execution Time

Time required to complete each task.

## Evaluation Rules

- Use equivalent task sets.
- Repeat experiments to account for variability.
- Evaluate output correctness independently.
- Record failed and incomplete executions.
- Compare efficiency only alongside correctness.

## Interpretation

A successful optimization should improve reliability
or efficiency without unacceptable degradation in
the other metrics.

Lower token consumption alone does not demonstrate
better performance.
