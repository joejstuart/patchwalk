//! Human review history. All writers use the same locked transactions.
use crate::{
    model::{Project, Result, Review, Status},
    protocol,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Write,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub format: String,
    pub version: u8,
    pub proposal_id: String,
    pub generation: u64,
    pub rounds: Vec<Round>,
    pub comments: Vec<Comment>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Round {
    pub id: String,
    pub proposal_revision: u64,
    pub proposal_digest: String,
    pub source_digest: String,
    pub context: Value,
    pub decisions: Vec<Decision>,
    pub annotation_status: BTreeMap<String, Status>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub author: String,
    pub outcome: Outcome,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pending,
    Accepted,
    ChangesRequested,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comment {
    pub id: String,
    pub round_id: String,
    pub author: String,
    pub target: Target,
    pub severity: Option<Severity>,
    pub body: String,
    pub replies: Vec<Reply>,
    pub resolved_by: Option<String>,
    pub resolved_in_round: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub author: String,
    pub body: String,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Overview,
    Annotation {
        annotation_id: String,
    },
    Code {
        path: String,
        side: Side,
        line: usize,
        end_line: Option<usize>,
    },
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Before,
    After,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub target: Target,
    pub severity: Option<Severity>,
    pub body: String,
}

pub fn nonblank(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err("Expected nonblank text".into());
    }
    Ok(())
}

impl History {
    pub fn load(project: &Project) -> Result<Self> {
        Self::load_path(&project.state_path, &project.proposal.id)
    }
    pub fn load_path(path: &std::path::Path, proposal_id: &str) -> Result<Self> {
        let empty = || Self {
            format: "proposal-nav-review".into(),
            version: 2,
            proposal_id: proposal_id.into(),
            generation: 0,
            rounds: vec![],
            comments: vec![],
        };
        if !path.exists() {
            return Ok(empty());
        }
        let value: Value = serde_json::from_slice(&fs::read(path)?)?;
        protocol::validate_review(&value)?;
        let history = if value["version"] == 1 {
            // Preserve legacy feedback even when it describes an older snapshot.
            let legacy: Review = serde_json::from_value(value)?;
            let mut history = empty();
            let mut round = Round {
                id: "round-1".into(),
                proposal_revision: legacy.proposal_revision,
                proposal_digest: legacy.proposal_digest,
                source_digest: legacy.source_digest,
                context: json!({"legacy":true}),
                decisions: vec![],
                annotation_status: BTreeMap::new(),
            };
            if legacy.approved {
                round.decisions.push(Decision {
                    author: "human".into(),
                    outcome: Outcome::Accepted,
                });
            }
            for note in legacy.notes {
                round.annotation_status.insert(note.id.clone(), note.status);
                for body in note.comments {
                    history.comments.push(Comment {
                        id: format!("comment-{}", history.comments.len() + 1),
                        round_id: round.id.clone(),
                        author: "human".into(),
                        target: Target::Annotation {
                            annotation_id: note.id.clone(),
                        },
                        severity: None,
                        body,
                        replies: vec![],
                        resolved_by: None,
                        resolved_in_round: None,
                    });
                }
            }
            history.proposal_id = legacy.proposal_id;
            history.rounds.push(round);
            history
        } else {
            serde_json::from_value(value)?
        };
        if history.proposal_id != proposal_id {
            return Err("Review belongs to a different proposal ID".into());
        }
        history.validate()?;
        Ok(history)
    }
    fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        for round in &self.rounds {
            if !ids.insert(&round.id) {
                return Err("Duplicate round ID".into());
            }
            let mut authors = HashSet::new();
            for decision in &round.decisions {
                nonblank(&decision.author)?;
                if !authors.insert(&decision.author) {
                    return Err("Duplicate reviewer decision".into());
                }
            }
        }
        let mut comments = HashSet::new();
        for comment in &self.comments {
            if !comments.insert(&comment.id) || !ids.contains(&comment.round_id) {
                return Err("Invalid comment ID or round reference".into());
            }
            nonblank(&comment.author)?;
            nonblank(&comment.body)?;
            if let Some(author) = &comment.resolved_by {
                nonblank(author)?;
            }
            if comment
                .resolved_in_round
                .as_ref()
                .is_some_and(|id| !ids.contains(id))
            {
                return Err("Unknown resolution round".into());
            }
            for reply in &comment.replies {
                nonblank(&reply.author)?;
                nonblank(&reply.body)?;
            }
        }
        Ok(())
    }
    pub fn current<'a>(&'a self, project: &Project) -> Option<&'a Round> {
        self.rounds.last().filter(|r| r.matches(project))
    }
    pub fn ensure_round(&mut self, project: &Project) -> Result<String> {
        if let Some(round) = self.current(project) {
            return Ok(round.id.clone());
        }
        let id = format!("round-{}", self.rounds.len() + 1);
        self.rounds.push(Round {
            id: id.clone(),
            proposal_revision: project.proposal.revision,
            proposal_digest: project.signature.clone(),
            source_digest: project.source_signature.clone(),
            context: context_snapshot(project)?,
            decisions: vec![],
            annotation_status: BTreeMap::new(),
        });
        Ok(id)
    }
    pub fn decision(&mut self, round_id: &str, author: &str, outcome: Outcome) -> Result<()> {
        nonblank(author)?;
        let round = self
            .rounds
            .iter_mut()
            .find(|r| r.id == round_id)
            .ok_or("Unknown round")?;
        round.decisions.retain(|d| d.author != author);
        round.decisions.push(Decision {
            author: author.into(),
            outcome,
        });
        Ok(())
    }
    pub fn add_comment(
        &mut self,
        project: &Project,
        round: &str,
        author: &str,
        finding: Finding,
    ) -> Result<String> {
        nonblank(author)?;
        nonblank(&finding.body)?;
        validate_target(project, &finding.target)?;
        let id = format!("comment-{}", self.comments.len() + 1);
        self.comments.push(Comment {
            id: id.clone(),
            round_id: round.into(),
            author: author.into(),
            target: finding.target,
            severity: finding.severity,
            body: finding.body,
            replies: vec![],
            resolved_by: None,
            resolved_in_round: None,
        });
        Ok(id)
    }
}
impl Round {
    pub fn matches(&self, project: &Project) -> bool {
        self.proposal_digest == project.signature
            && self.source_digest == project.source_signature
            && self.proposal_revision == project.proposal.revision
    }
}

