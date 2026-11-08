# Proposal Nav file protocol, version 1

Proposal Nav consumes a provider-neutral artifact. Any LLM, editor, script, or person
can produce it. No Codex session, API key, model name, agent SDK, or MCP server is
required by the format. Integrations transport the same files; they do not define
separate provider-specific proposal types.

The normative structural contracts are:

- `schemas/proposal-v1.schema.json` — JSON Schema Draft 2020-12 for proposals.
- `schemas/review-v1.schema.json` — JSON Schema Draft 2020-12 for review feedback.

The viewer embeds both schemas and validates offline. It additionally checks the
semantic and filesystem rules below. Schema compliance alone does not establish
that the proposed architecture is correct.

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
source file, so changes outside an anchored range invalidate old review state on
reopening. Files not referenced by the proposal are not fingerprinted. The viewer
currently loads a snapshot; it does not watch for changes during a review.

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

## Review feedback

The viewer owns feedback. Opening `feature.proposal.json` writes feedback to
`feature.proposal.review.json` when the reviewer saves; it never rewrites the proposal.
Feedback contains:

```text
format: "proposal-nav-review"
version: 1
proposal_id: the proposal's ID
proposal_revision: the reviewed revision
proposal_digest: "sha256:" plus 64 lowercase hex characters
source_digest: "sha256:" plus 64 lowercase hex characters
approved: true or false
annotations: [{ id, status: "pending" | "reviewed" | "flagged", comments: [...] }]
```

Feedback must have exactly one entry per annotation ID, with no duplicate IDs.
Its array order is irrelevant; the viewer resolves it by ID. Comments are nonblank
strings. Approval applies to the entire recorded snapshot. Flagging, changing a
review status, or adding a comment clears a previous approval. Approval is metadata
and never runs an agent or changes source files.

The viewer computes the digests; an LLM must not invent or recompute them.
`proposal_digest` is SHA-256 of compact JSON with recursively sorted object keys,
preserving array order. It is not an RFC 8785 interchange commitment: consumers
should treat the viewer-generated value as opaque. Formatting/key-order changes
alone preserve feedback. The source digest covers the viewer's loaded raw lines
and the set of virtual new-file paths, before tab expansion. It is also opaque.

Changing IDs, revision, proposal content, or loaded source invalidates saved review
state. Existing feedback is left on disk until a subsequent explicit save. Revisions
do not automatically transfer approvals or comments; producers may use old comments
as context, then request a fresh review. Never treat a boolean approval as permission
for a different revision or different source snapshot.

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
./proposal_nav/run.sh --check

# A proposal stored separately from the repository
./proposal_nav/run.sh /path/to/feature.proposal.json --repo /path/to/project --check

# Open it after successful validation
./proposal_nav/run.sh /path/to/feature.proposal.json --repo /path/to/project

# New-file example, from the scratch_pad directory
./proposal_nav/run.sh proposal_nav/examples/new-file.proposal.json --check
```

Validation exits nonzero for invalid JSON, unsupported versions, schema violations,
invalid paths or IDs, missing source, invalid ranges, and stale anchor context. Schema
errors include instance paths. The runtime resolves only bundled local schema refs;
proposal data cannot supply additional schemas or cause schema network fetches.
