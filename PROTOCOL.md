# Patchwalk file protocol, version 1

Patchwalk consumes a provider-neutral artifact. Any LLM, editor, script, or person
can produce it. No Codex session, API key, model name, agent SDK, or MCP server is
required by the format. Integrations transport the same files; they do not define
separate provider-specific proposal types.

The normative structural contracts are:

- `schemas/proposal-v1.schema.json` — JSON Schema Draft 2020-12 for proposals.
- `schemas/review-v1.schema.json` — JSON Schema Draft 2020-12 for review feedback.

The viewer embeds both schemas and validates offline. It additionally checks the
semantic and filesystem rules below. Schema compliance alone does not establish
that the proposed architecture is correct.

Patchwalk retains the `proposal-nav` and `proposal-nav-review` wire identifiers
and schema URNs for compatibility with existing artifacts and saved reviews.
The product and executable are now named Patchwalk and `patchwalk`.

## Proposal document

A complete, minimal existing-code example, assuming `src/orders/service.ts` has
`  const payment = await payments.charge(order);` on line 8:

```json
{
  "format": "proposal-nav",
  "version": 1,
  "id": "order-eligibility",
  "revision": 1,
  "title": "Check order eligibility before payment",
  "request": "Reject ineligible orders before charging customers.",
  "overview": {
    "summary": "Validate in the shared order submission service before payment.",
    "architecture": "The service coordinates submission. The route translates its result into HTTP responses.",
    "questions": ["Which rejection reasons should be exposed to callers?"]
  },
  "base_commit": null,
  "annotations": [
    {
      "id": "validate-before-payment",
      "title": "Validate before payment",
      "operation": "insert_before",
      "target": {
        "path": "src/orders/service.ts",
        "line": 8,
        "context": "  const payment = await payments.charge(order);"
      },
      "intent": "Call validateOrder and return a rejection before attempting payment.",
      "rationale": "Every order submission path uses this service boundary.",
      "alternatives": ["Validating only in the HTTP route would miss other callers."],
      "related": []
    }
  ]
}
```

`demo/proposal.json` is the full runnable example. `examples/new-file.proposal.json`
shows a new-file proposal that can be validated without existing source.

| Field | Meaning |
| --- | --- |
| `format`, `version` | Exactly `proposal-nav` and integer `1` |
| `id` | Stable ID for this proposal, independent of title and filename |
| `revision` | Positive integer; increment when revising proposal content |
| `title`, `request` | Short title and the original requested behavior |
| `overview.summary` | Whole-change approach |
| `overview.architecture` | Relevant responsibilities and relationships in the existing system |
| `overview.questions` | Unresolved decisions; an empty array is valid |
| `base_commit` | Full 40- or 64-character Git commit hash, or `null` if unavailable |
| `annotations` | Nonempty ordered walkthrough of proposed changes |
| `metadata` | Optional free-form JSON object for producer-specific provenance |

IDs contain 1–128 ASCII letters, digits, dots, underscores, or hyphens, starting
with a letter or digit. Annotation IDs must be unique within a proposal. Preserve
IDs when revising or reordering the same logical changes; never reuse an old ID
for an unrelated change. Related links refer to IDs, never array indices.

Unknown fields are rejected outside `metadata`. A producer may record its name,
model, or generation time in `metadata`, but the viewer does not require or execute
that information. The version is an integer protocol version, not a model version.
Breaking format changes require a new version. This replaces the prototype's
unversioned format; old files are not silently interpreted as v1.

## Annotations and anchors

Every annotation requires `id`, `title`, `operation`, `target`, `intent`,
`rationale`, `alternatives`, and `related`. Empty alternative/link arrays are valid.
Explanations are display text, not executable commands or automatic patches.

| Operation | Target | Meaning |
| --- | --- | --- |
| `insert_before` | Existing single-line anchor | Add something before this line |
| `insert_after` | Existing single-line anchor | Add something after this line |
| `replace` | Existing line or inclusive range | Modify this source |
| `delete` | Existing line or inclusive range | Remove this source |
| `create_file` | `{ "path": "src/new.ts" }` only | Propose a file that does not exist yet |

