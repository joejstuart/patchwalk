mod app;
mod bundled;
mod git_baseline;
mod highlight;
mod model;
mod prepare;
mod protocol;
mod review;
mod ui;

use app::App;
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind};
use model::{Project, Result};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

fn main() {
    if let Err(error) = start() {
        eprintln!("patchwalk: {error}");
        std::process::exit(1);
    }
}

struct PasteMode;

impl Drop for PasteMode {
    fn drop(&mut self) {
        let _ = crossterm::execute!(io::stdout(), DisableBracketedPaste);
    }
}

fn start() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().is_some_and(|arg| arg == "prepare") {
        return prepare::run(raw.into_iter().skip(1));
    }
    if raw.first().is_some_and(|arg| arg == "review") {
        return Err(
            "Headless review commands have been removed. Open a proposal in the terminal viewer."
                .into(),
        );
    }
    let mut args = raw.into_iter();
    let mut path = None;
    let mut repository = None;
    let mut check = false;
    let mut demo_requested = false;
    let mut guide = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "Patchwalk — review agent-generated changes in your terminal\n\nUsage: patchwalk [proposal.json] [--repo DIRECTORY] [--check]\n       patchwalk prepare --output FILE [--repo DIRECTORY]\n                        [--base REVISION [--head REVISION] | --range BASE..HEAD]\n       patchwalk --demo\n       patchwalk --agent-guide planned|implemented\n       patchwalk --version\n\nWithout a path, opens a disposable bundled demo (demo feedback is not retained).\n--repo selects the source root; defaults to the proposal's directory.\n--check validates the v1/v2 schema, IDs, paths, ranges, and source context without opening the UI.\n--agent-guide prints generation instructions and the JSON schema for an agent.\nprepare generates a validated draft with automatic diff anchors; see prepare --help.\n\nTry it: patchwalk --demo\nReview: patchwalk change.proposal.json --repo /path/to/project\nValidate: patchwalk change.proposal.json --repo /path/to/project --check\n\nIn the UI: Enter opens a change, w explains it, c comments, R reloads, ? helps.\nq saves feedback beside the proposal and quits. Ctrl+C quits without saving."
                );
                return Ok(());
            }
            "--check" => check = true,
            "--demo" => demo_requested = true,
            "--version" | "-V" => {
                println!("patchwalk {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--agent-guide" => {
                guide = Some(
                    args.next()
                        .ok_or("--agent-guide requires planned or implemented")?,
                );
            }
            "--repo" => {
                repository = Some(PathBuf::from(
                    args.next().ok_or("--repo requires a directory")?,
                ))
            }
            _ if arg.starts_with('-') => return Err(format!("Unknown option: {arg}").into()),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err("Expected only one proposal path".into()),
        }
    }
    if let Some(mode) = guide {
        if path.is_some() || repository.is_some() || check || demo_requested {
            return Err(
                "--agent-guide cannot be combined with viewer or validation arguments".into(),
            );
        }
        return bundled::agent_guide(&mode);
    }
    if demo_requested && (path.is_some() || repository.is_some()) {
        return Err("--demo cannot be combined with a proposal path or --repo".into());
    }
    let demo = if path.is_none() {
        Some(bundled::Demo::create()?)
    } else {
        None
    };
    let path = path.unwrap_or_else(|| demo.as_ref().unwrap().0.join("proposal.json"));
    let project = if repository.is_some() {
        Project::load_with_root(&path, repository.as_deref())?
    } else {
        Project::load(&path)?
    };
    if check {
        println!(
            "Valid proposal v{}: {}\nID: {} · revision {}\n{} annotations across {} loaded files\nBase commit: {}",
            project.proposal.version,
            project.proposal.title,
            project.proposal.id,
            project.proposal.revision,
            project.proposal.annotations.len(),
            project.files.len(),
            project
                .proposal
                .base_commit
                .as_deref()
                .unwrap_or("not recorded")
        );
        if let Some(head) = &project.proposal.head_commit {
            println!("Head commit: {head}");
        }
        println!("Feedback: {}", project.state_path.display());
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("An interactive terminal is required. Run in Terminal, or use --check.".into());
    }
    let mut app = App::new(project);
    if demo.is_some() {
        app.notify("Demo · explore with Enter, w, c and ? · demo feedback is temporary".into());
    }
    ratatui::run(|terminal| -> io::Result<()> {
        crossterm::execute!(io::stdout(), EnableBracketedPaste)?;
        let _paste_mode = PasteMode;
        let mut refreshed = std::time::Instant::now();
        while !app.quit {
            if refreshed.elapsed() >= std::time::Duration::from_secs(2) {
                app.refresh_review();
                refreshed = std::time::Instant::now();
            }
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
            if !event::poll(std::time::Duration::from_millis(250))? {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.key(key),
                Event::Paste(text) => app.paste(&text),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        Ok(())
    })?;
    Ok(())
}
