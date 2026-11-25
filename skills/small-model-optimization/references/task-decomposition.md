
# Task Decomposition

## Purpose

Provide a structured approach for breaking complex tasks
into smaller, independently verifiable units.

## When to Decompose

Decompose when:

- The task contains multiple independent requirements.
- Execution steps have dependencies.
- The result requires several tool interactions.
- The task is too complex to execute reliably in one pass.

Do not decompose trivial or atomic tasks.

## Procedure

1. Identify the final expected outcome.
2. Extract explicit requirements.
3. Group related requirements.
4. Identify dependencies between groups.
5. Define smaller execution units.
6. Establish completion criteria for each unit.
7. Execute and validate each unit.
8. Combine validated results.

## Important Rules

- Avoid excessive decomposition.
- Keep subtasks logically connected.
- Preserve the original requirements.
- Do not mark a subtask complete without evidence.
- Reassess the plan when new information changes dependencies.
