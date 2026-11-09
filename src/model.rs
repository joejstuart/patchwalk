use crate::protocol;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    path::{Component, Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

// UI model: wire IDs are resolved into navigation indices only after validation.
pub struct Proposal {
    pub version: u8,
    pub id: String,
    pub revision: u64,
    pub title: String,
    pub request: String,
    pub summary: String,
    pub architecture: String,
    pub questions: Vec<String>,
    pub base_commit: Option<String>,
    pub head_commit: Option<String>,
    pub annotations: Vec<Annotation>,
}

pub struct Annotation {
    pub id: String,
    pub title: String,
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub kind: String,
    pub intent: String,
    pub why: String,
    pub alternative: String,
    pub related: Vec<Related>,
}

pub enum Related {
    Annotation {
        label: String,
        annotation: usize,
    },
    Code {
        label: String,
        file: String,
        line: usize,
    },
}

impl Related {
    pub fn label(&self) -> &str {
        match self {
            Self::Annotation { label, .. } | Self::Code { label, .. } => label,
        }
    }
}

pub struct Project {
    pub proposal: Proposal,
    pub wire_proposal: serde_json::Value,
    pub raw_files: std::collections::BTreeMap<String, String>,
    pub file_modes: std::collections::BTreeMap<String, u32>,
    pub files: HashMap<String, Vec<String>>,
    pub new_files: HashSet<String>,
    pub diffs: HashMap<String, FileDiff>,
    pub state_path: PathBuf,
    pub(crate) signature: String,
    pub(crate) source_signature: String,
    proposal_path: PathBuf,
    repository_root: PathBuf,
}

#[derive(Clone, Copy, PartialEq)]
pub enum DiffKind {
    Context,
    Added,
    Removed,
}
pub struct DiffLine {
    pub before: Option<usize>,
    pub after: Option<usize>,
    pub text: String,
    pub kind: DiffKind,
    pub missing_newline: bool,
    pub emphasized: Vec<std::ops::Range<usize>>,
}
pub struct FileDiff {
    pub before: Option<String>,
    pub after: Option<String>,
    pub lines: Vec<DiffLine>,
}
impl FileDiff {
    pub(crate) fn new(file: protocol::ImplementedFile) -> Self {
        let diff = similar::TextDiff::from_lines(
            file.before.as_deref().unwrap_or(""),
            file.after.as_deref().unwrap_or(""),
        );
        let mut options = similar::InlineChangeOptions::new();
        options
            .mode(similar::InlineChangeMode::Chars)
            .min_ratio(0.0);
        // Bound optional intraline refinement; line-level diffs remain available.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
        let lines = diff
            .iter_all_inline_changes_with_options_deadline(options, Some(deadline))
            .map(|change| {
                let raw: String = change.values().iter().map(|(_, value)| *value).collect();
                let text = raw.trim_end_matches('\n').trim_end_matches('\r');
                let mut emphasized = Vec::new();
                let (mut bytes, mut characters) = (0usize, 0usize);
                for (highlight, part) in change.values() {
                    let available = text.len().saturating_sub(bytes).min(part.len());
                    let count: usize = part[..available]
                        .chars()
                        .map(|c| if c == '\t' { 4 } else { 1 })
                        .sum();
                    if *highlight && count > 0 {
                        emphasized.push(characters..characters + count);
                    }
                    characters += count;
                    bytes += part.len();
                }
                DiffLine {
                    missing_newline: change.missing_newline(),
                    before: change.old_index().map(|n| n + 1),
                    after: change.new_index().map(|n| n + 1),
                    text: text.replace('\t', "    "),
                    kind: match change.tag() {
                        similar::ChangeTag::Equal => DiffKind::Context,
                        similar::ChangeTag::Insert => DiffKind::Added,
                        similar::ChangeTag::Delete => DiffKind::Removed,
                    },
                    emphasized,
                }
            })
            .collect();
        Self {
            before: file.before,
            after: file.after,
            lines,
        }
    }
}

pub(crate) fn validate_source_path(file: &str) -> Result<()> {
    if file.contains('\\')
        || file.contains(':')
        || file
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(file)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("Expected a normalized repository-relative path: {file}").into());
    }
    Ok(())
}

pub(crate) fn source_path(root: &Path, file: &str) -> Result<PathBuf> {
    validate_source_path(file)?;
    let path = root.join(file);
    let mut ancestor = path.as_path();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or("No source parent")?
            }
            Err(e) => return Err(e.into()),
        }
    }
    if !ancestor.canonicalize()?.starts_with(root) {
        return Err(format!("Source must remain inside the repository: {file}").into());
    }
    Ok(path)
}

