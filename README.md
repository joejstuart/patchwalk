# Patchwalk

Patchwalk is a Rust + Ratatui terminal tool for understanding and reviewing
LLM-generated code changes. Start with an overview of the change, navigate to
annotations explaining where and why code belongs, inspect source or applied
diffs, and leave comments without hiding the code.

Patchwalk is a human review tool. It loads proposals produced by an LLM,
editor, or script and stores terminal review notes locally. It does not call an
LLM, apply patches, run project checks, or implement changes.

Previously named Proposal Nav. Existing `*.proposal.json` files and review history
remain compatible; their wire format identifiers are unchanged.

## Install and try it

Patchwalk is a standalone terminal binary named `patchwalk`. Build once
with Rust; the installed executable needs no Rust toolchain, source checkout,
Python, Node, or model API key. Git is required for Git-backed proposals.

From this directory, install the binary and its Codex skill:

```sh
./install.sh
patchwalk --version
patchwalk --demo
```

The installer puts the binary in `~/.local/bin` and the skill in
`~/.agents/skills/patchwalk`. It reports if the binary directory needs adding
to PATH; it does not edit shell configuration. If needed, add this to your shell
startup file, then open a new terminal:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Override destinations with `--bin-dir DIRECTORY` and `--skill-dir DIRECTORY`,
or use `--without-skill` for the viewer alone. Re-run the installer to update.
To uninstall, remove the installed binary and `patchwalk` skill directory.
The launcher `./run.sh` also works without installation. Both scripts require
Cargo on PATH.
For a fresh machine, install Rust from [rustup.rs](https://rustup.rs) first.
The first build takes longer. macOS and Linux are the supported shell-install
paths; no prebuilt release downloads are published by this checkout.

With Cargo on PATH, `cargo install --path . --locked` is another way to install
the binary, usually into `~/.cargo/bin`; install the skill separately if wanted.
A built binary can also be copied to another machine of the same OS and CPU
architecture. The demo, schemas, and generation guides travel with it.

The demo is disposable: its feedback is removed when the demo closes. Your own
proposals keep feedback beside the proposal file. The viewer requires an
interactive terminal of at least 65 columns by 16 rows; 110 by 35 or larger is
more comfortable. `prepare`, `--check` and `--agent-guide` work without a terminal.

## Use it with an agent

After installation, start a new Codex chat and ask:

> Use $patchwalk to prepare this change for review.

The skill teaches the agent to record a baseline, explain the change, write the
proposal, validate it, and give you an exact launch command. Mention whether you
want a plan before implementation or a review of completed work. Other agents
can obtain the same self-contained instructions and JSON schema directly:

```sh
patchwalk --agent-guide planned
patchwalk --agent-guide implemented
```

For a project that should routinely produce proposals, add this preference to
its existing `AGENTS.md`:

```text
For substantive code changes, use Patchwalk for the review handoff. Capture
HEAD before implementing, preserve unrelated edits, and create a validated
*.proposal.json with patchwalk --agent-guide implemented. Return the exact
viewer command. For plan-only requests use --agent-guide planned.
```

The skill is discoverable for relevant requests; installing it does not make
every coding task create a proposal. Project instructions establish that
preference. Codex's [skill documentation](https://learn.chatgpt.com/docs/build-skills)
describes discovery and explicit invocation. If the skill does not appear,
restart Codex. No model is called by the viewer itself.

## Generate a review skeleton

After implementing a change, agents can generate its structure and exact anchors:

```sh
patchwalk prepare --repo /path/to/project --output /tmp/change.proposal.json
# Use a previously recorded commit instead of HEAD when appropriate:
patchwalk prepare --repo /path/to/project --base COMMIT --output /tmp/change.proposal.json
```

`--output` is required and its parent directory must exist. `--repo` defaults to
the current directory and discovers the Git root even from a subdirectory.
`--base` defaults to `HEAD`; it accepts a revision and records its full commit
hash. Optional `--title TEXT` and `--request TEXT` set those fields directly.
The full diff from that base includes staged, unstaged, deleted, and nonignored
untracked files. It includes earlier edits; it does not identify task boundaries.

To review changes already committed, supply both endpoints or use a two-dot range:

```sh
patchwalk prepare --repo /path/to/project --base HEAD~3 --head HEAD --output /tmp/range.proposal.json
patchwalk prepare --repo /path/to/project --range 'HEAD~3..HEAD' --output /tmp/range.proposal.json
patchwalk /tmp/range.proposal.json --repo /path/to/project
```

Either form compares the two commit trees directly. You can use hashes, branches,
tags, or revision expressions as endpoints. Patchwalk records both full commit
hashes and ignores the index and working tree when generating and viewing the
proposal. Keep both commits available in the repository.

The command emits a v2 Git-backed proposal with one annotation per changed
region (nearby edits are grouped with three context lines). New files get one
`create_file` annotation. Anchors quote exact base-side lines; IDs are deterministic
for the same path, operation and target. Adjacent regions in a file are linked.
Empty base files use a virtual line 1 with empty context. Deletions, CRLF text,
and mode-only changes are supported. Renames appear as deletion plus addition.

The result is a **draft** with TODO explanations. An agent should replace the
request, overview, titles, intent and rationale; add alternatives and related
code where useful; and record actual validation results. Remove scaffold-only
questions and mark `metadata.patchwalk_prepare.status` as `ready`, then run:

```sh
patchwalk /tmp/change.proposal.json --repo /path/to/project --check
```

Validation checks structure and source references, including during prepare;
it does not check that TODOs are completed or judge correctness. Preparation
neither runs project tests nor calls a model. Existing output or adjacent review,
lock, or review-temporary files cause an error. Revisions should edit the saved
proposal and increment revision, preserving IDs and feedback. Unsupported binary,
non-UTF-8, symlink, or invalid-path inputs fail without publishing a proposal.
Prepare never changes the source, staging index, commits, or review history.

## Your first review

1. Ask the agent to prepare a proposal and wait for validation to pass.
2. Run the launch command it gives you, for example:
   `patchwalk /tmp/change.proposal.json --repo /path/to/project`.
3. Read Overview, press `Enter` to open a change, `w` for its reasoning,
   and `r` for related code. Press `c` to comment, then `Enter` to add it.
4. Press `q` to save and exit. Ask the agent to read the adjacent review JSON
   and revise the proposal. Press `R` in an open viewer to load revised code
   after saving your feedback.

## Command reference

```text
patchwalk [PROPOSAL] [--repo DIRECTORY] [--check]
patchwalk prepare --output FILE [--repo DIRECTORY] [--base REVISION [--head REVISION] | --range BASE..HEAD]
patchwalk --demo
patchwalk --agent-guide planned|implemented
patchwalk --version
patchwalk --help
```

### Open or validate a proposal

| Argument or option | Use |
| --- | --- |
| `PROPOSAL` | Path to a saved proposal JSON file. Omit it to open the bundled demo. |
| `--repo DIRECTORY` | Source repository root. Defaults to the proposal's directory. In Git mode, select the Git repository root. |
| `--check` | Validate proposal schema, IDs, source paths, line ranges, anchors, and implementation data; print a summary and exit. Does not open the viewer or save review history. |
| `--help`, `-h` | Show usage and first-use examples. |
| `--demo` | Open the disposable built-in demo. |
| `--version`, `-V` | Print the binary version. |
| `--agent-guide planned\|implemented` | Print generation instructions and schema without a source checkout. |

Repository-relative paths inside the proposal resolve against `--repo`, not the
shell's current directory. Use `--repo` when the proposal is stored separately
from its code. Paths must stay within that source root.

For example:

```sh
patchwalk /tmp/change.proposal.json --repo /path/to/project --check
patchwalk /tmp/change.proposal.json --repo /path/to/project
```

The validation summary includes the proposal version, ID, revision, annotation
count, base commit when recorded, and review file location. `--check` validates
the proposal and its source references; it does not judge the implementation's
correctness or run tests.

## Navigate the terminal

Opening a proposal starts on **Overview**, which explains the goal, approach,
architecture, open questions, and change sequence. Select a change in the
sidebar or overview and press `Enter` to inspect its code and explanation. The
sidebar always includes Overview, and `o` returns there from any code view.

For planned changes, annotations describe proposed additions or replacements
beside existing source. For implemented changes, the code pane shows a diff with
before/after line numbers. Press `w` for placement rationale and alternatives.
Press `r` to explore related code or another annotation, then `Esc` to return
with your previous position preserved.

| Key | Action |
| --- | --- |
| `n` / `p` | Next / previous annotation, wrapping across files |
| `Tab` | Switch sidebar / content focus |
| Arrows, `j/k` | Select sidebar items or overview links; scroll code |
| `PgUp` / `PgDn` | Scroll content one page |
| `Home` / `End` | Beginning / end of code view |
| `h/l`, left/right | Scroll source horizontally |
| `Enter` | Open selected overview link; expand reasoning in code |
| `w`, space | Expand / collapse annotation reasoning |
| `r` | Choose related code or another annotation; Enter follows |
| `Esc`, backspace | Return from exploration, then back to overview |
| `o` | Return directly to overview |
| `v` / `f` | Toggle an annotation's reviewed / flagged status |
| `c` | Draft an inline comment on the selected change or overview |
| `e` | View comments, select with arrows, and Enter to edit your own comment |
| `a` | Toggle whole-change approval as review metadata |
| `s` | Save review metadata |
| `R` | Reload proposal and source after saving feedback |
| `q` | Save added comments/status changes and quit |
| `Ctrl+C` | Quit without saving unsaved changes |
| `?` | Open help |

Within a popup, `Esc` closes it. Help supports arrow-key scrolling. Annotation
status commands (`v/f`) apply only while viewing an individual change. Approval
never invokes an agent or authorizes a code edit. View position is not persisted
between runs.

### Write comments beside the code

Comment counts appear beside changes in the sidebar and on the selected change.
Press `e` to open the comments list, use up/down to select, and press Enter to
edit your own comment. The editor contains only your text; metadata and the
comment target are preserved. Other authors' comments remain read-only.

Press `c` to open a docked editor beneath the code and explanation, or beneath
the overview. Its title identifies the target. The context stays visible while
you write.

| Key while drafting | Action |
| --- | --- |
| `Enter` | Add or update a nonblank comment in the in-memory review |
| `Esc` | Discard the unfinished draft |
| `Tab` | Switch between writing and reading context |
| Up/down, `PgUp/PgDn` | Scroll context without moving the comment's target |
| Left/right, Home/End | Move the text cursor while writing |
| Backspace, Delete | Edit text |
| `w` while reading | Expand / collapse the annotation's reasoning |

Pasted paragraphs preserve newlines and wrap; pasting does not submit a comment.
Navigation to other annotations is held while a draft is open. Added comments
appear even when reasoning is collapsed. Press `s` or `q` to persist them; an
unfinished or canceled draft is not saved. Edits use the same save and conflict
checks as new comments and clear approval.

## Local review history

Feedback is saved beside the proposal in `<proposal-stem>.review.json`. Terminal
comments use author `human`. Comments and earlier snapshots survive proposal and
source changes; previous approval becomes stale. Press `R` to load updated proposal and source. Reload keeps the selected
annotation by its stable ID when available, rebuilds syntax highlighting, and
returns from related-code exploration to the annotation. Save feedback and finish
or cancel drafts first; reload never discards unsaved work. Failed validation
leaves the current view intact. The viewer imports changes from other viewers while idle and rejects
conflicting saves while retaining local drafts. Malformed history is never
silently overwritten. Legacy history remains readable and migrates on save.

Approval reflects the human decision for the current snapshot. Historical
reviewer decisions and findings remain visible but do not block human approval.
Annotation status changes and new comments clear human approval.

There are no headless review, submission, context-export, reply, or resolution
commands. `--check` remains available to validate proposals without a terminal.
If a crashed writer leaves a review lock or temporary file, confirm no viewer is
writing before removing it.

## Proposal formats and source scope

Any LLM, editor, or script can write the provider-neutral proposal JSON. The
viewer validates bundled schemas, IDs, paths, source ranges, and anchor context.
Keep annotation IDs stable across revisions and increment the proposal revision
when changing its content.

| Format | Use |
| --- | --- |
| Proposal v1 | Review a plan before implementation. Annotations reference current source; proposed new files are virtual previews. |
| Proposal v2 with `implementation.source: "git"` | Review the working tree against a pinned Git commit, or compare two pinned commits with `implementation.head_commit`. |
| Proposal v2 with `implementation.files` | Review explicit before/after snapshots; the working tree must match the recorded after content. |

For Git mode, assume the repository starts clean. Record its full commit hash as
`base_commit`, implement the change, then write a v2 proposal containing
`"implementation": {"source":"git"}`. Existing-code anchors reference the base
commit; new-file annotations use `create_file` and a path. Staging is optional:
the before version comes from Git and the after version from the working tree.
Keep the base commit available until review is complete.

The terminal navigates annotated changes. Git snapshot tracking also includes other
changed tracked and nonignored untracked text files, so an unannotated edit can
make approval stale. The proposal and its own review/lock/temporary files are
excluded. New text files and deletions are supported; binary review is not.
Git commands do not change the index, branches, commits, or source files.

Planned proposals fingerprint their loaded source/evidence files. Snapshot-mode
proposals reject source drift; update their recorded after content when revising
an implementation. Handling unrelated preexisting edits and automatic code
location remapping are outside this prototype's scope.

- [Protocol and proposal examples](PROTOCOL.md)
- [Proposal v1 schema](schemas/proposal-v1.schema.json)
- [Proposal v2 schema](schemas/proposal-v2.schema.json)
- [Shared review v2 schema](schemas/review-v2.schema.json)
- [Legacy review v1 schema](schemas/review-v1.schema.json)
- [Planned-change generation prompt](prompts/generate-proposal.txt)
- [Implemented-change recording prompt](prompts/record-implemented-change.txt)
- [Runnable planned-change demo](demo/proposal.json)
- [New-file proposal example](examples/new-file.proposal.json)

The demo's TypeScript files are illustrative excerpts, not a complete executable
project. Patchwalk validates source references; reviewers assess architectural
claims and behavior independently.

## Appearance

The canvas and unchanged code inherit your terminal's default background and
foreground, including light themes and transparency. Pane borders remain clear.
Navigation uses cyan, proposal outlines yellow, and reviewed items green from
your terminal's ANSI palette. Selected entries use a marker and bold text.

Added diff lines use muted green (`#263D32`); removed lines use muted red
(`#492A32`). Stronger green/red backgrounds highlight changed characters within
replacement lines. Syntax colors remain visible on both sides, with sand-colored
strings and soft lavender keywords.

## Development

With Rust available on your PATH, from this directory:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Source and syntax highlighting are loaded and cached when opening a proposal.
The event loop polls for terminal input and checks shared feedback/source changes
periodically. Ratatui renders terminal differences, although the prototype still
constructs visible file display rows during draws. TypeScript uses the bundled
JavaScript grammar as a fallback. Large-file virtualization, automatic source
reload, and model orchestration remain outside the current implementation.

## Snapshot fidelity and historical access

Review fingerprints include exact source text, including tabs, CRLF versus LF,
and the final newline. Anchor comparison still normalizes line endings. Updating
from an older build can therefore make existing approval stale; start a fresh round.
In Git mode, fingerprints also include working-tree file modes (regular versus
executable on Unix, with absence recorded for deleted files). Mode-only changes
invalidate approval even when text is unchanged. Working-tree symlinks and other
nonregular source entries are rejected; symlink review is unsupported.

The terminal preserves raw source separately from display text; tab expansion
is presentation only. Saved history retains proposal and source snapshots for
provenance. New rounds store canonical inputs without derived proposal summaries
or display/diff line arrays; existing historical snapshots remain intact. History
has no public agent API. The source fingerprint change may make older approvals stale.
