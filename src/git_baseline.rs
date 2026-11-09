//! Read a pinned Git commit without consulting or changing the staging index.
use crate::model::Result;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub struct GitBaseline {
    root: PathBuf,
    commit: String,
}

impl GitBaseline {
    pub fn repository_root(directory: &Path) -> Result<PathBuf> {
        let top = Self::command(directory, &["rev-parse", "--show-toplevel"])?;
        if !top.status.success() {
            return Err("Git review requires a Git repository".into());
        }
        Ok(PathBuf::from(String::from_utf8(top.stdout)?.trim()).canonicalize()?)
    }

    pub fn commit(&self) -> &str {
        &self.commit
    }

    fn command(root: &Path, args: &[&str]) -> Result<Output> {
        Ok(Command::new("git")
            .current_dir(root)
            .args(["--no-pager", "--literal-pathspecs", "--no-optional-locks"])
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()?)
    }

    pub fn new(root: &Path, commit: &str) -> Result<Self> {
        if Self::repository_root(root)? != root {
            return Err("--repo must select the Git repository root".into());
        }
        Self::resolve(root, commit, "Base")
    }

    pub fn head(&self, commit: &str) -> Result<Self> {
        Self::resolve(&self.root, commit, "Head")
    }

    fn resolve(root: &Path, commit: &str, side: &str) -> Result<Self> {
        let expression = format!("{commit}^{{commit}}");
        let verified = Self::command(
            root,
            &["rev-parse", "--verify", "--end-of-options", &expression],
        )?;
        if !verified.status.success() {
            return Err(format!("{side} commit is unavailable: {commit}").into());
        }
        Ok(Self {
            root: root.to_owned(),
            commit: String::from_utf8(verified.stdout)?.trim().to_owned(),
        })
    }

    pub fn read(&self, path: &str) -> Result<Option<String>> {
        let tree = Self::command(&self.root, &["ls-tree", "-z", &self.commit, "--", path])?;
        if !tree.status.success() {
            return Err(format!("Cannot inspect Git base path: {path}").into());
        }
        if tree.stdout.is_empty() {
            return Ok(None);
        }
        let record = String::from_utf8(tree.stdout)?;
        let metadata = record.split('\t').next().ok_or("Invalid Git tree entry")?;
        let parts: Vec<_> = metadata.split_whitespace().collect();
        if parts.get(1) == Some(&"tree") {
            // A file on the other side may occupy this directory's path.
            return Ok(None);
        }
        if parts.len() != 3 || !matches!(parts[0], "100644" | "100755") || parts[1] != "blob" {
            return Err(format!("Git source must be a regular text file: {path}").into());
        }
        let contents = Self::command(&self.root, &["cat-file", "blob", parts[2]])?;
        if !contents.status.success() {
            return Err(format!("Cannot read Git source: {path}").into());
        }
        let text = String::from_utf8(contents.stdout)?;
        if text.contains('\0') {
            return Err(format!("Binary file is unsupported: {path}").into());
        }
        Ok(Some(text))
    }

    pub fn changed_paths(&self, head: Option<&Self>) -> Result<std::collections::BTreeSet<String>> {
        let mut paths = std::collections::BTreeSet::new();
        let mut diff = vec![
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--name-only",
            "-z",
            self.commit(),
        ];
        diff.extend(head.map(Self::commit));
        diff.push("--");
        let mut commands = vec![diff];
        if head.is_none() {
            commands.push(vec!["ls-files", "--others", "--exclude-standard", "-z"]);
        }
        for args in commands {
            let output = Self::command(&self.root, &args)?;
            if !output.status.success() {
                return Err("Cannot list changed Git paths".into());
            }
            for name in String::from_utf8(output.stdout)?
                .split('\0')
                .filter(|s| !s.is_empty())
            {
                paths.insert(name.to_owned());
            }
        }
        Ok(paths)
    }
}