impl Project {
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with_root(path, None)
    }

    pub fn load_with_root(path: &Path, repository: Option<&Path>) -> Result<Self> {
        let path = path.canonicalize()?;
        let parent = path.parent().ok_or("Proposal has no parent directory")?;
        let root = repository.unwrap_or(parent).canonicalize()?;
        if !root.is_dir() {
            return Err("Repository root must be a directory".into());
        }
        let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        let document = protocol::parse_proposal(&value)?;
        let signature = protocol::digest(&value);
        let mut ids = HashMap::new();
        for (index, note) in document.annotations.iter().enumerate() {
            if ids.insert(note.id.clone(), index).is_some() {
                return Err(format!("Duplicate annotation ID: {}", note.id).into());
            }
        }
        let git_base = if document
            .implementation
            .as_ref()
            .and_then(|i| i.source.as_deref())
            == Some("git")
        {
            Some(crate::git_baseline::GitBaseline::new(
                &root,
                document
                    .base_commit
                    .as_deref()
                    .ok_or("Git mode requires base_commit")?,
            )?)
        } else {
            None
        };
        let head_commit = document
            .implementation
            .as_ref()
            .and_then(|i| i.head_commit.as_deref());
        let git_head = git_base
            .as_ref()
            .zip(head_commit)
            .map(|(base, head)| base.head(head))
            .transpose()?;
        let implementation_files = if let Some(base) = &git_base
            && let Some(head) = &git_head
        {
            let mut result = Vec::new();
            for name in base.changed_paths(Some(head))? {
                validate_source_path(&name)?;
                result.push(protocol::ImplementedFile {
                    before: base.read(&name)?,
                    after: head.read(&name)?,
                    path: name,
                });
            }
            Some(result)
        } else if let Some(base) = &git_base {
            let mut paths: std::collections::BTreeSet<_> = document
                .annotations
                .iter()
                .map(|note| note.target.path.clone())
                .collect();
            paths.extend(base.changed_paths(None)?);
            // Review artifacts are coordination data, not implementation changes.
            let excluded = [
                path.clone(),
                path.with_extension("review.json"),
                path.with_extension("review.json.lock"),
                path.with_extension("review.json.tmp"),
            ];
            paths.retain(|name| !excluded.contains(&root.join(name)));
            let mut result = Vec::new();
            for name in paths {
                let source = source_path(&root, &name)?;
                if fs::symlink_metadata(&source).is_ok_and(|m| !m.is_file()) {
                    return Err(
                        format!("Git working source must be a regular text file: {name}").into(),
                    );
                }
                let after = match fs::read_to_string(&source) {
                    Ok(text) => Some(text),
                    Err(e)
                        if e.kind() == std::io::ErrorKind::NotFound
                            && fs::symlink_metadata(&source).is_err() =>
                    {
                        None
                    }
                    Err(e) => return Err(e.into()),
                };
                result.push(protocol::ImplementedFile {
                    before: base.read(&name)?,
                    after,
                    path: name,
                });
            }
            Some(result)
        } else {
            document.implementation.and_then(|i| i.files)
        };
        let mut file_modes = std::collections::BTreeMap::new();
        if git_base.is_some() && git_head.is_none() {
            for file in implementation_files.as_ref().into_iter().flatten() {
                let source = source_path(&root, &file.path)?;
                if let Ok(metadata) = fs::symlink_metadata(&source) {
                    if !metadata.is_file() {
                        return Err(format!(
                            "Git working source must be a regular text file: {}",
                            file.path
                        )
                        .into());
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        file_modes.insert(
                            file.path.clone(),
                            if metadata.permissions().mode() & 0o111 != 0 {
                                0o100755
                            } else {
                                0o100644
                            },
                        );
                    }
                    #[cfg(not(unix))]
                    file_modes.insert(file.path.clone(), 0o100644);
                } else {
                    file_modes.insert(file.path.clone(), 0);
                }
            }
        }
        let mut diffs = HashMap::new();
        if let Some(implementation_files) = implementation_files {
            for file in implementation_files {
                if git_head.is_none() {
                    let source = source_path(&root, &file.path)?;
                    if file.before == file.after {
                        if git_base.is_some() && file.before.as_deref() != Some("") {
                            continue;
                        }
                        if git_base.is_none() {
                            return Err(
                                format!("Unchanged implementation file: {}", file.path).into()
                            );
                        }
                    }
                    let current = match fs::read_to_string(&source) {
                        Ok(text) => Some(text),
                        Err(e)
                            if e.kind() == std::io::ErrorKind::NotFound
                                && fs::symlink_metadata(&source).is_err() =>
                        {
                            None
                        }
                        Err(e) => return Err(e.into()),
                    };
                    if current != file.after {
                        return Err(format!(
                            "Implemented source drift: {} does not match the after snapshot",
                            file.path
                        )
                        .into());
                    }
                }
                if git_base.is_none()
                    && !document
                        .annotations
                        .iter()
                        .any(|note| note.target.path == file.path)
                {
                    return Err(format!("Implemented file has no annotation: {}", file.path).into());
                }
                let name = file.path.clone();
                if diffs.insert(name.clone(), FileDiff::new(file)).is_some() {
                    return Err(format!("Duplicate implementation file: {name}").into());
                }
            }
            for note in &document.annotations {
                let Some(diff) = diffs.get(&note.target.path) else {
                    if git_base.is_some() {
                        continue;
                    }
                    return Err(
                        "Every implemented annotation must refer to a captured changed file".into(),
                    );
                };
                if (note.operation == "create_file") != diff.before.is_none() {
                    return Err("create_file must refer to an absent before snapshot".into());
                }
            }
        }
        let mut files = HashMap::<String, Vec<String>>::new();
        let mut new_files = HashSet::new();
        // Validate against raw source before converting tabs for display.
        let mut load_location = |target: &protocol::Target, create: bool| -> Result<()> {
            let source = if git_head.is_some() {
                validate_source_path(&target.path)?;
                None
            } else {
                Some(source_path(&root, &target.path)?)
            };
            if source.as_ref().is_some_and(|source| {
                source == &path || source.canonicalize().is_ok_and(|resolved| resolved == path)
            }) {
                return Err("A proposal cannot use itself as source".into());
            }
            if create {
                if !diffs.contains_key(&target.path) {
                    if git_head.is_some() {
                        return Err(format!(
                            "New file is absent from the commit diff: {}",
                            target.path
                        )
                        .into());
                    }
                    if source
                        .as_ref()
                        .is_some_and(|source| fs::symlink_metadata(source).is_ok())
                    {
                        return Err(format!("New file already exists: {}", target.path).into());
                    }
                }
                if !new_files.insert(target.path.clone()) {
                    return Err(format!("Duplicate new file: {}", target.path).into());
                }
                files.insert(
                    target.path.clone(),
                    vec!["[Proposed new file; no source exists yet]".into()],
                );
                return Ok(());
            }
            // Read evidence only from real source, never a virtual new-file preview.
            if new_files.contains(&target.path) {
                return Err("Code evidence must refer to an existing file".into());
            }
            if !files.contains_key(&target.path) {
                files.insert(
                    target.path.clone(),
                    match diffs.get(&target.path) {
                        Some(diff) => diff
                            .before
                            .clone()
                            .ok_or("Evidence requires existing before source")?,
                        None => match &git_base {
                            Some(base) => base
                                .read(&target.path)?
                                .ok_or("Evidence file does not exist in base commit")?,
                            None => fs::read_to_string(source.ok_or("Missing source path")?)?,
                        },
                    }
                    .lines()
                    .map(str::to_owned)
                    .collect(),
                );
            }
            let lines = &files[&target.path];
            let start = target.line.ok_or("Anchor line is required")?;
            let end = target.end_line.unwrap_or(start);
            // Implemented changes to an empty base file have one virtual anchor.
            // There is no real source line to quote for an insertion or deletion.
            if lines.is_empty()
                && start == 1
                && end == 1
                && target.context.as_deref() == Some("")
                && (git_base.is_some()
                    || diffs
                        .get(&target.path)
                        .is_some_and(|diff| diff.before.as_deref() == Some("")))
            {
                return Ok(());
            }
            if start == 0 || end < start || end > lines.len() {
                return Err(format!(
                    "Invalid anchor {}:{start}-{end} ({} lines)",
                    target.path,
                    lines.len()
                )
                .into());
            }
            if target.context.as_deref() != Some(lines[start - 1..end].join("\n").as_str()) {
                return Err(format!(
                    "Stale anchor {}:{start}-{end}: context does not match source",
                    target.path
                )
                .into());
            }
            Ok(())
        };
        for note in &document.annotations {
            let create = note.operation == "create_file";
            load_location(&note.target, create)?;
            if matches!(note.operation.as_str(), "insert_before" | "insert_after")
                && note
                    .target
                    .end_line
                    .is_some_and(|end| Some(end) != note.target.line)
            {
                return Err("Insertion anchors must identify one line".into());
            }
            for link in &note.related {
                match link {
                    protocol::Related::Code { target, .. } => load_location(target, false)?,
                    protocol::Related::Annotation { annotation_id, .. }
                        if !ids.contains_key(annotation_id) =>
                    {
                        return Err(
                            format!("Invalid related annotation ID: {annotation_id}").into()
                        );
                    }
                    _ => {}
                }
            }
        }
        let annotations = document
            .annotations
            .into_iter()
            .map(|note| {
                let kind = match note.operation.as_str() {
                    "insert_before" => "ADD BEFORE",
                    "insert_after" => "ADD AFTER",
                    "replace" => "MODIFY",
                    "delete" => "REMOVE",
                    "create_file" => "CREATE FILE",
                    _ => unreachable!("Schema constrains operations"),
                };
                let line = note.target.line.unwrap_or(1);
                Annotation {
                    id: note.id,
                    title: note.title,
                    file: note.target.path,
                    line,
                    end_line: note.target.end_line.unwrap_or(line),
                    kind: kind.into(),
                    intent: note.intent,
                    why: note.rationale,
                    alternative: if note.alternatives.is_empty() {
                        "No alternative recorded.".into()
                    } else {
                        note.alternatives.join("\n\n")
                    },
                    related: note
                        .related
                        .into_iter()
                        .map(|link| match link {
                            protocol::Related::Annotation {
                                label,
                                annotation_id,
                            } => Related::Annotation {
                                label,
                                annotation: ids[&annotation_id],
                            },
                            protocol::Related::Code { label, target } => Related::Code {
                                label,
                                file: target.path,
                                line: target.line.expect("Validated anchor"),
                            },
                        })
                        .collect(),
                }
            })
            .collect();
        let mut raw_files = std::collections::BTreeMap::new();
        for name in files.keys().filter(|name| !new_files.contains(*name)) {
            let raw = match &git_base {
                Some(base) => base.read(name)?.ok_or("Missing base source")?,
                None => match diffs.get(name) {
                    Some(diff) => diff.before.clone().ok_or("Missing before source")?,
                    None => fs::read_to_string(source_path(&root, name)?)?,
                },
            };
            raw_files.insert(name.clone(), raw);
        }
        let mut source_state = serde_json::json!({
            "raw_files": raw_files,
            "file_modes": file_modes,
            "files": files.iter().collect::<std::collections::BTreeMap<_, _>>(),
            "new_files": new_files.iter().collect::<std::collections::BTreeSet<_>>()
        });
        if !diffs.is_empty() {
            source_state["implemented_after"] = serde_json::to_value(
                diffs
                    .iter()
                    .map(|(path, diff)| (path, &diff.after))
                    .collect::<std::collections::BTreeMap<_, _>>(),
            )?;
        }
        let source_signature = protocol::digest(&source_state);
        Ok(Self {
            wire_proposal: value,
            raw_files,
            file_modes,
            proposal: Proposal {
                version: document.version,
                id: document.id,
                revision: document.revision,
                title: document.title,
                request: document.request,
                summary: document.overview.summary,
                architecture: document.overview.architecture,
                questions: document.overview.questions,
                base_commit: document.base_commit,
                head_commit: git_head.as_ref().map(|head| head.commit().to_owned()),
                annotations,
            },
            files: files
                .into_iter()
                .map(|(path, lines)| {
                    (
                        path,
                        lines.into_iter().map(|s| s.replace('\t', "    ")).collect(),
                    )
                })
                .collect(),
            new_files,
            diffs,
            state_path: path.with_extension("review.json"),
            proposal_path: path,
            repository_root: root,
            signature,
            source_signature,
        })
    }

    pub fn reload(&self) -> Result<Self> {
        Self::load_with_root(&self.proposal_path, Some(&self.repository_root))
    }

    pub fn assert_current(&self) -> Result<()> {
        let current = self.reload()?;
        if current.signature != self.signature || current.source_signature != self.source_signature
        {
            return Err(
                "Code or proposal changed while reviewing; reload before writing feedback".into(),
            );
        }
        Ok(())
    }

    pub fn read_review(&self) -> (Review, String) {
        match crate::review::project_review(self) {
            Ok(result) => result,
            Err(error) => {
                let mut review = Review::new(self);
                review.load_failed = true;
                (review, format!("Could not load review: {error}"))
            }
        }
    }

    pub fn save_review(&self, review: &Review) -> Result<()> {
        if review.load_failed {
            return Err(
                "Review could not be loaded; refusing to overwrite existing feedback".into(),
            );
        }
        crate::review::save_projection(self, review)
    }
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Pending,
    Reviewed,
    Flagged,
}

