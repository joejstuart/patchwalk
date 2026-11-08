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
    pub id: String,
    pub revision: u64,
    pub title: String,
    pub request: String,
    pub summary: String,
    pub architecture: String,
    pub questions: Vec<String>,
    pub base_commit: Option<String>,
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
    pub files: HashMap<String, Vec<String>>,
    pub new_files: HashSet<String>,
    pub state_path: PathBuf,
    signature: String,
    source_signature: String,
}

fn source_path(root: &Path, file: &str) -> Result<PathBuf> {
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
        let mut files = HashMap::<String, Vec<String>>::new();
        let mut new_files = HashSet::new();
        // Validate against raw source before converting tabs for display.
        let mut load_location = |target: &protocol::Target, create: bool| -> Result<()> {
            let source = source_path(&root, &target.path)?;
            if source == path || source.canonicalize().is_ok_and(|resolved| resolved == path) {
                return Err("A proposal cannot use itself as source".into());
            }
            if create {
                if fs::symlink_metadata(&source).is_ok() {
                    return Err(format!("New file already exists: {}", target.path).into());
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
                    fs::read_to_string(source)?
                        .lines()
                        .map(str::to_owned)
                        .collect(),
                );
            }
            let lines = &files[&target.path];
            let start = target.line.ok_or("Anchor line is required")?;
            let end = target.end_line.unwrap_or(start);
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
        let source_signature = protocol::digest(&serde_json::json!({
            "files": files.iter().collect::<std::collections::BTreeMap<_, _>>(),
            "new_files": new_files.iter().collect::<std::collections::BTreeSet<_>>()
        }));
        Ok(Self {
            proposal: Proposal {
                id: document.id,
                revision: document.revision,
                title: document.title,
                request: document.request,
                summary: document.overview.summary,
                architecture: document.overview.architecture,
                questions: document.overview.questions,
                base_commit: document.base_commit,
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
            state_path: path.with_extension("review.json"),
            signature,
            source_signature,
        })
    }

    pub fn read_review(&self) -> (Review, String) {
        if !self.state_path.exists() {
            return (
                Review::new(self),
                "Read-only source · Review notes stay local".into(),
            );
        }
        let load = || -> Result<Review> {
            let value = serde_json::from_str(&fs::read_to_string(&self.state_path)?)?;
            protocol::validate_review(&value)?;
            let mut review: Review = serde_json::from_value(value)?;
            if review.proposal_id != self.proposal.id
                || review.proposal_revision != self.proposal.revision
                || review.proposal_digest != self.signature
                || review.source_digest != self.source_signature
            {
                return Err(
                    "Saved review belongs to a different proposal revision; ignored".into(),
                );
            }
            let mut by_id = HashMap::new();
            for note in review.notes {
                if by_id.insert(note.id.clone(), note).is_some() {
                    return Err("Duplicate feedback annotation ID".into());
                }
            }
            if by_id.len() != self.proposal.annotations.len() {
                return Err("Feedback IDs do not match proposal".into());
            }
            review.notes = self
                .proposal
                .annotations
                .iter()
                .map(|note| {
                    by_id
                        .remove(&note.id)
                        .ok_or("Feedback IDs do not match proposal")
                })
                .collect::<std::result::Result<_, _>>()?;
            Ok(review)
        };
        match load() {
            Ok(review) => (review, "Loaded saved review".into()),
            Err(e) => (Review::new(self), format!("Could not load review: {e}")),
        }
    }

    pub fn save_review(&self, review: &Review) -> Result<()> {
        let value = serde_json::to_value(review)?;
        protocol::validate_review(&value)?;
        let temporary = self.state_path.with_extension("json.tmp");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        use std::io::Write;
        if let Err(error) = (|| -> std::io::Result<()> {
            file.write_all(&serde_json::to_vec_pretty(&value)?)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.state_path)
        })() {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        Ok(())
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
    proposal_id: String,
    proposal_revision: u64,
    proposal_digest: String,
    source_digest: String,
    #[serde(rename = "annotations")]
    pub notes: Vec<ReviewNote>,
    pub approved: bool,
}

impl Review {
    fn new(project: &Project) -> Self {
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
        assert_eq!(loaded.notes[0].comments, review.notes[0].comments);
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
        assert!(message.contains("different proposal"));
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
        value["version"] = 2.into();
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
    fn feedback_uses_ids_and_is_invalidated_by_content_or_source_changes() {
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
        feedback["annotations"].as_array_mut().unwrap().reverse();
        fs::write(&project.state_path, feedback.to_string()).unwrap();
        let (loaded, _) = project.read_review();
        assert_eq!(loaded.notes[0].id, "add-rule");
        assert_eq!(loaded.notes[0].comments, ["Feedback for the first ID"]);
        // A source change outside the anchor must still invalidate old approval.
        fs::write(
            fixture.0.join("code.ts"),
            "function submit() {}\n// changed elsewhere\n",
        )
        .unwrap();
        let updated = Project::load(&fixture.path()).unwrap();
        assert!(updated.read_review().0.notes[0].comments.is_empty());
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
                .is_empty()
        );
    }
}
