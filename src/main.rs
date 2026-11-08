mod app;
mod highlight;
mod model;
mod protocol;
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
        eprintln!("proposal-nav: {error}");
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
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut repository = None;
    let mut check = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "Usage: proposal-nav [proposal.json] [--repo DIRECTORY] [--check]\n\nWithout a path, opens the bundled demo.\n--repo selects the source root; defaults to the proposal's directory.\n--check validates the v1 schema, IDs, paths, ranges, and source context without opening the UI.\nIn the UI, press ? for navigation help. q saves feedback beside the proposal."
                );
                return Ok(());
            }
            "--check" => check = true,
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
    let path = path
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo/proposal.json"));
    let project = if repository.is_some() {
        Project::load_with_root(&path, repository.as_deref())?
    } else {
        Project::load(&path)?
    };
    if check {
        println!(
            "Valid proposal v1: {}\nID: {} · revision {}\n{} annotations across {} loaded files\nBase commit: {}\nFeedback: {}",
            project.proposal.title,
            project.proposal.id,
            project.proposal.revision,
            project.proposal.annotations.len(),
            project.files.len(),
            project
                .proposal
                .base_commit
                .as_deref()
                .unwrap_or("not recorded"),
            project.state_path.display()
        );
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("An interactive terminal is required. Run in Terminal, or use --check.".into());
    }
    let mut app = App::new(project);
    ratatui::run(|terminal| -> io::Result<()> {
        crossterm::execute!(io::stdout(), EnableBracketedPaste)?;
        let _paste_mode = PasteMode;
        while !app.quit {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
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