#[derive(Default, Deserialize, Serialize)]
pub struct ReviewNote {
    pub id: String,
    pub status: Status,
    pub comments: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Review {
    format: String,
    version: u8,
    pub(crate) proposal_id: String,
    pub(crate) proposal_revision: u64,
    pub(crate) proposal_digest: String,
    pub(crate) source_digest: String,
    #[serde(rename = "annotations")]
    pub notes: Vec<ReviewNote>,
    pub approved: bool,
    #[serde(skip)]
    pub stale: bool,
    #[serde(skip)]
    pub(crate) original_approved: bool,
    #[serde(skip)]
    pub(crate) approval_changed: bool,
    #[serde(skip)]
    pub overview_comments: Vec<String>,
    #[serde(skip)]
    pub(crate) generation: u64,
    #[serde(skip)]
    pub(crate) original_comments: Vec<Vec<String>>,
    #[serde(skip)]
    pub(crate) original_overview_comments: Vec<String>,
    #[serde(skip)]
    pub(crate) load_failed: bool,
    #[serde(skip)]
    pub(crate) comment_sources: HashMap<String, crate::review::Comment>,
    #[serde(skip)]
    pub(crate) comment_edits: HashMap<String, String>,
}

impl Review {
    pub(crate) fn new(project: &Project) -> Self {
        Self {
            format: "proposal-nav-review".into(),
            version: 1,
            proposal_id: project.proposal.id.clone(),
            proposal_revision: project.proposal.revision,
            proposal_digest: project.signature.clone(),
            source_digest: project.source_signature.clone(),
            notes: project
                .proposal
                .annotations
                .iter()
                .map(|note| ReviewNote {
                    id: note.id.clone(),
                    ..Default::default()
                })
                .collect(),
            approved: false,
            stale: false,
            original_approved: false,
            approval_changed: false,
            overview_comments: vec![],
            generation: 0,
            original_comments: vec![],
            original_overview_comments: vec![],
            load_failed: false,
            comment_sources: HashMap::new(),
            comment_edits: HashMap::new(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn intraline_changes_track_characters_after_unicode_and_expanded_tabs() {
        let diff = FileDiff::new(protocol::ImplementedFile {
            path: "code.ts".into(),
            before: Some("\tlet 名 = 30;\n".into()),
            after: Some("\tlet 名 = 60;\n".into()),
        });
        assert_eq!(diff.lines.len(), 2);
        for (line, expected) in diff.lines.iter().zip(["3", "6"]) {
            assert!(line.text.starts_with("    let 名"));
            let emphasized: String = line
                .text
                .chars()
                .enumerate()
                .filter(|(index, _)| line.emphasized.iter().any(|range| range.contains(index)))
                .map(|(_, character)| character)
                .collect();
            assert_eq!(emphasized, expected);
        }
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "proposal-nav-{}-{unique}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("code.ts"), "function submit() {}\n").unwrap();
            let proposal = serde_json::json!({
                "format":"proposal-nav", "version":1, "id":"test-proposal", "revision":1,
                "title":"Test", "request":"A feature", "base_commit":null,
                "overview":{"summary":"An approach", "architecture":"Service owns coordination", "questions":[]},
                "annotations":[{"id":"add-rule", "title":"Add a rule", "operation":"insert_before",
                    "target":{"path":"code.ts", "line":1, "context":"function submit() {}"},
                    "intent":"Validate", "rationale":"Owns the flow", "alternatives":[], "related":[]}]
            });
            fs::write(root.join("proposal.json"), proposal.to_string()).unwrap();
            Self(root)
        }

        fn path(&self) -> PathBuf {
            self.0.join("proposal.json")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn review_roundtrip_leaves_sources_and_proposal_unchanged() {
        let fixture = Fixture::new();
        let original = fs::read(fixture.path()).unwrap();
        let project = Project::load(&fixture.path()).unwrap();
        let (mut review, _) = project.read_review();
        review.notes[0].status = Status::Flagged;
        review.notes[0]
            .comments
            .push("Check responsibility boundary".into());
        project.save_review(&review).unwrap();
        let (loaded, _) = project.read_review();
        assert_eq!(loaded.notes[0].status, Status::Flagged);
        assert!(loaded.notes[0].comments[0].contains(&review.notes[0].comments[0]));
        assert_eq!(fs::read(fixture.path()).unwrap(), original);
        assert_eq!(
            fs::read_to_string(fixture.0.join("code.ts")).unwrap(),
            "function submit() {}\n"
        );
        fs::write(
            fixture.path(),
            format!("{}\n", String::from_utf8(original).unwrap()),
        )
        .unwrap();
        let unchanged = Project::load(&fixture.path()).unwrap();
        let (loaded, _) = unchanged.read_review();
        assert_eq!(loaded.notes[0].status, Status::Flagged); // Formatting is not a revision.
        let mut value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(fixture.path()).unwrap()).unwrap();
        value["revision"] = 2.into();
        fs::write(fixture.path(), value.to_string()).unwrap();
        let changed = Project::load(&fixture.path()).unwrap();
        let (loaded, message) = changed.read_review();
        assert_eq!(loaded.notes[0].status, Status::Pending);
        assert!(message.contains("previous approvals stale"));
        assert!(!loaded.notes[0].comments.is_empty());
    }

    #[test]
    fn terminal_save_preserves_feedback_and_rejects_concurrent_writers() {
        let fixture = Fixture::new();
        let project = Project::load(&fixture.path()).unwrap();
        let (mut terminal, _) = project.read_review();
        terminal.notes[0].comments.push("My unsaved note".into());
        crate::review::transaction(&project, None, |history| {
            let round = history.ensure_round(&project)?;
            history.add_comment(
                &project,
                &round,
                "reviewer",
                crate::review::Finding {
                    target: crate::review::Target::Overview,
                    severity: None,
                    body: "Architecture finding".into(),
                },
            )?;
            Ok(())
        })
        .unwrap();
        assert!(
            project
                .save_review(&terminal)
                .unwrap_err()
                .to_string()
                .contains("another process")
        );
        assert_eq!(terminal.notes[0].comments, ["My unsaved note"]);
        let (mut fresh, _) = project.read_review();
        assert!(fresh.overview_comments[0].contains("reviewer"));
        fresh.notes[0].comments.push("My saved note".into());
        project.save_review(&fresh).unwrap();
        let (again, _) = project.read_review();
        project.save_review(&again).unwrap();
        let history = crate::review::History::load(&project).unwrap();
        assert_eq!(history.comments.len(), 2); // Saving again does not duplicate comments.
        assert!(history.comments.iter().any(|c| c.author == "human"));
        assert!(history.comments.iter().any(|c| c.author == "reviewer"));
    }

    #[test]
    fn legacy_review_migrates_without_losing_stale_comments_or_approval_history() {
        let fixture = Fixture::new();
        let project = Project::load(&fixture.path()).unwrap();
        let mut legacy = Review::new(&project);
        legacy.notes[0].comments.push("Legacy feedback".into());
        legacy.approved = true;
        fs::write(&project.state_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() {}\n// revision\n",
        )
        .unwrap();
        let changed = Project::load(&fixture.path()).unwrap();
        let (review, _) = changed.read_review();
        assert!(review.stale);
        assert!(!review.approved);
        assert!(review.notes[0].comments[0].contains("Legacy feedback"));
        changed.save_review(&review).unwrap();
        let history = crate::review::History::load(&changed).unwrap();
        assert_eq!(history.rounds.len(), 2);
        assert!(history.rounds[0].decisions[0].outcome == crate::review::Outcome::Accepted);
        assert_eq!(history.comments.len(), 1);
        assert_eq!(history.version, 2);
    }

    #[test]
    fn save_rejects_code_changed_since_terminal_opened_and_corrupt_review() {
        let fixture = Fixture::new();
        let project = Project::load(&fixture.path()).unwrap();
        let (review, _) = project.read_review();
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() {}\n// revision\n",
        )
        .unwrap();
        assert!(
            project
                .save_review(&review)
                .unwrap_err()
                .to_string()
                .contains("changed while reviewing")
        );
        assert!(!project.state_path.exists());
        fs::write(&project.state_path, "broken json").unwrap();
        let project = Project::load(&fixture.path()).unwrap();
        let (review, message) = project.read_review();
        assert!(message.contains("Could not load"));
        assert!(project.save_review(&review).is_err());
        assert_eq!(
            fs::read_to_string(&project.state_path).unwrap(),
            "broken json"
        );
    }

    #[test]
    fn terminal_overview_comments_share_history_and_external_reviews_refresh_safely() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let fixture = Fixture::new();
        let project = Project::load(&fixture.path()).unwrap();
        let mut app = crate::app::App::new(project);
        app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(app.comment.as_ref().unwrap().overview);
        app.paste("Human architecture question");
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.save();
        assert!(!app.dirty);
        let history = crate::review::History::load(&app.project).unwrap();
        assert!(matches!(
            history.comments[0].target,
            crate::review::Target::Overview
        ));
        crate::review::transaction(&app.project, None, |history| {
            let round = history.ensure_round(&app.project)?;
            history.decision(&round, "reviewer", crate::review::Outcome::Accepted)
        })
        .unwrap();
        app.refresh_review();
        assert!(!app.review.approved);
        // Plain save preserves historical reviewer decisions.
        app.save();
        assert_eq!(
            crate::review::History::load(&app.project).unwrap().rounds[0]
                .decisions
                .len(),
            2
        );
        // Human comment created a pending decision; explicitly accept it to finish.
        app.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.save();
        assert!(app.review.approved);
        app.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.save();
        assert!(!app.review.approved);
        app.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.save();
        assert!(app.review.approved);
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() {}\n// revision\n",
        )
        .unwrap();
        app.refresh_review();
        assert!(app.review.stale);
        assert!(!app.review.approved);
        assert!(app.review.overview_comments[0].contains("Human architecture question"));
    }

