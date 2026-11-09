use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "patchwalk-prepare-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let f = Self(root);
        f.git(&["init", "-q"]);
        f.git(&["config", "user.name", "Fixture"]);
        f.git(&["config", "user.email", "fixture@example.invalid"]);
        f.git(&["config", "core.autocrlf", "false"]);
        f.git(&["config", "core.filemode", "true"]);
        f
    }
    fn git(&self, args: &[&str]) -> String {
        let result = Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    }
    fn write(&self, name: &str, text: &str) {
        fs::write(self.0.join(name), text).unwrap();
    }
    fn commit(&self) {
        self.git(&["add", "."]);
        self.git(&["-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]);
    }
    fn prepare(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_patchwalk"))
            .arg("prepare")
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
    fn valid(&self, path: &Path) -> Value {
        let result = Command::new(env!("CARGO_BIN_EXE_patchwalk"))
            .arg(path)
            .arg("--repo")
            .arg(&self.0)
            .arg("--check")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn prepare_covers_regions_additions_deletions_crlf_and_empty_base_files() {
    let f = Fixture::new();
    let before = (1..=40).map(|n| format!("line {n}\n")).collect::<String>();
    f.write("regions.txt", &before);
    f.write("delete.txt", "gone\n");
    f.write("empty-delete.txt", "");
    f.write("empty-grow.txt", "");
    f.write("crlf.txt", "a\r\nb\r\n");
    f.write(".gitignore", "ignored.txt\n");
    f.commit();
    let base = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    f.write(
        "regions.txt",
        &format!("first\n{}last\n", before.replace("line 20\n", "changed\n")),
    );
    fs::remove_file(f.0.join("delete.txt")).unwrap();
    fs::remove_file(f.0.join("empty-delete.txt")).unwrap();
    f.write("empty-grow.txt", "now populated\n");
    f.write("crlf.txt", "a\r\nnew\r\n");
    f.write("new 名 file.txt", "new\n");
    f.write("new-empty.txt", "");
    f.write("ignored.txt", "ignored\n");
    f.git(&["add", "crlf.txt"]);
    let index_before = fs::read(f.0.join(".git/index")).unwrap();
    success(f.prepare(&[
        "--output",
        "change.proposal.json",
        "--title",
        "Example",
        "--request",
        "Explain this feature",
    ]));
    let p = f.valid(&f.0.join("change.proposal.json"));
    assert_eq!(p["base_commit"], base);
    assert_eq!(p["title"], "Example");
    assert_eq!(p["request"], "Explain this feature");
    let notes = p["annotations"].as_array().unwrap();
    assert_eq!(
        notes
            .iter()
            .filter(|a| a["target"]["path"] == "regions.txt")
            .count(),
        3
    );
    assert!(notes.iter().any(|a| a["operation"] == "insert_before"));
    assert!(notes.iter().any(|a| a["operation"] == "insert_after"));
    assert!(notes.iter().any(|a| a["operation"] == "delete"));
    assert!(
        notes
            .iter()
            .any(|a| a["operation"] == "create_file" && a["target"]["path"] == "new 名 file.txt")
    );
    assert!(
        !notes.iter().any(|a| a["target"]["path"] == "ignored.txt"
            || a["target"]["path"] == "change.proposal.json")
    );
    let crlf = notes
        .iter()
        .find(|a| a["target"]["path"] == "crlf.txt")
        .unwrap();
    assert_eq!(crlf["target"]["context"], "b");
    assert_eq!(fs::read(f.0.join(".git/index")).unwrap(), index_before);
    assert_eq!(
        fs::read_to_string(f.0.join("regions.txt")).unwrap(),
        format!("first\n{}last\n", before.replace("line 20\n", "changed\n"))
    );
    assert!(!f.0.join("change.proposal.review.json").exists());
    assert!(!fs::read_dir(&f.0).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".prepare-")
    }));
}

#[test]
fn prepare_uses_explicit_base_discovers_root_and_keeps_ids_deterministic() {
    let f = Fixture::new();
    f.write("file.txt", "old\n");
    f.commit();
    let base = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    f.write("file.txt", "committed change\n");
    f.commit();
    fs::create_dir(f.0.join("nested")).unwrap();
    let output = f.0.join("review.proposal.json");
    let prepare = || {
        Command::new(env!("CARGO_BIN_EXE_patchwalk"))
            .args(["prepare", "--base", &base, "--output"])
            .arg(&output)
            .current_dir(f.0.join("nested"))
            .output()
            .unwrap()
    };
    success(prepare());
    let first = f.valid(&output);
    assert_eq!(first["base_commit"], base);
    assert_eq!(first["annotations"][0]["target"]["context"], "old");
    fs::remove_file(&output).unwrap();
    success(prepare());
    assert_eq!(first, f.valid(&output));
}

#[test]
fn prepare_accepts_commit_endpoints_and_ranges_with_the_same_pinned_diff() {
    let f = Fixture::new();
    f.write("changed.txt", "base\n");
    f.write("removed.txt", "remove this\n");
    f.commit();
    let base = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    f.write("transient.txt", "intermediate only\n");
    f.commit();
    fs::remove_file(f.0.join("transient.txt")).unwrap();
    fs::remove_file(f.0.join("removed.txt")).unwrap();
    f.write("changed.txt", "committed change\n");
    f.write("added.txt", "committed addition\n");
    f.commit();
    let head = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    f.git(&["tag", "review-head"]);

    f.write("changed.txt", "staged change\n");
    f.git(&["add", "changed.txt"]);
    f.write("changed.txt", "working change\n");
    f.write("untracked.bin", "a\0b");
    let index = fs::read(f.0.join(".git/index")).unwrap();
    let output = f.0.join("range.proposal.json");
    success(f.prepare(&[
        "--base",
        "HEAD~2",
        "--head",
        "review-head",
        "--output",
        "range.proposal.json",
    ]));
    let proposal = f.valid(&output);
    assert_eq!(proposal["base_commit"], base);
    assert_eq!(proposal["implementation"]["head_commit"], head);
    let notes = proposal["annotations"].as_array().unwrap();
    let changes: Vec<_> = notes
        .iter()
        .map(|note| {
            (
                note["target"]["path"].as_str().unwrap(),
                note["operation"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            ("added.txt", "create_file"),
            ("changed.txt", "replace"),
            ("removed.txt", "delete")
        ]
    );
    assert_eq!(notes[1]["target"]["context"], "base");
    assert_eq!(fs::read(f.0.join(".git/index")).unwrap(), index);
    assert_eq!(
        fs::read_to_string(f.0.join("changed.txt")).unwrap(),
        "working change\n"
    );

    fs::remove_file(&output).unwrap();
    success(f.prepare(&[
        "--range",
        "HEAD~2..review-head",
        "--output",
        "range.proposal.json",
    ]));
    assert_eq!(proposal, f.valid(&output));
    f.commit();
    f.git(&["tag", "-f", "review-head"]);
    assert_eq!(proposal, f.valid(&output));
}

#[test]
fn prepare_rejects_invalid_or_empty_commit_ranges_without_publishing() {
    let f = Fixture::new();
    f.write("file.txt", "base\n");
    f.commit();
    for (args, error) in [
        (vec!["--range", "HEAD...HEAD"], "BASE..HEAD"),
        (vec!["--range", "HEAD.."], "BASE..HEAD"),
        (vec!["--range", "..HEAD"], "BASE..HEAD"),
        (
            vec!["--range", "HEAD..HEAD", "--base", "HEAD"],
            "cannot be combined",
        ),
        (
            vec!["--range", "HEAD..HEAD", "--head", "HEAD"],
            "cannot be combined",
        ),
        (vec!["--head", "HEAD"], "requires --base"),
        (
            vec!["--range", "missing..HEAD"],
            "Base commit is unavailable",
        ),
        (
            vec!["--range", "HEAD..missing"],
            "Head commit is unavailable",
        ),
        (vec!["--range", "HEAD..HEAD"], "No changed files"),
    ] {
        let mut command = vec!["--output", "out.json"];
        command.extend(args);
        let result = f.prepare(&command);
        assert!(!result.status.success());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains(error), "{stderr}");
        assert!(!f.0.join("out.json").exists());
    }
}

#[test]
fn prepare_refuses_overwrites_clean_repos_and_unsupported_files_without_artifacts() {
    let f = Fixture::new();
    f.write("file.txt", "old\n");
    f.commit();
    assert!(!f.prepare(&["--output", "out.json"]).status.success());
    assert!(!f.0.join("out.json").exists());
    f.write("file.txt", "changed\n");
    f.write("out.json", "preserve proposal");
    assert!(!f.prepare(&["--output", "out.json"]).status.success());
    assert_eq!(
        fs::read_to_string(f.0.join("out.json")).unwrap(),
        "preserve proposal"
    );
    fs::remove_file(f.0.join("out.json")).unwrap();
    f.write("out.review.json", "preserve feedback");
    assert!(!f.prepare(&["--output", "out.json"]).status.success());
    assert!(!f.0.join("out.json").exists());
    assert_eq!(
        fs::read_to_string(f.0.join("out.review.json")).unwrap(),
        "preserve feedback"
    );
    fs::remove_file(f.0.join("out.review.json")).unwrap();
    f.write("binary.bin", "a\0b");
    assert!(!f.prepare(&["--output", "out.json"]).status.success());
    assert!(!f.0.join("out.json").exists());
    assert!(!fs::read_dir(&f.0).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".prepare-")
    }));
}

#[cfg(unix)]
#[test]
fn prepare_handles_mode_only_changes_and_refuses_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let f = Fixture::new();
    f.write("mode.txt", "text\n");
    f.write("empty-mode.txt", "");
    f.commit();
    fs::set_permissions(f.0.join("mode.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(
        f.0.join("empty-mode.txt"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    success(f.prepare(&["--output", "out.json"]));
    assert_eq!(
        f.valid(&f.0.join("out.json"))["annotations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    fs::remove_file(f.0.join("out.json")).unwrap();
    symlink("mode.txt", f.0.join("link.txt")).unwrap();
    assert!(!f.prepare(&["--output", "out.json"]).status.success());
    assert!(!f.0.join("out.json").exists());
}
