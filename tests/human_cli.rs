use std::process::Command;

#[test]
fn portable_binary_validates_bundled_demo_from_an_unrelated_directory() {
    let root = std::env::temp_dir().join(format!("patchwalk-portable-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join(format!("viewer{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_patchwalk"), &binary).unwrap();
    let output = Command::new(&binary)
        .args(["--demo", "--check"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary = String::from_utf8(output.stdout).unwrap();
    assert!(summary.contains("5 annotations across 5 loaded files"));
    let feedback = summary.split("Feedback: ").nth(1).unwrap().trim();
    assert!(!std::path::Path::new(feedback).parent().unwrap().exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn agent_guides_supply_parseable_schemas_and_reject_ambiguous_modes() {
    for (mode, version) in [("planned", 1), ("implemented", 2)] {
        let output = Command::new(env!("CARGO_BIN_EXE_patchwalk"))
            .args(["--agent-guide", mode])
            .output()
            .unwrap();
        assert!(output.status.success());
        let guide = String::from_utf8(output.stdout).unwrap();
        let schema: serde_json::Value =
            serde_json::from_str(guide.split_once("--- JSON schema ---\n").unwrap().1).unwrap();
        assert_eq!(schema["properties"]["version"]["const"], version);
    }
    for args in [
        vec!["--agent-guide"],
        vec!["--agent-guide", "unknown"],
        vec!["--agent-guide", "planned", "--check"],
        vec!["--demo", "missing.json"],
        vec!["--demo", "--repo", "."],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_patchwalk"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}

#[test]
fn help_only_advertises_human_viewer_and_validation() {
    let output = Command::new(env!("CARGO_BIN_EXE_patchwalk"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--check"));
    assert!(!help.contains("review COMMAND"));
    assert!(!help.contains("headless"));
}

#[test]
fn removed_review_commands_fail_without_loading_or_writing_history() {
    for command in [
        "start", "context", "status", "comments", "submit", "comment", "reply", "resolve", "--help",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_patchwalk"))
            .args(["review", command])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("removed"));
    }
}