    #[test]
    fn invalid_anchors_and_related_links_fail_before_ui() {
        let fixture = Fixture::new();
        let mut json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(fixture.path()).unwrap()).unwrap();
        json["annotations"][0]["target"]["line"] = 2.into();
        fs::write(fixture.path(), json.to_string()).unwrap();
        assert!(
            Project::load(&fixture.path())
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid anchor")
        );
        json["annotations"][0]["target"]["line"] = 1.into();
        json["annotations"][0]["related"] = serde_json::json!([{"type":"annotation", "label": "Missing", "annotation_id":"missing"}]);
        fs::write(fixture.path(), json.to_string()).unwrap();
        let error = Project::load(&fixture.path()).err().unwrap().to_string();
        assert!(error.contains("Invalid related"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_source_cannot_escape_proposal_root() {
        let fixture = Fixture::new();
        let other = Fixture::new();
        fs::remove_file(fixture.0.join("code.ts")).unwrap();
        std::os::unix::fs::symlink(other.0.join("code.ts"), fixture.0.join("code.ts")).unwrap();
        assert!(
            Project::load(&fixture.path())
                .err()
                .unwrap()
                .to_string()
                .contains("inside")
        );
    }
    fn read_json(fixture: &Fixture) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(fixture.path()).unwrap()).unwrap()
    }

