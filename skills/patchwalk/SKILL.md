---
name: patchwalk
description: Prepare validated Patchwalk artifacts for terminal review of planned or implemented code changes. Use when the user asks for a Patchwalk proposal or review handoff.
---

Use the installed `patchwalk` binary to deliver a navigable explanation of a
change. It is a human terminal viewer, not an agent review API.

Run `patchwalk --agent-guide planned` for a plan before implementation, or
`patchwalk --agent-guide implemented` for a completed Git-backed change. These
commands print the generation instructions and complete JSON schema; read the
selected guide before writing the artifact. No source checkout of the tool is
required. If the binary is unavailable, report that setup is needed.

For implemented changes, capture `git rev-parse HEAD` before work starts and
check the initial working tree. If it is already dirty, identify the intended
baseline and scope with the user; never reset, stash, or absorb unrelated work
just to create an artifact. Existing-code anchors refer to the recorded base
commit, not the modified working tree.

For implemented Git-backed changes, generate the mechanical structure first:

```sh
patchwalk prepare --repo /absolute/path/project --base RECORDED_COMMIT --output /absolute/path/change.proposal.json
```

`prepare` discovers changed files and regions, generates base-side anchors,
deterministic IDs and adjacent-region links, and validates before publishing.
It covers the entire current diff from the base, including pre-existing edits;
it cannot infer which edits belong to this task. Reuse the user's established
scope rather than asking again. It refuses existing output or adjacent feedback.
For a revision, edit the existing proposal, preserve IDs, and increment revision;
do not rerun prepare over saved work. Planned proposals still use the planned
guide directly.

For a review between existing commits, use `--base BASE --head HEAD` or
`--range BASE..HEAD` with `prepare`. Both endpoints are pinned and the review
reads both sides from Git.

Replace TODO request, summary, architecture, intent and rationale with grounded
explanations. Rename annotation titles for a useful walkthrough, record actual
validation results, and add meaningful alternatives and related links where
helpful. Remove scaffold-only questions and mark
`metadata.patchwalk_prepare.status` as `ready` when complete. Structural validation
does not establish that explanations are complete or the code is correct.

Choose a descriptive `*.proposal.json` filename outside the source paths being
reviewed, preserve IDs across revisions, and validate the result with:

```sh
patchwalk /absolute/path/change.proposal.json --repo /absolute/path/project --check
```

Fix validation errors before handing it over. Give the user the exact command to
open it, using quoted absolute paths. Leave the interactive viewer to the user
unless they explicitly request it. In later rounds, read the adjacent
`*.proposal.review.json` as human feedback, preserve it, and revise the proposal
with a higher revision. Approval is scoped to the saved snapshot and does not
authorize implementation. Never edit viewer-generated feedback on the user's
behalf merely to mark a change approved.
