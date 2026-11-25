
# Context Optimization

## Purpose

Reduce irrelevant context while preserving information
necessary for correct task execution.

## Context Prioritization

Prioritize:

1. Current user request.
2. Explicit requirements and constraints.
3. Relevant execution history.
4. Tool outputs required for the next step.
5. Supporting documentation.

## Techniques

### Progressive Retrieval

Load additional information only when required.

### Context Deduplication

Avoid repeating information already available.

### Intermediate Summarization

Summarize completed steps while preserving:

- Decisions.
- Important facts.
- Constraints.
- Unresolved issues.
- Next actions.

### Context Boundaries

Avoid mixing unrelated tasks or historical information.

## Important Rules

- Never discard critical requirements.
- Never replace evidence with unsupported summaries.
- Preserve exact identifiers and technical details.
- Do not assume that shorter context is always better.