pub fn validate_target(project: &Project, target: &Target) -> Result<()> {
    match target {
        Target::Overview => Ok(()),
        Target::Annotation { annotation_id } => {
            if !project
                .proposal
                .annotations
                .iter()
                .any(|a| &a.id == annotation_id)
            {
                return Err("Unknown annotation ID".into());
            }
            Ok(())
        }
        Target::Code {
            path,
            side,
            line,
            end_line,
        } => {
            let count = if let Some(diff) = project.diffs.get(path) {
                match side {
                    Side::Before => &diff.before,
                    Side::After => &diff.after,
                }
                .as_deref()
                .unwrap_or("")
                .lines()
                .count()
            } else {
                project
                    .files
                    .get(path)
                    .ok_or("Code target must be in the review context")?
                    .len()
            };
            if *line == 0 || end_line.unwrap_or(*line) < *line || end_line.unwrap_or(*line) > count
            {
                return Err("Code target is outside the selected side's line range".into());
            }
            Ok(())
        }
    }
}

pub fn context_snapshot(project: &Project) -> Result<Value> {
    let mut paths: Vec<_> = project.files.keys().chain(project.diffs.keys()).collect();
    paths.sort();
    paths.dedup();
    let files: Vec<_> = paths.into_iter().map(|path| {
        let annotations: Vec<_> = project.proposal.annotations.iter().filter(|a| &a.file == path).map(|a| &a.id).collect();
        if let Some(diff) = project.diffs.get(path) {
            json!({"path":path,"annotation_ids":annotations,"before":diff.before,"after":diff.after,
                "lines":diff.lines.iter().map(|line| json!({"kind":match line.kind {
                    crate::model::DiffKind::Context=>"context", crate::model::DiffKind::Added=>"added", crate::model::DiffKind::Removed=>"removed"},
                    "before_line":line.before,"after_line":line.after,"text":line.text})).collect::<Vec<_>>()})
        } else { json!({"path":path,"annotation_ids":annotations,"source":project.raw_files.get(path).map(|raw| raw.lines().map(str::to_owned).collect::<Vec<_>>()).unwrap_or_else(|| project.files[path].clone()),"content":project.raw_files.get(path)}) }
    }).collect();
    Ok(
        json!({"proposal_document":project.wire_proposal,"working_tree_modes":project.file_modes,"proposal":{"id":project.proposal.id,"revision":project.proposal.revision,"title":project.proposal.title,
        "request":project.proposal.request,"goal":project.proposal.request,"approach":project.proposal.summary,"architecture":project.proposal.architecture,
        "questions":project.proposal.questions,"annotations":project.proposal.annotations.iter().map(|a| json!({
            "id":a.id,"path":a.file,"before_line":a.line,"before_end_line":a.end_line,"operation":a.kind,
            "title":a.title,"intent":a.intent,"rationale":a.why,"alternatives":a.alternative})).collect::<Vec<_>>()},
        "snapshot":{"base_commit":project.proposal.base_commit,"proposal_digest":project.signature,"source_digest":project.source_signature},
        "files":files}),
    )
}

