---
name: small-model-optimization
description: Adapts task execution strategies for small language models (SLMs). Use when working with models with limited reasoning, context capacity, instruction following or tool-use reliability. Helps decompose complex tasks, optimize context and improve execution reliability.
---

# Small Model Optimization

## Objective

Improve task execution reliability and efficiency when using
small language models.

Adapt instructions, context and execution strategies to the
capabilities and constraints of the active model.

## When to Use

Use this skill when:

- The task requires multiple reasoning or execution steps.
- The model struggles with complex instructions.
- Context capacity is limited.
- Tool calls are unreliable.
- The task has a high risk of execution errors.
- A simpler execution strategy could improve reliability.

Do not apply unnecessary optimization to simple tasks.

## Core Principles

1. Prefer explicit instructions over ambiguous requests.
2. Break complex tasks into smaller, verifiable steps.
3. Minimize irrelevant context.
4. Avoid unnecessary tool calls.
5. Validate intermediate results.
6. Preserve critical requirements and constraints.
7. Never assume a task succeeded without evidence.
8. Optimize for correctness before token efficiency.

## Workflow

### 1. Assess the Task

Identify:

- Expected outcome.
- Task complexity.
- Available tools.
- Context requirements.
- Execution constraints.
- Verification criteria.

Determine whether direct execution or task decomposition is appropriate.

### 2. Adapt the Execution Strategy

For complex tasks:

1. Divide the task into smaller subtasks.
2. Define the expected output of each subtask.
3. Identify dependencies.
4. Execute one logical step at a time.
5. Verify intermediate results.
6. Continue only when required conditions are satisfied.

For simple tasks, execute directly.

### 3. Optimize Context

- Include only relevant information.
- Preserve critical requirements and constraints.
- Avoid repeating existing information.
- Load references only when necessary.
- Summarize completed work without losing essential details.
- Prioritize information required for the next execution step.

### 4. Optimize Tool Usage

- Select the minimum tools necessary.
- Prefer deterministic tools for deterministic operations.
- Inspect tool results before dependent actions.
- Avoid repeating failed calls without changing the approach.
- Respect execution permissions and approval requirements.

### 5. Recover From Errors

When an operation fails:

1. Identify the failure.
2. Determine whether it is transient or structural.
3. Inspect available error information.
4. Adjust the execution strategy.
5. Retry only when justified.
6. Stop when recovery is unsafe or impossible.

### 6. Verify Results

Before declaring completion:

- Compare the result against the original requirements.
- Check for missing steps.
- Validate available outputs.
- Identify unresolved errors.
- Report limitations honestly.

## Model Adaptation

Use available model metadata when provided by the runtime.

Consider:

- Context window.
- Maximum output tokens.
- Tool-calling support.
- Structured output support.
- Known instruction-following limitations.

Do not invent model capabilities or assume that all SLMs behave identically.

If model metadata is unavailable, use conservative execution strategies without making unsupported assumptions.

## Constraints

- Do not sacrifice correctness solely to reduce token consumption.
- Do not split tasks that are already simple.
- Do not remove information necessary for correctness.
- Do not assume unavailable tools exist.
- Do not claim successful execution without evidence.
- Do not bypass Core security policies.

## References

Consult these documents when relevant:

- [Task Decomposition](references/task-decomposition.md)
- [Context Optimization](references/context-optimization.md)
- [Evaluation Methodology](references/evaluation-methodology.md)

## Expected Outcome

A task execution strategy that balances:

- Correctness.
- Reliability.
- Context consumption.
- Tool usage.
- Execution complexity.