    fn write_json(fixture: &Fixture, value: &serde_json::Value) {
        fs::write(fixture.path(), serde_json::to_vec(value).unwrap()).unwrap();
    }

    #[test]
    fn contract_rejects_unknown_versions_fields_operations_and_duplicate_ids() {
        let fixture = Fixture::new();
        let valid = read_json(&fixture);
        let mut cases = Vec::new();
        let mut value = valid.clone();
        value["version"] = 99.into();
        cases.push(value);
        let mut value = valid.clone();
        value["unexpected"] = true.into();
        cases.push(value);
        let mut value = valid.clone();
        value["annotations"][0]["operation"] = "run_shell".into();
        cases.push(value);
        let mut value = valid.clone();
        value["annotations"][0]["target"]["line"] = 0.into();
        cases.push(value);
        let mut value = valid.clone();
        value["annotations"][0]["target"]
            .as_object_mut()
            .unwrap()
            .remove("context");
        cases.push(value);
        let mut value = valid.clone();
        value["annotations"]
            .as_array_mut()
            .unwrap()
            .push(valid["annotations"][0].clone());
        cases.push(value);
        for value in cases {
            write_json(&fixture, &value);
            assert!(
                Project::load(&fixture.path()).is_err(),
                "Accepted invalid contract: {value}"
            );
        }
    }

    #[test]
    fn anchors_validate_raw_context_and_inclusive_ranges() {
        let fixture = Fixture::new();
        let mut value = read_json(&fixture);
        fs::write(fixture.0.join("code.ts"), "first\n\tsecond\nthird\n").unwrap();
        value["annotations"][0]["operation"] = "replace".into();
        value["annotations"][0]["target"] = serde_json::json!({"path":"code.ts", "line":1, "end_line":2, "context":"first\n\tsecond"});
        write_json(&fixture, &value);
        let project = Project::load(&fixture.path()).unwrap();
        assert_eq!(project.proposal.annotations[0].end_line, 2);
        assert_eq!(project.files["code.ts"][1], "    second");
        value["annotations"][0]["target"]["context"] = "outdated text".into();
        write_json(&fixture, &value);
        assert!(
            Project::load(&fixture.path())
                .err()
                .unwrap()
                .to_string()
                .contains("Stale anchor")
        );
        value["annotations"][0]["target"]["context"] = "first\n\tsecond".into();
        value["annotations"][0]["operation"] = "insert_after".into();
        write_json(&fixture, &value);
        assert!(
            Project::load(&fixture.path()).is_err(),
            "Insertion cannot anchor a multi-line range"
        );
    }

