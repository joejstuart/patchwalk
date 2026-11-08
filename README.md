# Proposal Nav

A Rust + Ratatui terminal prototype for exploring planned changes as annotations
anchored to existing source. No LLM is connected, and no code changes are applied.

## Run

From the scratch_pad directory:

```sh
./proposal_nav/run.sh
```

The launcher uses the isolated Rust toolchain already installed in this workspace,
or a system Cargo installation. The first release build takes a little time.
With a normal Rust installation you can also run `cargo run --release` from this directory.
Use a terminal of at least 65 columns by 16 rows; 110 by 35 or larger is more comfortable.

## Appearance

The canvas and ordinary text inherit your terminal's default background and
foreground, including light themes and transparency. ANSI colors use your terminal
palette: cyan for navigation, yellow for proposal outlines, and green for reviewed
items. Pane dividers remain explicit; selected entries use a marker and bold text
rather than a background fill. Diff additions use a muted green background
(`#263D32`), and deletions use muted red (`#492A32`). Stronger green and red
highlights mark the exact characters changed within replacement lines. Syntax
coloring is retained on both sides, with sand-colored strings and soft lavender
keywords. Unchanged code inherits the terminal background.

## A short walkthrough

1. Start on **Overview** to read the goal, approach, and sequence of changes.
2. Press `Tab` to focus the overview, use the down arrow to highlight **Validate
   before payment**, and press `Enter` to open its code annotation. Alternatively,
   use arrows in the sidebar to navigate from Overview directly into the changes.
3. Press `w` to expand the placement rationale and alternative.
4. Press `r`, select **Comparable service flow** with the down arrow, and press
   `Enter`. You are now reading evidence in the returns service.
5. Arrows or `j/k` scroll the code. `Tab` switches sidebar/content focus.
6. Press `Esc` to return to the annotation, preserving scroll and focus. Press
   `Esc` again to return to the overview with your previous link selection.
7. Reopen a change and press `c` to draft a comment beneath the code pane. The
   code and explanation remain visible. Use arrows or `PgUp/PgDn` to scroll them
   while typing; `Tab` switches to reading, where `w` expands the reasoning.
   Press `Tab` to resume typing, `Enter` to add the comment, or `Esc` to cancel.
   This does not send a question to an agent.
8. Press `f` to flag the annotation, or `v` to mark it reviewed.
9. Press `n/p` to explore other annotations. Press `o` from any change or evidence
   view to return directly to the overview. The sidebar's Overview entry is always
   available; arrow up from the first change to select it.
10. Press `q` to save review metadata and quit. Reopening always starts on the
    overview with your saved statuses and notes retained.

## Keys

| Key | Action |
| --- | --- |
| `n` / `p` | Next / previous annotation, wrapping across files |
| `Tab` | Switch sidebar / content focus |
| Arrows, `j/k` | Select sidebar items or overview links; scroll code |
| `PgUp` / `PgDn` | Scroll code one page |
| `Home` / `End` | Beginning / end of code view |
| `h/l`, left/right | Scroll source horizontally |
| `Enter` | Open selected overview link; expand reasoning in code |
| `w`, space | Expand / collapse annotation reasoning |
| `r` | Choose related code or another annotation; Enter follows |
| `Esc`, backspace | Return from exploration, then back to overview |
| `o` | Return directly to overview |
| `v` / `f` | Toggle reviewed / flagged status |
| `c` | Draft an inline review comment without covering the code |
| `a` | Toggle whole-proposal approval as local metadata |
| `s` | Save review metadata |
| `q` | Save changes and quit |
| `Ctrl+C` | Quit without saving unsaved changes |
| `?` | Help |

Within a popup, `Esc` closes it. Overview links support arrow-key selection; use
`PgUp/PgDn` to scroll the overview text. Help supports arrow-key scrolling.
Annotation review commands (`v/f/c`) apply only while viewing an individual change.
Approval never invokes an agent. Changing a status or adding a comment clears approval.
Feedback is saved only to `<proposal-stem>.review.json` beside the proposal, using
atomic replacement. The source and proposal are not rewritten. A saved review is ignored
if the proposal content, revision, or loaded source has changed. View position is not persisted between runs.

## Writing comments in context

The comment editor is docked below the code and explanation, with no overlay or
dimming. Its title identifies the annotation receiving the comment. The draft stays
visible while you scroll the context with arrows or `PgUp/PgDn`. Press `Tab` to read
and use `w` to expand or collapse reasoning, then `Tab` to resume typing. Navigation
to other annotations is held while a draft is open, so its target cannot change.

`Enter` adds a nonblank comment; `Esc` discards the draft. Left/right, Home/End,
Backspace, and Delete edit the text. Pasted paragraphs wrap and preserve newlines;
paste does not submit the comment. Added comments appear with the annotation even
when its reasoning is collapsed. `s` or `q` saves added comments to feedback; a
canceled or unfinished draft is not written to feedback.

## Provider-neutral file contract

Any LLM, editor, or script can produce the same versioned JSON artifact. The viewer
validates against bundled JSON Schemas and checks paths, IDs, ranges, and source
context. No provider-specific integration is required to open a proposal.

- [Protocol and examples](PROTOCOL.md)
- [Proposal v1 JSON Schema](schemas/proposal-v1.schema.json)
- [Review feedback v1 JSON Schema](schemas/review-v1.schema.json)
- [Generic LLM generation prompt](prompts/generate-proposal.txt)
- [Runnable existing-code example](demo/proposal.json)
- [New-file example](examples/new-file.proposal.json)

```sh
./proposal_nav/run.sh /path/to/feature.proposal.json --repo /path/to/project --check
./proposal_nav/run.sh /path/to/feature.proposal.json --repo /path/to/project
```

Without `--repo`, paths resolve under the proposal's directory. The included
TypeScript files are illustrative excerpts, not a complete executable project.
The viewer does not invoke an LLM or verify architectural claims automatically.

## Development

With Rust available on your PATH, from this directory:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Source is loaded once; the UI blocks on terminal events rather than running an
idle redraw timer. Ratatui handles differential terminal rendering. This is a
navigation prototype, not a large-repository performance benchmark: it still
constructs the current file's display rows on each input event. Syntax colors are cached at load time. TypeScript excerpts use the bundled JavaScript
grammar as a fallback. Large-file virtualization, live proposal revisions, and agent
integration are outside the current scope.

## Review an implemented change

Assume the repository starts clean. Record the current Git commit, implement the
change, and write a v2 proposal with that hash as `base_commit` and
`"implementation": {"source":"git"}`. The viewer obtains the before version from
Git and the after version from the working tree. No snapshots are required; staging
is optional, and new untracked files are supported.

The code pane shows the diff for annotated paths, alongside explanations and inline
comments. Later source edits invalidate older saved approvals when reopened.
Version 1 remains available for pre-implementation plans. Earlier explicit snapshot
bundles are also still readable.

See [implemented-change schema](schemas/proposal-v2.schema.json),
[format details](PROTOCOL.md#implemented-changes-proposal-version-2), and the
[generic recording prompt](prompts/record-implemented-change.txt).
