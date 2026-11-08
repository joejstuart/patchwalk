//! Mechanical review scaffolding. Explanations remain the author's responsibility.
use crate::{
    git_baseline::GitBaseline,
    model::{Project, Result},
    protocol,
};
use serde_json::{Value, json};
use similar::{DiffTag, TextDiff};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const HELP: &str = "Usage: patchwalk prepare --output FILE [--repo DIRECTORY] [--base REVISION]
                         [--title TEXT] [--request TEXT]

Generate a draft v2 proposal covering the complete diff against HEAD (or --base),
including staged, unstaged, deleted, and nonignored untracked text files.
--repo defaults to the current directory; subdirectories discover the Git root.
Anchors and IDs are automatic. Fill in TODO explanations, then run --check.
Existing proposals and review files are never overwritten. Binary files and
symlinks are unsupported; an error leaves no proposal behind.
This is the entire diff from the base, not just changes from this agent task.

Example: patchwalk prepare --output /tmp/change.proposal.json --base HEAD";

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut output = None;
    let mut repository = std::env::current_dir()?;
    let mut revision = "HEAD".to_owned();
    let mut title = "Draft change review".to_owned();
    let mut request = "TODO: Describe the original request.".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            "--output" | "--repo" | "--base" | "--title" | "--request" => {
                let value = args
                    .next()
                    .filter(|s| !s.is_empty() && !s.starts_with("--"))
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                match arg.as_str() {
                    "--output" => output = Some(PathBuf::from(value)),
                    "--repo" => repository = PathBuf::from(value),
                    "--base" => revision = value,
                    "--title" => title = value,
                    "--request" => request = value,
                    _ => unreachable!(),
                }
            }
            _ => {
                return Err(format!(
                    "Unknown prepare argument: {arg}. See patchwalk prepare --help"
                )
                .into());
            }
        }
    }
    let output = output.ok_or("prepare requires --output FILE. See patchwalk prepare --help")?;
    let root = GitBaseline::repository_root(&repository)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let output = parent
        .canonicalize()?
        .join(output.file_name().ok_or("Output requires a filename")?);
    if output.starts_with(root.join(".git")) {
        return Err("Review output must be outside .git".into());
    }
    let feedback = output.with_extension("review.json");
    for path in [
        &output,
        &feedback,
        &output.with_extension("review.json.lock"),
        &output.with_extension("review.json.tmp"),
    ] {
        if fs::symlink_metadata(path).is_ok() {
            return Err(format!(
                "Refusing to overwrite existing proposal or review artifact: {}",
                path.display()
            )
            .into());
        }
    }
    let base = GitBaseline::new(&root, &revision)?;
    let mut annotations = Vec::new();
    let mut files = 0;
    for name in base.changed_paths()? {
        let path = crate::model::source_path(&root, &name)?;
        if path == output {
            continue;
        }
        let before = base.read(&name)?;
        let after = read_text(&path)?;
        if before.is_none() && after.is_none() {
            continue;
        }
        for text in before.iter().chain(after.iter()) {
            if text.contains('\0') {
                return Err(format!("Binary file is unsupported: {name}").into());
            }
        }
        let first = annotations.len();
        if let Some(before) = before {
            let after_text = after.as_deref().unwrap_or("");
            let lines: Vec<_> = before.lines().collect();
            for group in TextDiff::from_lines(&before, after_text).grouped_ops(3) {
                let changes: Vec<_> = group
                    .iter()
                    .filter(|op| op.tag() != DiffTag::Equal)
                    .collect();
                if changes.is_empty() {
                    continue;
                }
                let start = changes.first().unwrap().old_range().start;
                let end = changes.last().unwrap().old_range().end;
                let added: usize = changes.iter().map(|op| op.new_range().len()).sum();
                let removed: usize = changes.iter().map(|op| op.old_range().len()).sum();
                let (operation, line, last) = if removed == 0 {
                    if start < lines.len() {
                        ("insert_before", start + 1, start + 1)
                    } else if lines.is_empty() {
                        ("insert_before", 1, 1)
                    } else {
                        ("insert_after", lines.len(), lines.len())
                    }
                } else if added == 0 {
                    ("delete", start + 1, end)
                } else {
                    ("replace", start + 1, end)
                };
                let context = if lines.is_empty() {
                    String::new()
                } else {
                    lines[line - 1..last].join("\n")
                };
                let target = json!({"path":name,"line":line,"end_line":last,"context":context});
                annotations.push(annotation(
                    &name,
                    operation,
                    target,
                    &format!("{name}: base L{line}–L{last} (+{added}/−{removed})"),
                    &format!("TODO: Explain this region ({added} added, {removed} removed lines)."),
                ));
            }
            if first == annotations.len() {
                // Empty-file deletions and mode-only changes have no text diff.
                let operation = if after.is_none() { "delete" } else { "replace" };
                let target =
                    json!({"path":name,"line":1,"context":lines.first().copied().unwrap_or("")});
                annotations.push(annotation(&name, operation, target, &format!("{name}: file state"),
                    "TODO: Explain the file deletion or mode-only change; there is no line-level text diff."));
            }
        } else {
            annotations.push(annotation(
                &name,
                "create_file",
                json!({"path":name}),
                &format!("Add {name}"),
                "TODO: Explain this new file's role and behavior.",
            ));
        }
        let end = annotations.len();
        for index in first..end {
            let related: Vec<_> = [index.checked_sub(1).filter(|other| *other >= first), (index + 1 < end).then_some(index + 1)]
                .into_iter().flatten()
                .map(|other| json!({"type":"annotation","label":"Adjacent changed region in this file","annotation_id":annotations[other]["id"]})).collect();
            annotations[index]["related"] = json!(related);
        }
        files += 1;
    }
    if annotations.is_empty() {
        return Err("No changed files to prepare against the selected base".into());
    }
    let count = annotations.len();
    let id = stable_id("review", &json!(output.to_string_lossy()));
    let proposal = json!({
        "format":"proposal-nav", "version":2, "id":id, "revision":1,
        "title":title, "request":request, "base_commit":base.commit(),
        "implementation":{"source":"git"},
        "overview":{
            "summary":format!("DRAFT: {files} changed files and {count} review locations. TODO: Explain the overall approach."),
            "architecture":"TODO: Explain responsibilities, placement, and interactions between the changed components.",
            "questions":["Generated structure only: replace TODO explanations and record actual validation results before handoff.","Scope is the full current diff against the recorded base; pre-existing edits are included."]
        },
        "annotations":annotations,
        "metadata":{"patchwalk_prepare":{"status":"draft","generator_version":env!("CARGO_PKG_VERSION"),"scope":"full_diff","changed_files":files,"annotation_count":count}}
    });
    protocol::parse_proposal(&proposal)?;
    let (temporary, mut file) = Temporary::create(&output)?;
    serde_json::to_writer_pretty(&mut file, &proposal)?;
    writeln!(file)?;
    file.sync_all()?;
    drop(file);
    let project = Project::load_with_root(&temporary.0, Some(&root))?;
    project.assert_current()?;
    // link creates the final path atomically and fails if someone else created it.
    fs::hard_link(&temporary.0, &output)?;
    println!(
        "Prepared draft: {}\n{files} files · {count} annotations\nBase commit: {}\nFill in TODO explanations, then validate:\n  patchwalk {} --repo {} --check",
        output.display(),
        base.commit(),
        shell_quote(&output),
        shell_quote(&root)
    );
    Ok(())
}

fn stable_id(prefix: &str, value: &Value) -> String {
    let digest = protocol::digest(value);
    format!("{prefix}-{}", &digest[7..31])
}

fn annotation(path: &str, operation: &str, target: Value, title: &str, intent: &str) -> Value {
    json!({
        "id":stable_id("change", &json!([path,operation,target])),
        "title":title, "operation":operation, "target":target, "intent":intent,
        "rationale":"TODO: Explain why this change belongs here.",
        "alternatives":[], "related":[]
    })
}

fn read_text(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => Err(format!(
            "Source must be a regular file (symlinks unsupported): {}",
            path.display()
        )
        .into()),
        Ok(_) => fs::read_to_string(path)
            .map(Some)
            .map_err(|e| format!("Cannot read UTF-8 text {}: {e}", path.display()).into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

struct Temporary(PathBuf);
impl Temporary {
    fn create(output: &Path) -> Result<(Self, fs::File)> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = output.with_extension(format!("prepare-{}-{nonce}.tmp", std::process::id()));
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok((Self(path), file))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