struct Lock(std::path::PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Lock before reading, then atomically replace. A crashed writer leaves an explicit
/// lock error rather than silently allowing concurrent whole-document overwrites.
pub fn transaction<T>(
    project: &Project,
    expected: Option<u64>,
    update: impl FnOnce(&mut History) -> Result<T>,
) -> Result<T> {
    let lock_path = project.state_path.with_extension("json.lock");
    let mut lock_file = fs::OpenOptions::new().write(true).create_new(true).open(&lock_path)
        .map_err(|e| format!("Cannot lock review at {}: {e}. Retry after the other writer finishes; remove a leftover lock only if no writer is running.", lock_path.display()))?;
    let _lock = Lock(lock_path);
    writeln!(lock_file, "{}", std::process::id())?;
    let mut history = History::load(project)?;
    if expected.is_some_and(|g| g != history.generation) {
        return Err("Review changed in another process; reopen the viewer before saving. Your draft has not been written.".into());
    }
    project.assert_current()?;
    let output = update(&mut history)?;
    history.generation += 1;
    history.validate()?;
    let value = serde_json::to_value(&history)?;
    protocol::validate_review(&value)?;
    let temporary = project.state_path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let _temporary = Lock(temporary.clone());
    file.write_all(&serde_json::to_vec_pretty(&value)?)?;
    file.sync_all()?;
    project.assert_current()?;
    fs::rename(&temporary, &project.state_path)?;
    Ok(output)
}

pub fn display_comment(comment: &Comment) -> String {
    let location = match &comment.target {
        Target::Overview => "overview".to_owned(),
        Target::Annotation { annotation_id } => annotation_id.clone(),
        Target::Code {
            path,
            side,
            line,
            end_line,
        } => format!(
            "{path}:{line}-{} ({})",
            end_line.unwrap_or(*line),
            match side {
                Side::Before => "before",
                Side::After => "after",
            }
        ),
    };
    let mut text = format!(
        "[{} · {} · {}{}] {} · {}",
        comment.id,
        comment.author,
        comment.round_id,
        if comment.resolved_by.is_some() {
            " · resolved"
        } else {
            " · open"
        },
        location,
        comment.body
    );
    for reply in &comment.replies {
        text.push_str(&format!("\n↳ {}: {}", reply.author, reply.body));
    }
    text
}

pub fn project_review(project: &Project) -> Result<(Review, String)> {
    let history = History::load(project)?;
    let mut review = Review::new(project);
    review.generation = history.generation;
    review.stale = !history.rounds.is_empty() && history.current(project).is_none();
    review.approved = history.current(project).is_some_and(|round| {
        round
            .decisions
            .iter()
            .any(|decision| decision.author == "human" && decision.outcome == Outcome::Accepted)
    });
    review.original_approved = review.approved;
    for note in &mut review.notes {
        if let Some(round) = history.current(project) {
            note.status = round
                .annotation_status
                .get(&note.id)
                .copied()
                .unwrap_or_default();
        }
        for comment in &history.comments {
            let matches = match &comment.target {
                Target::Annotation { annotation_id } => annotation_id == &note.id,
                Target::Code { path, .. } => project
                    .proposal
                    .annotations
                    .iter()
                    .find(|a| &a.file == path)
                    .is_some_and(|a| a.id == note.id),
                Target::Overview => false,
            };
            if matches {
                note.comments.push(display_comment(comment));
            }
        }
    }
    review.original_comments = review.notes.iter().map(|n| n.comments.clone()).collect();
    review.overview_comments = history
        .comments
        .iter()
        .filter(|c| match &c.target {
            Target::Overview => true,
            Target::Annotation { annotation_id } => !project
                .proposal
                .annotations
                .iter()
                .any(|a| &a.id == annotation_id),
            Target::Code { path, .. } => {
                !project.proposal.annotations.iter().any(|a| &a.file == path)
            }
        })
        .map(display_comment)
        .collect();
    review.original_overview_comments = review.overview_comments.clone();
    Ok((
        review,
        if history.rounds.is_empty() {
            "Review notes stay local".into()
        } else if history.current(project).is_none() {
            "Code or proposal changed since last review · comments preserved · previous approvals stale".into()
        } else {
            "Loaded shared review".into()
        },
    ))
}

pub fn save_projection(project: &Project, review: &Review) -> Result<()> {
    transaction(project, Some(review.generation), |history| {
        let round = history.ensure_round(project)?;
        let mut added = false;
        for (index, note) in review.notes.iter().enumerate() {
            let original = review
                .original_comments
                .get(index)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if !note.comments.starts_with(original) {
                return Err(
                    "Existing discussion cannot be edited through a stale projection".into(),
                );
            }
            for body in &note.comments[original.len()..] {
                history.add_comment(
                    project,
                    &round,
                    "human",
                    Finding {
                        target: Target::Annotation {
                            annotation_id: note.id.clone(),
                        },
                        severity: None,
                        body: body.clone(),
                    },
                )?;
                added = true;
            }
        }
        if !review
            .overview_comments
            .starts_with(&review.original_overview_comments)
        {
            return Err(
                "Existing overview discussion cannot be edited through a stale projection".into(),
            );
        }
        for body in &review.overview_comments[review.original_overview_comments.len()..] {
            history.add_comment(
                project,
                &round,
                "human",
                Finding {
                    target: Target::Overview,
                    severity: None,
                    body: body.clone(),
                },
            )?;
            added = true;
        }
        let current = history.rounds.last_mut().unwrap();
        current.annotation_status = review
            .notes
            .iter()
            .map(|n| (n.id.clone(), n.status))
            .collect();
        // A new finding requires renewed acceptance, including when it came from the terminal.
        if added {
            for decision in &mut current.decisions {
                decision.outcome = Outcome::Pending;
            }
        }
        if review.approval_changed || review.approved != review.original_approved {
            history.decision(
                &round,
                "human",
                if review.approved {
                    Outcome::Accepted
                } else {
                    Outcome::Pending
                },
            )?;
        }
        Ok(())
    })
}