Existing-code anchors require `path`, one-based `line`, and `context`. Optional
`end_line` is inclusive and defaults to `line`. It must not precede `line` or extend
past EOF. Insertion anchors must identify one line. Multiple annotations may share
an insertion point; this is an explanation sequence, not an ordered patch executor.

`context` must exactly match the anchored source lines joined with `\n`, without a
trailing newline. Preserve tabs and spaces; the viewer expands tabs only for display.
CRLF line endings are normalized to LF for this comparison. An empty string may
anchor an existing blank line. The viewer rejects mismatches rather than silently
moving an annotation. A source match establishes location, not the correctness of
an architectural claim.

All paths use `/` and are relative to a user-selected repository root. Absolute
paths, drive prefixes, backslashes, empty components, `.` and `..` components, and
symlink escapes are rejected. The default root is the proposal's directory; choose
an independent source root with `--repo`. The artifact cannot choose an arbitrary
filesystem root for the viewer. Proposal and feedback files can live outside the
repository, and source files are never copied or edited by loading a proposal.

`create_file` anchors omit line numbers and context. The viewer renders a clearly
marked virtual preview; it never creates the source file. The path must be absent,
and any existing parent must resolve inside the repository. A broken symlink is
not an absent file. References to existing code must refer to real source, not a
virtual new-file preview.

`base_commit` records provenance and is not itself checked against Git. Exact
anchor context is checked on load. Review feedback also fingerprints every loaded
source file, so changes outside an anchored range make earlier approvals stale.
For planned proposals, files not referenced by the proposal are not fingerprinted.
Git review additionally covers all changed text files. The viewer periodically
checks for changed source and feedback; reopen to load a changed source snapshot.

## Related links

Link to another proposed change by its stable ID:

```json
{
  "type": "annotation",
  "label": "Rule definition",
  "annotation_id": "eligibility-rules"
}
```

Link to evidence in existing code using a normal source anchor:

```json
{
  "type": "code",
  "label": "HTTP caller",
  "target": {
    "path": "src/routes/orders.ts",
    "line": 6,
    "context": "  const result = await submitOrder(order);"
  }
}
```

Forward links are allowed, but all IDs must exist. Evidence ranges are validated;
following a link opens its first line. Self-links and cycles are allowed because
these are navigational references, not execution dependencies.

## Shared review feedback

The terminal stores `<proposal-stem>.review.json`, version 2, using
[review-v2.schema.json](schemas/review-v2.schema.json). This is local human review
history, not an agent interface. Comments retain stable IDs and original targets;
rounds retain proposal/source snapshots and human approval metadata. Old comments
and decisions remain readable for compatibility. Only the human decision for the
current snapshot controls approval; historical reviewer decisions and error
findings do not block it. New comments and annotation status changes clear human
approval. Source or proposal changes make previous approval stale.

Writes use a lock, generation checks, and atomic replacement. Conflicting saves
retain the local draft and fail instead of overwriting another viewer's changes.
Legacy v1 history migrates on save; its original source cannot be reconstructed.

## Producer workflow

1. Read the user's request, relevant source, project conventions, and schema.
2. Investigate without editing source. State uncertainties in `overview.questions`.
3. Generate one complete UTF-8 JSON document, preserving IDs across revisions.
4. Validate shape against the schema, then validate source anchors with the viewer.
5. Publish atomically using a temporary file and rename, so a future watcher never
   sees half a document. Choose the final basename before review begins.
6. Let the reviewer inspect the proposal. Read the viewer-written feedback file
   when asked to continue. Revise the proposal rather than overwriting feedback.

A generic generation prompt is in `prompts/generate-proposal.txt`. Skills, MCP tools,
CLI adapters, and app integrations can wrap that workflow without changing the file
contract. Publishing proposals does not currently push messages into any chat app.