    #[test]
    fn new_files_are_virtual_and_external_repo_roots_are_explicit() {
        let metadata = Fixture::new();
        let source = Fixture::new();
        fs::remove_file(metadata.0.join("code.ts")).unwrap();
        assert!(Project::load(&metadata.path()).is_err());
        Project::load_with_root(&metadata.path(), Some(&source.0)).unwrap();
        let mut value = read_json(&metadata);
        value["annotations"][0]["operation"] = "create_file".into();
        value["annotations"][0]["target"] = serde_json::json!({"path":"nested/new.ts"});
        write_json(&metadata, &value);
        let project = Project::load_with_root(&metadata.path(), Some(&source.0)).unwrap();
        assert!(project.new_files.contains("nested/new.ts"));
        assert!(!source.0.join("nested/new.ts").exists());
        value["annotations"][0]["target"]["path"] = "code.ts".into();
        write_json(&metadata, &value);
        assert!(Project::load_with_root(&metadata.path(), Some(&source.0)).is_err());
        value["annotations"][0]["target"]["path"] = "../escape.ts".into();
        write_json(&metadata, &value);
        assert!(Project::load_with_root(&metadata.path(), Some(&source.0)).is_err());
    }

    #[test]
    fn feedback_uses_ids_and_preserves_comments_across_content_or_source_changes() {
        let fixture = Fixture::new();
        let mut value = read_json(&fixture);
        let mut second = value["annotations"][0].clone();
        second["id"] = "second-rule".into();
        value["annotations"].as_array_mut().unwrap().push(second);
        write_json(&fixture, &value);
        let project = Project::load(&fixture.path()).unwrap();
        let (mut review, _) = project.read_review();
        review.notes[0]
            .comments
            .push("Feedback for the first ID".into());
        project.save_review(&review).unwrap();
        let mut feedback: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&project.state_path).unwrap()).unwrap();
        protocol::validate_review(&feedback).unwrap();
        feedback["comments"].as_array_mut().unwrap().reverse();
        fs::write(&project.state_path, feedback.to_string()).unwrap();
        let (loaded, _) = project.read_review();
        assert_eq!(loaded.notes[0].id, "add-rule");
        assert!(loaded.notes[0].comments[0].contains("Feedback for the first ID"));
        // A source change outside the anchor must still invalidate old approval.
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() {}\n// changed elsewhere\n",
        )
        .unwrap();
        let updated = Project::load(&fixture.path()).unwrap();
        assert!(!updated.read_review().0.notes[0].comments.is_empty());
        assert!(updated.read_review().0.stale);
        // Content changes invalidate feedback even if a generator forgot to bump revision.
        value["title"] = "A different proposal".into();
        write_json(&fixture, &value);
        assert!(
            Project::load(&fixture.path())
                .unwrap()
                .read_review()
                .0
                .notes[0]
                .comments
                .len()
                == 1
        );
    }
    #[test]
    fn implemented_proposal_anchors_before_source_and_checks_after_drift() {
        let fixture = Fixture::new();
        let mut value = read_json(&fixture);
        let before = "function submit() {}\n";
        let after = "function submit() { validate(); }\n";
        value["version"] = 2.into();
        value["implementation"] =
            serde_json::json!({"files":[{"path":"code.ts","before":before,"after":after}]});
        fs::write(fixture.0.join("code.ts"), after).unwrap();
        write_json(&fixture, &value);
        let project = Project::load(&fixture.path()).unwrap();
        assert_eq!(project.files["code.ts"][0], "function submit() {}");
        let lines = &project.diffs["code.ts"].lines;
        assert!(
            lines
                .iter()
                .any(|line| line.kind == DiffKind::Added && line.after == Some(1))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.kind == DiffKind::Removed && line.before == Some(1))
        );
        project.save_review(&project.read_review().0).unwrap();
        let history = crate::review::History::load(&project).unwrap();
        let saved = &history.rounds[0].context["files"][0];
        assert_eq!(saved["before"], before);
        assert_eq!(saved["after"], after);
        assert!(saved.get("lines").is_none());
        fs::write(fixture.0.join("code.ts"), "intervening edit\n").unwrap();
        assert!(
            Project::load(&fixture.path())
                .err()
                .unwrap()
                .to_string()
                .contains("drift")
        );
    }

    #[test]
    fn implemented_new_file_shows_real_additions_and_rejects_uncaptured_files() {
        let fixture = Fixture::new();
        let mut value = read_json(&fixture);
        value["version"] = 2.into();
        value["annotations"][0]["operation"] = "create_file".into();
        value["annotations"][0]["target"] = serde_json::json!({"path":"new.ts"});
        value["implementation"] = serde_json::json!({"files":[{"path":"new.ts","before":null,"after":"export const rule = true;\n"}]});
        fs::write(fixture.0.join("new.ts"), "export const rule = true;\n").unwrap();
        write_json(&fixture, &value);
        let project = Project::load(&fixture.path()).unwrap();
        assert!(
            project.diffs["new.ts"]
                .lines
                .iter()
                .all(|line| line.kind == DiffKind::Added)
        );
        assert!(project.new_files.contains("new.ts"));
        value["implementation"]["files"].as_array_mut().unwrap().push(serde_json::json!({"path":"code.ts","before":null,"after":"function submit() {}\n"}));
        write_json(&fixture, &value);
        assert!(
            Project::load(&fixture.path())
                .err()
                .unwrap()
                .to_string()
                .contains("no annotation")
        );
    }
    fn git(fixture: &Fixture, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(&fixture.0)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn git_proposal(fixture: &Fixture) -> serde_json::Value {
        git(fixture, &["init", "-q"]);
        git(fixture, &["add", "--", "code.ts"]);
        git(
            fixture,
            &[
                "-c",
                "user.name=Proposal Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "baseline",
            ],
        );
        let mut value = read_json(fixture);
        value["version"] = 2.into();
        value["base_commit"] = git(fixture, &["rev-parse", "HEAD"]).into();
        value["implementation"] = serde_json::json!({"source":"git"});
        value
    }

    #[test]
    fn git_mode_uses_commit_and_worktree_and_never_the_index() {
        let fixture = Fixture::new();
        let value = git_proposal(&fixture);
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() { staged(); }\n",
        )
        .unwrap();
        git(&fixture, &["add", "--", "code.ts"]);
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() { working(); }\n",
        )
        .unwrap();
        write_json(&fixture, &value);
        let project = Project::load(&fixture.path()).unwrap();
        assert_eq!(project.files["code.ts"], ["function submit() {}"]);
        assert!(
            project.diffs["code.ts"]
                .after
                .as_ref()
                .unwrap()
                .contains("working()")
        );
        assert!(git(&fixture, &["show", ":code.ts"]).contains("staged()"));
        let (mut review, _) = project.read_review();
        review.notes[0]
            .comments
            .push("Review of working version".into());
        project.save_review(&review).unwrap();
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() { newer(); }\n",
        )
        .unwrap();
        let newer = Project::load(&fixture.path()).unwrap();
        assert!(
            newer.diffs["code.ts"]
                .after
                .as_ref()
                .unwrap()
                .contains("newer()")
        );
        assert_eq!(newer.read_review().0.notes[0].comments.len(), 1);
        assert!(newer.read_review().0.stale);
    }

    #[test]
    fn git_commit_range_reads_pinned_content_and_keeps_feedback_current() {
        let fixture = Fixture::new();
        let mut value = git_proposal(&fixture);
        let before = "function submit() {}\n";
        let after = "function submit() { committed(); }\n";
        let source = fixture.0.join("code.ts");
        fs::write(&source, after).unwrap();
        git(&fixture, &["add", "--", "code.ts"]);
        git(
            &fixture,
            &[
                "-c",
                "user.name=Proposal Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "selected head",
            ],
        );
        value["implementation"]["head_commit"] = git(&fixture, &["rev-parse", "HEAD"]).into();
        write_json(&fixture, &value);
        let project = Project::load(&fixture.path()).unwrap();

        git(&fixture, &["checkout", "--detach", "HEAD^"]);
        fs::remove_file(&source).unwrap();
        fs::create_dir(&source).unwrap();
        let reloaded = project.reload().unwrap();
        let diff = &reloaded.diffs["code.ts"];
        assert_eq!(diff.before.as_deref(), Some(before));
        assert_eq!(diff.after.as_deref(), Some(after));
        project.assert_current().unwrap();
        project.save_review(&project.read_review().0).unwrap();
        let history = crate::review::History::load(&reloaded).unwrap();
        assert_eq!(history.rounds[0].context["files"][0]["after"], after);
    }

    #[test]
    fn git_mode_handles_untracked_additions_and_deleted_files_without_snapshots() {
        let fixture = Fixture::new();
        let mut value = git_proposal(&fixture);
        value["annotations"][0]["operation"] = "delete".into();
        let mut new_note = value["annotations"][0].clone();
        new_note["id"] = "new-file".into();
        new_note["operation"] = "create_file".into();
        new_note["target"] = serde_json::json!({"path":"new.ts"});
        value["annotations"].as_array_mut().unwrap().push(new_note);
        fs::remove_file(fixture.0.join("code.ts")).unwrap();
        fs::write(fixture.0.join("new.ts"), "export const added = true;\n").unwrap();
        write_json(&fixture, &value);
        let project = Project::load(&fixture.path()).unwrap();
        assert!(project.diffs["code.ts"].after.is_none());
        assert!(project.diffs["new.ts"].before.is_none());
        assert!(
            project.diffs["new.ts"]
                .lines
                .iter()
                .all(|line| line.kind == DiffKind::Added)
        );
        value["base_commit"] = serde_json::Value::Null;
        write_json(&fixture, &value);
        assert!(Project::load(&fixture.path()).is_err());
    }
    #[test]
    fn human_approval_ignores_legacy_reviewer_blocks() {
        let fixture = Fixture::new();
        let project = Project::load(&fixture.path()).unwrap();
        crate::review::transaction(&project, None, |history| {
            let round = history.ensure_round(&project)?;
            history.decision(
                &round,
                "old-agent",
                crate::review::Outcome::ChangesRequested,
            )?;
            history.add_comment(
                &project,
                &round,
                "old-agent",
                crate::review::Finding {
                    target: crate::review::Target::Overview,
                    severity: Some(crate::review::Severity::Error),
                    body: "Historical finding".into(),
                },
            )?;
            Ok(())
        })
        .unwrap();
        let (mut review, _) = project.read_review();
        assert!(!review.approved);
        review.approved = true;
        review.approval_changed = true;
        project.save_review(&review).unwrap();
        let (review, _) = project.read_review();
        assert!(review.approved);
        assert!(review.overview_comments[0].contains("Historical finding"));
    }

    #[test]
    fn raw_source_and_proposal_survive_human_history_snapshots() {
        let fixture = Fixture::new();
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() {}\r\n\t// evidence\r\n",
        )
        .unwrap();
        let project = Project::load(&fixture.path()).unwrap();
        assert_eq!(
            project.raw_files["code.ts"],
            "function submit() {}\r\n\t// evidence\r\n"
        );
        let (mut review, _) = project.read_review();
        review.approved = true;
        project.save_review(&review).unwrap();
        let history = crate::review::History::load(&project).unwrap();
        assert_eq!(
            history.rounds[0].context["proposal_document"],
            read_json(&fixture)
        );
        let context = &history.rounds[0].context;
        assert_eq!(
            context["files"][0]["content"],
            "function submit() {}\r\n\t// evidence\r\n"
        );
        assert!(context.get("proposal").is_none());
        assert!(context["files"][0].get("source").is_none());
        for contents in [
            "function submit() {}\n\t// evidence\n",
            "function submit() {}\r\n\t// evidence",
        ] {
            fs::write(fixture.0.join("code.ts"), contents).unwrap();
            assert!(
                Project::load(&fixture.path())
                    .unwrap()
                    .read_review()
                    .0
                    .stale
            );
        }
    }

    #[test]
    fn terminal_edits_saved_comments_without_changing_identity_or_target() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let fixture = Fixture::new();
        let mut app = crate::app::App::new(Project::load(&fixture.path()).unwrap());
        let press =
            |app: &mut crate::app::App, code| app.key(KeyEvent::new(code, KeyModifiers::NONE));
        app.select(0, false);
        press(&mut app, KeyCode::Char('c'));
        app.paste("Original 🙂");
        press(&mut app, KeyCode::Enter);
        app.save();
        let original = crate::review::History::load(&app.project).unwrap().comments[0].clone();
        press(&mut app, KeyCode::Char('a'));
        app.save();
        assert!(app.review.approved);
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.comment.as_ref().unwrap().text, "Original 🙂");
        press(&mut app, KeyCode::Home);
        for _ in "Original 🙂".chars() {
            press(&mut app, KeyCode::Delete);
        }
        app.paste("Updated 🙂\nWith detail");
        press(&mut app, KeyCode::Enter);
        assert!(!app.review.approved);
        assert_eq!(app.review.notes[0].comments.len(), 1);
        app.save();
        assert!(!app.dirty);
        let history = crate::review::History::load(&app.project).unwrap();
        let mut expected = serde_json::to_value(original).unwrap();
        expected["body"] = "Updated 🙂\nWith detail".into();
        assert_eq!(history.comments.len(), 1);
        assert_eq!(
            serde_json::to_value(&history.comments[0]).unwrap(),
            expected
        );
        app.save();
        assert_eq!(
            crate::review::History::load(&app.project)
                .unwrap()
                .comments
                .len(),
            1
        );
        let reopened = crate::app::App::new(Project::load(&fixture.path()).unwrap());
        assert!(reopened.review.notes[0].comments[0].contains("Updated 🙂"));
    }

    #[test]
    fn overview_comment_selection_supports_cancel_and_unsaved_edits() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let fixture = Fixture::new();
        let mut app = crate::app::App::new(Project::load(&fixture.path()).unwrap());
        let press =
            |app: &mut crate::app::App, code| app.key(KeyEvent::new(code, KeyModifiers::NONE));
        for text in ["First", "Second"] {
            press(&mut app, KeyCode::Char('c'));
            app.paste(text);
            press(&mut app, KeyCode::Enter);
        }
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.comment.as_ref().unwrap().text, "Second");
        app.paste(" canceled");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.review.overview_comments[1], "Second");
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        app.paste(" updated");
        press(&mut app, KeyCode::Enter);
        app.save();
        assert!(!app.dirty);
        let history = crate::review::History::load(&app.project).unwrap();
        assert_eq!(history.comments.len(), 2);
        assert_eq!(history.comments[1].body, "Second updated");
        // Saved overview comments also remain editable on reopen.
        let mut app = crate::app::App::new(Project::load(&fixture.path()).unwrap());
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        app.paste(" again");
        press(&mut app, KeyCode::Enter);
        app.save();
        assert!(!app.dirty);
        assert_eq!(
            crate::review::History::load(&app.project).unwrap().comments[1].body,
            "Second updated again"
        );
    }

    #[test]
    fn comment_edits_reject_other_authors_and_concurrent_writes() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let fixture = Fixture::new();
        let project = Project::load(&fixture.path()).unwrap();
        crate::review::transaction(&project, None, |history| {
            let round = history.ensure_round(&project)?;
            for (author, body) in [("human", "Original"), ("other", "Read only")] {
                history.add_comment(
                    &project,
                    &round,
                    author,
                    crate::review::Finding {
                        target: crate::review::Target::Overview,
                        severity: None,
                        body: body.into(),
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();
        let mut app = crate::app::App::new(Project::load(&fixture.path()).unwrap());
        let press =
            |app: &mut crate::app::App, code| app.key(KeyEvent::new(code, KeyModifiers::NONE));
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(app.comment.is_none());
        assert!(!app.dirty);
        press(&mut app, KeyCode::Char('e'));
        press(&mut app, KeyCode::Enter);
        app.paste(" local");
        press(&mut app, KeyCode::Enter);
        crate::review::transaction(&project, None, |history| {
            history.comments[0].body = "External update".into();
            Ok(())
        })
        .unwrap();
        app.save();
        assert!(app.dirty);
        assert!(app.review.overview_comments[0].contains("Original local"));
        assert_eq!(
            crate::review::History::load(&project).unwrap().comments[0].body,
            "External update"
        );
    }

    #[test]
    fn compact_new_snapshots_preserve_existing_historical_context() {
        let fixture = Fixture::new();
        let project = Project::load(&fixture.path()).unwrap();
        project.save_review(&project.read_review().0).unwrap();
        let mut history = crate::review::History::load(&project).unwrap();
        history.rounds[0].context["proposal"] = serde_json::json!({"legacy_export":"keep exactly"});
        history.rounds[0].context["files"][0]["source"] = serde_json::json!(["legacy line array"]);
        let old = history.rounds[0].context.clone();
        fs::write(&project.state_path, serde_json::to_vec(&history).unwrap()).unwrap();
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() {}\n// new round\n",
        )
        .unwrap();
        let changed = Project::load(&fixture.path()).unwrap();
        changed.save_review(&changed.read_review().0).unwrap();
        let history = crate::review::History::load(&changed).unwrap();
        assert_eq!(history.rounds.len(), 2);
        assert_eq!(history.rounds[0].context, old);
        assert!(history.rounds[1].context.get("proposal").is_none());
        assert_eq!(
            history.rounds[1].context["files"][0]["content"],
            "function submit() {}\n// new round\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn git_human_approval_detects_modes_and_rejects_symlink_substitution() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let fixture = Fixture::new();
        let value = git_proposal(&fixture);
        write_json(&fixture, &value);
        let project = Project::load(&fixture.path()).unwrap();
        let (mut review, _) = project.read_review();
        review.approved = true;
        project.save_review(&review).unwrap();
        fs::set_permissions(fixture.0.join("code.ts"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            Project::load(&fixture.path())
                .unwrap()
                .read_review()
                .0
                .stale
        );
        fs::write(fixture.0.join("copy.ts"), "function submit() {}\n").unwrap();
        fs::remove_file(fixture.0.join("code.ts")).unwrap();
        symlink("copy.ts", fixture.0.join("code.ts")).unwrap();
        assert!(Project::load(&fixture.path()).is_err());
    }
}