## Validation commands

```sh
# Bundled example and its original source excerpts
./run.sh --check

# A proposal stored separately from the repository
./run.sh /path/to/feature.proposal.json --repo /path/to/project --check

# Open it after successful validation
./run.sh /path/to/feature.proposal.json --repo /path/to/project

# New-file example, from the scratch_pad directory
./run.sh examples/new-file.proposal.json --check
```

Validation exits nonzero for invalid JSON, unsupported versions, schema violations,
invalid paths or IDs, missing source, invalid ranges, and stale anchor context. Schema
errors include instance paths. The runtime resolves only bundled local schema refs;
proposal data cannot supply additional schemas or cause schema network fetches.

## Implemented changes: proposal version 2

The default implemented-review workflow assumes the repository starts clean. Record
its current full commit hash before editing. Set `version` to `2`, `base_commit` to
that hash, and add:

```json
{"implementation": {"source": "git"}}
```

No file snapshots are needed. The viewer reads original files from that commit
using Git, reads the result from the working tree, and renders the diff for the
annotated paths. Snapshot tracking also includes other changed text files. Staging is irrelevant: both staged and unstaged edits appear.
Untracked new files and deletions are supported. No Git commands alter the index,
branches, commits, or working files. Git must be installed and `--repo` must name the
repository root. A full hash is required rather than a moving reference like HEAD.

Existing-code anchors and evidence refer to the base commit's lines. New-file
annotations use `create_file` and a path only. Annotate the intended changes;
Git review also includes unannotated changed files. Annotation anchors remain
readable if a later revision restores their target file to the baseline. Handling
unrelated existing changes is outside the clean-start workflow.

The code pane shows before/after line numbers and colored additions/removals.
Annotations and comments remain available. Later working-tree edits are reflected
on reopening; feedback fingerprints the loaded result, so previous approvals do
not silently apply to a different working tree. The viewer periodically detects
source changes and marks approval stale; reopen to reload code. Keep the pinned
base commit available until review is complete.

Use `schemas/proposal-v2.schema.json` and the provider-neutral workflow in
`prompts/record-implemented-change.txt`. Version 1 remains supported for plans.

### Optional snapshot compatibility

The earlier v2 snapshot representation is still accepted:

```json
{"implementation":{"files":[{"path":"src/example.py","before":"old\n","after":"new\n"}]}}
```

This explicit snapshot mode is optional, not part of the clean-Git workflow. Values
are complete UTF-8 file strings, with null representing absence. The viewer checks
the checkout against captured after content and rejects drift. The Git and snapshot
representations cannot be combined. Shared review history supports both forms;
each round binds feedback to its exact loaded result.

## Snapshot fidelity and historical access

Review fingerprints include exact source text, including tabs, CRLF versus LF,
and the final newline. Anchor comparison still normalizes line endings. Updating
from an older build can therefore make existing approval stale; start a fresh round.
In Git mode, fingerprints also include working-tree file modes (regular versus
executable on Unix, with absence recorded for deleted files). Mode-only changes
invalidate approval even when text is unchanged. Working-tree symlinks and other
nonregular source entries are rejected; symlink review is unsupported.

Raw source is retained separately from terminal display text. Saved history
preserves exact source and the original proposal JSON for provenance. No command
exports context or accepts reviewer submissions; humans review through the TUI.

## Generated implemented-review scaffolds

`patchwalk prepare --output FILE --repo ROOT [--base REVISION]` creates a
validated v2 Git-backed draft from the entire current diff. It emits base-side
anchors and deterministic annotation IDs, with TODO explanations for the author.
It does not introduce a new wire version or authorize code changes. Empty
existing files in implemented proposals may use the virtual anchor line 1,
end_line 1, context ""; planned-proposal source anchors remain real source lines.
For existing proposals, edit and increment revision rather than overwriting them
with a new scaffold. The command refuses existing proposals or adjacent feedback.
