use crate::{
    app::{App, Focus, Popup},
    model::{Related, Status},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Padding,
        Paragraph, Wrap,
    },
};
use std::path::Path;

// Reset delegates both the canvas and normal text to the terminal theme.
const BG: Color = Color::Reset;
const BORDER: Color = Color::Reset;
const MUTED: Color = Color::DarkGray;
const TEXT: Color = Color::Reset;
const BRIGHT: Color = Color::Reset;
const ACCENT: Color = Color::Cyan;
const WARM: Color = Color::Yellow;
const GREEN: Color = Color::Green;
// Muted diff rows, with stronger backgrounds only on changed characters.
const DIFF_ADDED: Color = Color::Rgb(158, 203, 174);
const DIFF_ADDED_BG: Color = Color::Rgb(38, 61, 50); // #263D32
const DIFF_ADDED_EMPHASIS: Color = Color::Rgb(56, 108, 76);
const DIFF_REMOVED: Color = Color::Rgb(225, 161, 170);
const DIFF_REMOVED_BG: Color = Color::Rgb(73, 42, 50); // #492A32
const DIFF_REMOVED_EMPHASIS: Color = Color::Rgb(132, 64, 79);
const DIFF_TEXT: Color = Color::Rgb(220, 227, 234);
const DIFF_GUTTER: Color = Color::Rgb(145, 157, 169);

struct Row {
    text: Line<'static>,
    source: Option<usize>,
    annotation: Option<usize>,
}

fn span(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(color))
}

fn strong(text: impl Into<String>, color: Color) -> Span<'static> {
    span(text, color).style(Style::default().fg(color).add_modifier(Modifier::BOLD))
}

fn simple(text: impl Into<String>, color: Color) -> Line<'static> {
    Line::from(span(text, color))
}

// Muted syntax palette. Plain source
// and all backgrounds still inherit the terminal theme.
fn syntax_color(r: u8, g: u8, b: u8) -> Color {
    match (r, g, b) {
        (101, 115, 126) => MUTED,
        (180, 142, 173) => Color::Rgb(185, 164, 211),
        (142, 183, 212) | (143, 161, 179) => Color::Rgb(146, 184, 215),
        (143, 188, 187) | (150, 181, 180) => Color::Rgb(136, 193, 192),
        (163, 190, 140) => Color::Rgb(201, 181, 139),
        (208, 135, 112) | (235, 203, 139) => Color::Rgb(204, 169, 135),
        _ => Color::Reset,
    }
}

fn comment_count(count: usize) -> String {
    format!("{count} comment{}", if count == 1 { "" } else { "s" })
}

fn filename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

type Binding = (&'static str, &'static str);

/// One shared grid for both command rows; narrow views omit whole secondary
/// commands instead of clipping their labels. The full bindings remain in help.
fn footer_bindings(app: &App, columns: usize) -> (Vec<Binding>, Vec<Binding>) {
    if let Some(draft) = &app.comment {
        let top = if draft.editing {
            vec![
                (
                    "Enter",
                    if draft.edit_index.is_some() {
                        "Update"
                    } else {
                        "Add"
                    },
                ),
                ("Tab", "Read"),
                ("Esc", "Cancel"),
            ]
        } else {
            vec![("Tab", "Write"), ("w", "Reason"), ("Esc", "Cancel")]
        };
        return (
            top,
            vec![
                ("←→", if draft.editing { "Edit" } else { "Pan" }),
                ("↑↓", "Scroll"),
                ("PgUp/Dn", "Page"),
            ],
        );
    }
    if app.view.overview {
        let top = vec![
            ("Enter", "Open"),
            ("↑↓", "Select"),
            ("Tab", "Pane"),
            ("c/e", "Comments"),
        ];
        let bottom = if columns < 4 {
            vec![("a", "Approve"), ("?", "Help"), ("q", "Quit")]
        } else {
            vec![
                ("a", "Approve"),
                ("s", "Save"),
                ("?", "Help"),
                ("q", "Quit"),
            ]
        };
        return (top, bottom);
    }
    let top = vec![
        ("n/p", "Notes"),
        ("Tab", "Pane"),
        ("w", "Reason"),
        ("r", "Related"),
        ("Esc", "Back"),
    ];
    let bottom = match columns {
        1..=3 => vec![("c/e", "Comments"), ("?", "Help"), ("q", "Quit")],
        4 => vec![
            ("c/e", "Comments"),
            ("v", "Review"),
            ("?", "Help"),
            ("q", "Quit"),
        ],
        _ => vec![
            ("c/e", "Comments"),
            ("v", "Review"),
            ("f", "Flag"),
            ("?", "Help"),
            ("q", "Quit"),
        ],
    };
    (top, bottom)
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    const MIN_COLUMN_WIDTH: u16 = 16;
    const STATUS_WIDTH: u16 = 14;
    let command_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(STATUS_WIDTH + 2),
        area.height,
    );
    let max_columns = if app.comment.is_some() {
        3
    } else if app.view.overview {
        4
    } else {
        5
    };
    let columns = (command_area.width / MIN_COLUMN_WIDTH)
        .max(1)
        .min(max_columns);
    let (top, bottom) = footer_bindings(app, columns as usize);
    let key_width = [&top, &bottom]
        .into_iter()
        .flat_map(|bindings| bindings.iter().take(columns as usize))
        .map(|(label, _)| Line::from(*label).width())
        .max()
        .unwrap_or(0)
        .max(4);
    for (row, bindings) in [top, bottom].iter().enumerate() {
        for (column, (label, action)) in bindings.iter().take(columns as usize).enumerate() {
            let start = command_area.width * column as u16 / columns;
            let end = command_area.width * (column as u16 + 1) / columns;
            let padding = key_width.saturating_sub(Line::from(*label).width());
            let line = Line::from(vec![
                strong(*label, ACCENT),
                span(" ".repeat(padding + 1), TEXT),
                span(*action, TEXT),
            ]);
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(command_area.x + start, area.y + row as u16, end - start, 1),
            );
        }
    }
    let mode = if let Some(draft) = &app.comment {
        if draft.editing { "COMMENT" } else { "CONTEXT" }
    } else if app.view.focus == Focus::Outline {
        "OUTLINE"
    } else if app.view.overview {
        "OVERVIEW"
    } else {
        "CODE"
    };
    let (status, color) = if app.comment.is_some() {
        ("● Draft", WARM)
    } else if app.dirty {
        ("● Unsaved", WARM)
    } else if app.review.stale {
        ("Review stale", WARM)
    } else if app.review.approved {
        ("✓ Approved", GREEN)
    } else {
        ("Local review", MUTED)
    };
    let divider_x = area.right().saturating_sub(STATUS_WIDTH);
    frame.render_widget(
        Paragraph::new(vec![simple("│", BORDER), simple("│", BORDER)]),
        Rect::new(divider_x, area.y, 1, area.height),
    );
    frame.render_widget(
        Paragraph::new(vec![simple(mode, ACCENT), simple(status, color)]),
        Rect::new(divider_x + 2, area.y, STATUS_WIDTH - 2, area.height),
    );
}

/// A full-width annotation surface, aligned with the source text after its gutter.
fn card_row(rows: &mut Vec<Row>, content: Vec<Span<'static>>, width: usize, anchor: Option<usize>) {
    let mut spans = vec![
        Span::styled("       ", Style::default().bg(BG)),
        Span::styled("│  ", Style::default().fg(WARM).bg(BG)),
    ];
    spans.extend(content.into_iter().map(|mut s| {
        s.style = s.style.bg(BG);
        s
    }));
    let used = Line::from(spans.clone()).width();
    spans.push(Span::styled(
        format!("{}│", " ".repeat(width.saturating_sub(used + 1))),
        Style::default().fg(WARM).bg(BG),
    ));
    rows.push(Row {
        text: Line::from(spans),
        source: None,
        annotation: anchor,
    });
}

fn card_edge(rows: &mut Vec<Row>, label: &str, width: usize, anchor: Option<usize>, bottom: bool) {
    let left = if bottom { "       ╰" } else { "       ╭" };
    let right = if bottom { "╯" } else { "╮" };
    let heading = if label.is_empty() {
        String::new()
    } else {
        format!("─ {label} ")
    };
    let used = Line::from(format!("{left}{heading}{right}")).width();
    rows.push(Row {
        text: simple(
            format!(
                "{left}{heading}{}{right}",
                "─".repeat(width.saturating_sub(used))
            ),
            WARM,
        ),
        source: None,
        annotation: anchor,
    });
}

fn card_text(rows: &mut Vec<Row>, text: &str, width: usize, color: Color) {
    for line in textwrap::wrap(text, width.saturating_sub(12).max(8)) {
        card_row(rows, vec![span(line.into_owned(), color)], width, None);
    }
}

fn card_heading(rows: &mut Vec<Row>, text: &str, width: usize) {
    card_row(rows, vec![], width, None);
    card_row(rows, vec![strong(text, WARM)], width, None);
}

fn annotation_rows(app: &App, index: usize, width: usize, rows: &mut Vec<Row>) {
    let note = &app.project.proposal.annotations[index];
    let selected = !app.view.evidence && index == app.view.selected;
    let compact = app.page_height < 18;
    if !selected {
        rows.push(Row {
            text: Line::from(vec![
                span("       ┊  ", BORDER),
                span(format!("{:02}  ", index + 1), WARM),
                span(note.title.clone(), MUTED),
            ]),
            source: None,
            annotation: Some(index),
        });
        return;
    }
    rows.push(Row {
        text: Line::from(""),
        source: None,
        annotation: None,
    });
    let location = if note.kind == "CREATE FILE" {
        "new file".into()
    } else if note.end_line > note.line {
        format!("L{}–L{}", note.line, note.end_line)
    } else {
        format!("L{}", note.line)
    };
    card_edge(
        rows,
        &format!("{:02} {} · {location}", index + 1, note.kind),
        width,
        Some(index),
        false,
    );
    let count = app.review.notes[index].comments.len();
    if !compact {
        card_text(
            rows,
            &format!(
                "{} · {}",
                comment_count(count),
                if count == 0 { "c add" } else { "e view/edit" }
            ),
            width,
            ACCENT,
        );
    }
    for title in textwrap::wrap(&note.title, width.saturating_sub(12).max(8)) {
        card_row(rows, vec![strong(title.into_owned(), BRIGHT)], width, None);
    }
    if !compact {
        card_row(rows, vec![], width, None);
    }
    card_text(rows, &note.intent, width, TEXT);
    if app.view.expanded {
        card_heading(rows, "WHY HERE", width);
        card_text(rows, &note.why, width, TEXT);
        card_heading(rows, "ALTERNATIVE", width);
        card_text(rows, &note.alternative, width, TEXT);
        card_heading(rows, "RELATED CODE", width);
        for link in &note.related {
            card_text(rows, &format!("↗ {}", link.label()), width, ACCENT);
        }
    }
    for comment in &app.review.notes[index].comments {
        card_heading(rows, "REVIEW DISCUSSION", width);
        card_text(rows, comment, width, BRIGHT);
    }

    if !compact {
        card_row(rows, vec![], width, None);
    }
    let hint = if compact {
        format!(
            "{} · {}",
            comment_count(count),
            if count == 0 { "c add" } else { "e edit" }
        )
    } else if app.view.expanded {
        "w collapse  ·  r explore code".into()
    } else {
        "w why here  ·  r explore code".into()
    };
    card_text(rows, &hint, width, MUTED);
    if !compact {
        card_row(rows, vec![], width, None);
    }
    card_edge(rows, "", width, None, true);
    rows.push(Row {
        text: Line::from(""),
        source: None,
        annotation: None,
    });
}

fn diff_annotation(app: &App, id: usize, width: usize, rows: &mut Vec<Row>) {
    let start = rows.len();
    annotation_rows(app, id, width.saturating_sub(5), rows);
    for row in &mut rows[start..] {
        row.text.spans.insert(0, span("     ", TEXT));
    }
}

fn diff_rows(app: &App, width: usize) -> Vec<Row> {
    use crate::model::DiffKind;
    let diff = &app.project.diffs[&app.view.file];
    let notes: Vec<_> = app
        .project
        .proposal
        .annotations
        .iter()
        .enumerate()
        .filter(|(_, note)| note.file == app.view.file)
        .collect();
    let mut rows = Vec::new();
    let mut emitted = std::collections::HashSet::new();
    let mut old_cursor = 1;
    for line in &diff.lines {
        let anchor = line.before.unwrap_or(old_cursor);
        for (id, note) in &notes {
            if !emitted.contains(id)
                && (note.kind == "CREATE FILE" || (note.kind != "ADD AFTER" && note.line == anchor))
            {
                diff_annotation(app, *id, width, &mut rows);
                emitted.insert(*id);
            }
        }
        let (marker, color) = match line.kind {
            DiffKind::Context => (' ', TEXT),
            DiffKind::Added => ('+', DIFF_ADDED),
            DiffKind::Removed => ('-', DIFF_REMOVED),
        };
        let old = line.before.map(|n| n.to_string()).unwrap_or_default();
        let new = line.after.map(|n| n.to_string()).unwrap_or_default();
        let mut spans = vec![
            Span::styled(
                format!("{old:>4}"),
                Style::default()
                    .fg(if marker == '-' { color } else { MUTED })
                    .add_modifier(if marker == '-' {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            span(" ", MUTED),
            Span::styled(
                format!("{new:>4}"),
                Style::default()
                    .fg(if marker == '+' { color } else { MUTED })
                    .add_modifier(if marker == '+' {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
            strong(format!(" {marker} "), color),
        ];
        let cache = if line.kind == DiffKind::Removed {
            &app.highlights.files
        } else {
            &app.highlights.after_files
        };
        let number = if line.kind == DiffKind::Removed {
            line.before
        } else {
            line.after
        };
        let tokens =
            number.and_then(|n| cache.get(&app.view.file).and_then(|lines| lines.get(n - 1)));
        let (background, emphasis_bg, emphasis_fg) = match line.kind {
            DiffKind::Context => (BG, BG, TEXT),
            DiffKind::Added => (
                DIFF_ADDED_BG,
                DIFF_ADDED_EMPHASIS,
                Color::Rgb(217, 255, 225),
            ),
            DiffKind::Removed => (
                DIFF_REMOVED_BG,
                DIFF_REMOVED_EMPHASIS,
                Color::Rgb(255, 224, 229),
            ),
        };
        for gutter in &mut spans {
            gutter.style = gutter.style.bg(background);
            if marker != ' ' && gutter.style.fg == Some(MUTED) {
                gutter.style = gutter.style.fg(DIFF_GUTTER);
            }
        }
        let fallback;
        let tokens = if let Some(tokens) = tokens {
            tokens.as_slice()
        } else {
            fallback = vec![(syntect::highlighting::Style::default(), line.text.clone())];
            fallback.as_slice()
        };
        let (mut offset, mut range_index) = (0usize, 0usize);
        for (style, text) in tokens {
            let c = style.foreground;
            let syntax = syntax_color(c.r, c.g, c.b);
            let foreground = if syntax == Color::Reset && marker != ' ' {
                DIFF_TEXT
            } else {
                syntax
            };
            for ch in text.chars() {
                while range_index < line.emphasized.len()
                    && line.emphasized[range_index].end <= offset
                {
                    range_index += 1;
                }
                let highlighted = line
                    .emphasized
                    .get(range_index)
                    .is_some_and(|range| range.contains(&offset));
                let paint = if highlighted {
                    Style::default()
                        .fg(emphasis_fg)
                        .bg(emphasis_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(foreground).bg(background)
                };
                if offset >= app.view.horizontal as usize {
                    if let Some(last) = spans.last_mut().filter(|last| last.style == paint) {
                        last.content.to_mut().push(ch);
                    } else {
                        spans.push(Span::styled(ch.to_string(), paint));
                    }
                }
                offset += 1;
            }
        }
        let used = Line::from(spans.clone()).width();
        spans.push(Span::styled(
            " ".repeat(width.saturating_sub(used)),
            Style::default().bg(background),
        ));
        rows.push(Row {
            text: Line::from(spans).style(Style::default().bg(background)),
            source: line.before,
            annotation: None,
        });
        if line.missing_newline && line.kind != DiffKind::Context {
            rows.push(Row {
                text: simple("            \\ No newline at end of file", MUTED),
                source: None,
                annotation: None,
            });
        }
        if let Some(old_line) = line.before {
            old_cursor = old_line + 1;
            for (id, note) in &notes {
                if !emitted.contains(id) && note.kind == "ADD AFTER" && note.line == old_line {
                    diff_annotation(app, *id, width, &mut rows);
                    emitted.insert(*id);
                }
            }
        }
    }
    for (id, _) in notes {
        if !emitted.contains(&id) {
            diff_annotation(app, id, width, &mut rows);
        }
    }
    rows
}

fn code_rows(app: &App, width: usize) -> Vec<Row> {
    if !app.view.evidence && app.project.diffs.contains_key(&app.view.file) {
        return diff_rows(app, width);
    }
    let source = &app.project.files[&app.view.file];
    let notes = &app.project.proposal.annotations;
    let mut rows = Vec::new();
    for (index, _code) in source.iter().enumerate() {
        let line = index + 1;
        for (id, note) in notes.iter().enumerate() {
            if note.file == app.view.file && note.line == line && note.kind != "ADD AFTER" {
                annotation_rows(app, id, width, &mut rows);
            }
        }
        let selected = &app.project.proposal.annotations[app.view.selected];
        let is_target = if !app.view.evidence && selected.file == app.view.file {
            (selected.line..=selected.end_line).contains(&line)
        } else {
            line == app.view.target_line
        };
        let background = BG;
        let marker = if is_target { "›" } else { " " };
        let mut spans = vec![Span::styled(
            format!(" {marker}{line:>3}  "),
            Style::default()
                .fg(if is_target { ACCENT } else { MUTED })
                .bg(background),
        )];
        let mut skip = app.view.horizontal as usize;
        for (style, text) in &app.highlights.files[&app.view.file][index] {
            let count = text.chars().count();
            if skip >= count {
                skip -= count;
                continue;
            }
            let visible: String = text.chars().skip(skip).collect();
            skip = 0;
            let c = style.foreground;
            spans.push(Span::styled(
                visible,
                Style::default()
                    .fg(syntax_color(c.r, c.g, c.b))
                    .bg(background),
            ));
        }
        rows.push(Row {
            text: Line::from(spans).style(Style::default().bg(background)),
            source: Some(line),
            annotation: None,
        });
        for (id, note) in notes.iter().enumerate() {
            if note.file == app.view.file && note.line == line && note.kind == "ADD AFTER" {
                annotation_rows(app, id, width, &mut rows);
            }
        }
    }
    rows
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        area,
    );
    if area.width < 65 || area.height < 16 {
        frame.render_widget(Paragraph::new("Patchwalk\n\nEnlarge the terminal to at least 65 columns × 16 rows.\nPress q to quit.")
            .style(Style::default().fg(TEXT).bg(BG)).wrap(Wrap { trim: false }), area);
        return;
    }
    let compact = area.height < 28;
    let vertical = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .split(area);
    draw_header(frame, app, vertical[0]);
    let sidebar_width = if area.width >= 120 {
        32
    } else if area.width >= 95 {
        29
    } else {
        24
    };
    let panes = Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(30)])
        .split(vertical[1]);
    draw_outline(frame, app, panes[0], compact);
    if app.view.overview {
        draw_overview(frame, app, panes[1]);
    } else {
        draw_code(frame, app, panes[1]);
    }

    // A quiet status line keeps application messages separate from navigation.
    let footer = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(BORDER));
    frame.render_widget(footer, vertical[2]);
    draw_footer(frame, app, vertical[3]);
    if app.popup.is_some() {
        draw_popup(frame, app);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let inner = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );
    let columns = Layout::horizontal([Constraint::Min(10), Constraint::Length(22)])
        .split(Rect { height: 1, ..inner });
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            strong("◈  PATCHWALK", ACCENT),
            span("   /   REVIEW", MUTED),
        ])),
        columns[0],
    );
    let state = if app.review.stale {
        "PREVIOUS REVIEW STALE"
    } else if app.review.approved {
        "✓ REVIEW ACCEPTED"
    } else if !app.project.diffs.is_empty() {
        "IMPLEMENTED CHANGE"
    } else {
        "SAVED PROPOSAL"
    };
    frame.render_widget(
        Paragraph::new(simple(
            state,
            if app.review.approved { GREEN } else { MUTED },
        ))
        .right_aligned(),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(strong(
            app.project.proposal.title.clone(),
            BRIGHT,
        ))),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
}

fn draw_outline(frame: &mut Frame, app: &App, area: Rect, compact: bool) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(BORDER))
        .padding(Padding::new(1, 1, 0, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let sections = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(if compact { 2 } else { 3 }),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            strong(
                " PROPOSAL",
                if app.view.focus == Focus::Outline {
                    ACCENT
                } else {
                    MUTED
                },
            ),
            span(format!("  {:02}", app.review.notes.len()), MUTED),
        ])),
        sections[0],
    );
    let overview_style = Style::default().bg(BG);
    let mut overview = vec![Line::from(vec![
        span(if app.view.overview { "▎ " } else { "  " }, ACCENT),
        strong(
            format!(
                "◎ Overview{}",
                if app.review.overview_comments.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", app.review.overview_comments.len())
                }
            ),
            if app.view.overview { BRIGHT } else { TEXT },
        ),
    ])];
    if !compact {
        overview.push(simple(
            if app.review.overview_comments.is_empty() {
                "    Goal and approach".into()
            } else {
                format!(
                    "    {} · e",
                    comment_count(app.review.overview_comments.len())
                )
            },
            MUTED,
        ));
    }
    frame.render_widget(
        Paragraph::new(overview).style(overview_style),
        Rect {
            height: sections[1].height.saturating_sub(1),
            ..sections[1]
        },
    );
    let items: Vec<ListItem> = app
        .project
        .proposal
        .annotations
        .iter()
        .enumerate()
        .map(|(i, note)| {
            let selected = !app.view.overview && i == app.view.selected;
            let status = match app.review.notes[i].status {
                Status::Pending => " ",
                Status::Reviewed => "✓",
                Status::Flagged => "!",
            };
            let status_color = match app.review.notes[i].status {
                Status::Reviewed => GREEN,
                Status::Flagged => WARM,
                _ => MUTED,
            };
            let available = inner.width.saturating_sub(7).max(6) as usize;
            let mut lines = Vec::new();
            for (row, title) in textwrap::wrap(&note.title, available)
                .into_iter()
                .enumerate()
            {
                let prefix = if row == 0 {
                    format!("{:02} ", i + 1)
                } else {
                    "   ".into()
                };
                let mut spans = vec![
                    span(prefix, if selected { ACCENT } else { MUTED }),
                    if selected {
                        strong(title.into_owned(), BRIGHT)
                    } else {
                        span(title.into_owned(), TEXT)
                    },
                ];
                if row == 0 {
                    spans.push(span(format!(" {status}"), status_color));
                }
                lines.push(Line::from(spans));
            }
            let count = app.review.notes[i].comments.len();
            if count > 0 {
                lines.push(simple(format!("   {} · e", comment_count(count)), ACCENT));
            }
            if !compact {
                let path = filename(&note.file);
                lines.push(simple(format!("   {path}"), MUTED));
            }
            lines.push(Line::from(""));
            ListItem::new(lines)
        })
        .collect();
    let mut state =
        ListState::default().with_selected((!app.view.overview).then_some(app.view.selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("▎ ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        sections[2],
        &mut state,
    );
    let reviewed = app
        .review
        .notes
        .iter()
        .filter(|n| n.status == Status::Reviewed)
        .count();
    let flagged = app
        .review
        .notes
        .iter()
        .filter(|n| n.status == Status::Flagged)
        .count();
    let summary = vec![Line::from(vec![
        span(format!(" ✓ {reviewed} reviewed"), GREEN),
        span(format!("   ! {flagged}"), WARM),
    ])];
    frame.render_widget(Paragraph::new(summary), sections[3]);
}

fn draw_overview(frame: &mut Frame, app: &mut App, area: Rect) {
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);
    let editor_height = app.comment.as_ref().map_or(0, |draft| {
        let (lines, _) = comment_layout(
            &draft.text,
            draft.cursor,
            area.width.saturating_sub(4) as usize,
        );
        (lines.len() + 2).clamp(3, (area.height as usize / 3).clamp(3, 7)) as u16
    });
    let sections = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(editor_height),
        Constraint::Length(1),
    ])
    .split(area);
    let file_count = app
        .project
        .proposal
        .annotations
        .iter()
        .map(|note| &note.file)
        .collect::<std::collections::HashSet<_>>()
        .len();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![span("  ", TEXT), strong("Overview", BRIGHT)]),
            simple(
                format!(
                    "  {} proposed changes across {file_count} files",
                    app.review.notes.len()
                ),
                MUTED,
            ),
        ]),
        sections[0],
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER)),
        Rect::new(sections[0].x, sections[0].y + 2, sections[0].width, 1),
    );
    let content = Rect::new(
        sections[1].x + 2,
        sections[1].y,
        sections[1].width.saturating_sub(4),
        sections[1].height,
    );
    let width = content.width.max(1) as usize;
    let mut lines = Vec::new();
    if !app.review.overview_comments.is_empty() {
        lines.push(simple(
            format!(
                "{} · e view/edit",
                comment_count(app.review.overview_comments.len())
            ),
            ACCENT,
        ));
    }
    for (heading, text) in [
        ("THE GOAL", &app.project.proposal.request),
        ("THE APPROACH", &app.project.proposal.summary),
    ] {
        lines.push(Line::from(strong(heading, WARM)));
        lines.extend(
            textwrap::wrap(text, width)
                .into_iter()
                .map(|s| simple(s.into_owned(), TEXT)),
        );
        lines.push(Line::from(""));
    }
    if app.review.stale {
        lines.push(Line::from(strong(
            "CODE OR PROPOSAL CHANGED · PREVIOUS APPROVALS STALE",
            WARM,
        )));
        lines.push(simple(
            "Previous discussion is preserved below and beside each change.",
            MUTED,
        ));
        lines.push(Line::from(""));
    }
    for comment in &app.review.overview_comments {
        lines.push(Line::from(strong("REVIEW DISCUSSION", WARM)));
        lines.extend(
            textwrap::wrap(comment, width)
                .into_iter()
                .map(|s| simple(s.into_owned(), TEXT)),
        );
        lines.push(Line::from(""));
    }
    lines.push(Line::from(strong("CHANGE SEQUENCE", WARM)));
    lines.push(simple(
        "Select a change to inspect its code and reasoning.",
        MUTED,
    ));
    lines.push(Line::from(""));
    let mut selected_rows = 0..0;
    for (index, note) in app.project.proposal.annotations.iter().enumerate() {
        let start = lines.len();
        let focused = app.view.focus == Focus::Code && index == app.view.selected;
        let background = BG;
        let marker = if focused { "▸" } else { " " };
        let status = match app.review.notes[index].status {
            Status::Pending => "",
            Status::Reviewed => "  ✓ reviewed",
            Status::Flagged => "  ! flagged",
        };
        for (row, title) in textwrap::wrap(
            &format!("{}{status}", note.title),
            width.saturating_sub(5).max(6),
        )
        .into_iter()
        .enumerate()
        {
            let prefix = if row == 0 {
                format!("{marker} {:02} ", index + 1)
            } else {
                "     ".into()
            };
            lines.push(
                Line::from(vec![
                    span(prefix, ACCENT),
                    if focused {
                        strong(title.into_owned(), BRIGHT)
                    } else {
                        span(title.into_owned(), TEXT)
                    },
                ])
                .style(Style::default().bg(background).add_modifier(if focused {
                    Modifier::UNDERLINED
                } else {
                    Modifier::empty()
                })),
            );
        }
        let path = if app.project.new_files.contains(&note.file) {
            format!("{} (new)", note.file)
        } else {
            format!("{}:{}", note.file, note.line)
        };
        for row in textwrap::wrap(&path, width.saturating_sub(5).max(6)) {
            lines.push(simple(format!("     {row}"), MUTED).style(Style::default().bg(background)));
        }
        if index == app.view.selected {
            selected_rows = start..lines.len();
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(strong("ARCHITECTURE", WARM)));
    lines.extend(
        textwrap::wrap(&app.project.proposal.architecture, width)
            .into_iter()
            .map(|s| simple(s.into_owned(), TEXT)),
    );
    if !app.project.proposal.questions.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(strong("OPEN QUESTIONS", WARM)));
        for question in &app.project.proposal.questions {
            lines.extend(
                textwrap::wrap(&format!("• {question}"), width)
                    .into_iter()
                    .map(|s| simple(s.into_owned(), TEXT)),
            );
        }
    }
    app.page_height = content.height as usize;
    app.row_count = lines.len();
    if app.recenter && app.view.focus == Focus::Code {
        if selected_rows.start < app.view.scroll {
            app.view.scroll = selected_rows.start;
        } else if selected_rows.end > app.view.scroll + app.page_height {
            app.view.scroll = selected_rows.end.saturating_sub(app.page_height);
        }
    }
    app.recenter = false;
    app.view.scroll = app
        .view
        .scroll
        .min(lines.len().saturating_sub(app.page_height));
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(app.view.scroll)
                .take(app.page_height)
                .collect::<Vec<_>>(),
        ),
        content,
    );
    let hint = if let Some(notice) = &app.notice {
        format!("  {notice}")
    } else if app.view.scroll + app.page_height < app.row_count {
        "  ↓ More below  ·  Tab + ↑↓ selects a change".into()
    } else {
        "  Enter opens a change  ·  Esc / o returns here".into()
    };
    if app.comment.is_some() {
        draw_comment(frame, app, sections[2]);
    }
    frame.render_widget(Paragraph::new(simple(hint, MUTED)), sections[3]);
}

fn draw_code(frame: &mut Frame, app: &mut App, area: Rect) {
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);
    let editor_height = app.comment.as_ref().map_or(0, |draft| {
        let (lines, _) = comment_layout(
            &draft.text,
            draft.cursor,
            area.width.saturating_sub(4) as usize,
        );
        (lines.len() + 2).clamp(3, (area.height as usize / 3).clamp(3, 7)) as u16
    });
    let sections = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(editor_height),
        Constraint::Length(1),
    ])
    .split(area);
    let tab = sections[0];
    let label_width = if tab.width >= 66 { 23 } else { 15 };
    let title_areas = Layout::horizontal([Constraint::Min(10), Constraint::Length(label_width)])
        .split(Rect { height: 1, ..tab });
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            span("  ", TEXT),
            strong(filename(&app.view.file), BRIGHT),
        ])),
        title_areas[0],
    );
    let applied = !app.view.evidence && app.project.diffs.contains_key(&app.view.file);
    let role = if applied {
        "Applied diff"
    } else if app.view.evidence && app.project.diffs.contains_key(&app.view.file) {
        "Evidence · before"
    } else if app.project.new_files.contains(&app.view.file) {
        "Proposed new file"
    } else if app.view.evidence {
        "↗ Evidence"
    } else {
        "Existing code"
    };
    frame.render_widget(
        Paragraph::new(simple(
            format!("{role}  "),
            if app.view.evidence { ACCENT } else { MUTED },
        ))
        .right_aligned(),
        title_areas[1],
    );
    let legend = if applied {
        Line::from(vec![
            span("  Before / After    ", MUTED),
            strong("− Removed", DIFF_REMOVED),
            span("    ", MUTED),
            strong("+ Added", DIFF_ADDED),
        ])
    } else {
        simple(format!("  {}", app.view.file), MUTED)
    };
    frame.render_widget(
        Paragraph::new(legend),
        Rect::new(tab.x, tab.y + 1, tab.width, 1),
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER)),
        Rect::new(tab.x, tab.y + 2, tab.width, 1),
    );
    let code_area = Rect {
        width: sections[1].width.saturating_sub(2),
        ..sections[1]
    };
    app.page_height = code_area.height as usize;
    let rows = code_rows(app, code_area.width as usize);
    app.row_count = rows.len();
    if app.recenter {
        let anchor = rows
            .iter()
            .position(|r| {
                if app.view.evidence {
                    r.source == Some(app.view.target_line)
                } else {
                    r.annotation == Some(app.view.selected)
                }
            })
            .unwrap_or(0);
        app.view.scroll = anchor.saturating_sub(3);
        app.recenter = false;
    }
    app.view.scroll = app
        .view
        .scroll
        .min(rows.len().saturating_sub(app.page_height));
    let lines: Vec<_> = rows
        .into_iter()
        .skip(app.view.scroll)
        .take(app.page_height)
        .map(|r| r.text)
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG)),
        code_area,
    );
    // A small scroll thumb makes off-screen reasoning discoverable without a heavy frame.
    if app.row_count > app.page_height && code_area.height > 0 {
        let track = code_area.height as usize;
        let thumb = (track * app.page_height / app.row_count).max(1);
        let offset = (track - thumb) * app.view.scroll / (app.row_count - app.page_height).max(1);
        for y in 0..track {
            frame.render_widget(
                Paragraph::new(simple(
                    if y >= offset && y < offset + thumb {
                        "┃"
                    } else {
                        "│"
                    },
                    if y >= offset && y < offset + thumb {
                        MUTED
                    } else {
                        BORDER
                    },
                )),
                Rect::new(area.right() - 1, code_area.y + y as u16, 1, 1),
            );
        }
    }
    if app.comment.is_some() {
        draw_comment(frame, app, sections[2]);
    }
    let location = format!(
        "  {:02} / {:02} annotations  ·  {}",
        app.view.selected + 1,
        app.review.notes.len(),
        if app.view.evidence {
            "Esc returns to proposal"
        } else if applied {
            "Implemented change · review only"
        } else {
            "Source unchanged"
        }
    );
    let info = if let Some(notice) = &app.notice {
        format!("  {notice}")
    } else {
        location
    };
    frame.render_widget(Paragraph::new(simple(info, MUTED)), sections[3]);
}

/// Wrap input by terminal cells rather than bytes; retain a visible insertion point.
fn comment_layout(text: &str, cursor: usize, width: usize) -> (Vec<Line<'static>>, (u16, usize)) {
    let width = width.max(1);
    let mut rows = vec![String::new()];
    let (mut column, mut row) = (0usize, 0usize);
    let mut caret = (0, 0);
    for (offset, c) in text.char_indices() {
        let cells = Line::from(c.to_string()).width();
        if c != '\n' && column + cells > width {
            rows.push(String::new());
            row += 1;
            column = 0;
        }
        if offset == cursor {
            caret = (column as u16, row);
        }
        if c == '\n' {
            rows.push(String::new());
            row += 1;
            column = 0;
        } else {
            rows[row].push(c);
            column += cells;
            if column >= width {
                rows.push(String::new());
                row += 1;
                column = 0;
            }
        }
    }
    if cursor == text.len() {
        caret = (column as u16, row);
    }
    (rows.into_iter().map(Line::from).collect(), caret)
}

fn draw_comment(frame: &mut Frame, app: &App, area: Rect) {
    let Some(draft) = &app.comment else {
        return;
    };
    let note = &app.project.proposal.annotations[draft.annotation];
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(if let Some(index) = draft.edit_index {
            format!(
                " Edit comment {} · {} ",
                index + 1,
                if draft.overview {
                    "Overview".into()
                } else {
                    format!("{:02} {}", draft.annotation + 1, note.title)
                }
            )
        } else if draft.overview {
            " Comment · Overview ".into()
        } else {
            format!(" Comment · {:02} {} ", draft.annotation + 1, note.title)
        })
        .border_style(Style::default().fg(if draft.editing { ACCENT } else { BORDER }))
        .style(Style::default().bg(BG).fg(TEXT));
    // Match the width used when calculating the dock height.
    let area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }
    let (lines, (column, row)) = comment_layout(&draft.text, draft.cursor, inner.width as usize);
    let first_row = row.saturating_sub(inner.height as usize - 1);
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(first_row)
                .take(inner.height as usize)
                .collect::<Vec<_>>(),
        ),
        inner,
    );
    if draft.editing {
        frame.set_cursor_position((
            inner.x + column.min(inner.width - 1),
            inner.y + (row - first_row) as u16,
        ));
    }
}

fn draw_popup(frame: &mut Frame, app: &mut App) {
    let screen = frame.area();
    // Dim the entire workspace while retaining its spatial context.
    let buffer = frame.buffer_mut();
    for cell in &mut buffer.content {
        cell.set_fg(MUTED).set_bg(BG);
    }
    let is_related = matches!(app.popup, Some(Popup::Related(_)));
    let width = screen.width.saturating_sub(8).min(82);
    let max_height = screen.height.saturating_sub(4);
    let desired_height = if is_related {
        7 + app.project.proposal.annotations[app.view.selected]
            .related
            .len() as u16
            * 3
    } else {
        28
    };
    let height = max_height.min(desired_height);
    let area = Rect::new(
        (screen.width - width) / 2,
        (screen.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG).fg(TEXT))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let sections = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let title = match app.popup.as_ref().unwrap() {
        Popup::Related(_) => "Related code",
        Popup::Help => "Keyboard shortcuts",
        Popup::Comments(_) => "Comments · select one to edit",
    };
    frame.render_widget(
        Paragraph::new(Line::from(strong(title, BRIGHT))),
        sections[0],
    );
    let instructions = if matches!(app.popup, Some(Popup::Comments(_))) {
        "↑↓ select    Enter/e edit    Esc close"
    } else if is_related {
        "↑↓ select    Enter follow    Esc close"
    } else {
        "↑↓ scroll    Esc close"
    };
    frame.render_widget(Paragraph::new(simple(instructions, MUTED)), sections[2]);
    if let Some(Popup::Related(selected)) = app.popup.as_ref() {
        let note = &app.project.proposal.annotations[app.view.selected];
        let items: Vec<_> = note
            .related
            .iter()
            .map(|link| {
                let destination = match link {
                    Related::Annotation { annotation, .. } => format!(
                        "Note {:02}  /  {}",
                        annotation + 1,
                        app.project.proposal.annotations[*annotation].file
                    ),
                    Related::Code { file, line, .. } => format!("{file}:{line}"),
                };
                ListItem::new(vec![
                    Line::from(span(link.label(), TEXT)),
                    simple(destination, MUTED),
                    Line::from(""),
                ])
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(*selected));
        frame.render_stateful_widget(
            List::new(items).highlight_symbol("↗ ").highlight_style(
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
            sections[1],
            &mut state,
        );
        return;
    }
    if let Some(Popup::Comments(selected)) = app.popup.as_ref() {
        let items: Vec<_> = app
            .current_comments()
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let mut lines = vec![Line::from(strong(
                    format!(
                        "{} · {}",
                        index + 1,
                        if app.comment_editable(index) {
                            "Your comment · Enter to edit"
                        } else {
                            "Read-only comment"
                        }
                    ),
                    ACCENT,
                ))];
                lines.extend(
                    textwrap::wrap(
                        app.comment_body(index),
                        sections[1].width.saturating_sub(3).max(1) as usize,
                    )
                    .into_iter()
                    .take(4)
                    .map(|line| simple(line.into_owned(), TEXT)),
                );
                lines.push(Line::from(""));
                ListItem::new(lines)
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(*selected));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("▎ ")
                .highlight_style(Style::default().add_modifier(Modifier::BOLD)),
            sections[1],
            &mut state,
        );
        return;
    }
    let mut lines = Vec::new();
    let width = sections[1].width.max(1) as usize;
    match app.popup.as_ref().unwrap() {
        Popup::Help => {
            for (heading, text) in [
                (
                    "MOVE",
                    "n / p        Next / previous annotation\nTab          Switch sidebar / content focus\n↑↓ or j/k    Select sidebar items / overview links, or scroll code\nPgUp/PgDn    Scroll code by a page\nHome / End   Beginning / end of code view\nh/l or ←/→   Scroll source horizontally",
                ),
                (
                    "EXPLORE",
                    "Enter         Open an overview link\nEnter/w/Space Expand / collapse reasoning\nr             Follow related code\nEsc/Backspace Return to previous view, then overview\no             Return directly to overview\nR             Reload proposal and source",
                ),
                (
                    "REVIEW",
                    "v   Toggle reviewed    f   Toggle flag\nc   Add comment        e   View/edit comments\na   Toggle approval\ns   Save feedback      q   Save and quit\nCtrl+C  Quit without saving",
                ),
            ] {
                lines.push(Line::from(strong(heading, WARM)));
                for row in text.lines() {
                    lines.extend(
                        textwrap::wrap(row, width)
                            .into_iter()
                            .map(|s| simple(s.into_owned(), TEXT)),
                    );
                }
                lines.push(Line::from(""));
            }
            lines.extend(textwrap::wrap("Source is read-only. Approval is local metadata; no LLM is connected. Review edits clear approval.", width)
                .into_iter().map(|s| simple(s.into_owned(), MUTED)));
        }
        Popup::Related(_) | Popup::Comments(_) => unreachable!(),
    }
    let max_scroll = lines
        .len()
        .saturating_sub(sections[1].height as usize)
        .min(u16::MAX as usize) as u16;
    app.popup_scroll = app.popup_scroll.min(max_scroll);
    let scroll = app.popup_scroll;
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), sections[1]);
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::Path;

    /// Export actual terminal cells for visual inspection, without opening a TTY.
    #[test]
    #[ignore]
    fn export_visual_previews() {
        let output = std::env::var("PATCHWALK_PREVIEW_DIR").expect("Set PATCHWALK_PREVIEW_DIR");
        std::fs::create_dir_all(&output).unwrap();
        for (name, width, height, expanded, popup) in [
            ("comment", 130, 40, true, None),
            ("comment-compact", 80, 24, false, None),
            ("overview", 110, 35, false, None),
            ("overview-compact", 80, 24, false, None),
            ("review", 110, 35, false, None),
            ("implemented", 150, 45, false, None),
            ("footer-narrow", 65, 24, false, None),
            ("footer-wide", 160, 35, false, None),
            ("compact", 80, 24, false, None),
            ("reasoning", 110, 40, true, None),
            ("related", 110, 35, false, Some(Popup::Related(1))),
        ] {
            let project = if name == "implemented" {
                let Ok(path) = std::env::var("PATCHWALK_PREVIEW_PROPOSAL") else {
                    continue;
                };
                let root = std::env::var("PATCHWALK_PREVIEW_REPO").ok();
                Project::load_with_root(Path::new(&path), root.as_deref().map(Path::new)).unwrap()
            } else {
                Project::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/proposal.json"))
                    .unwrap()
            };
            let mut app = App::new(project);
            // A preview must not include a user's saved review comments.
            for note in &mut app.review.notes {
                note.status = Status::Pending;
                note.comments.clear();
            }
            app.review.approved = false;
            if !name.starts_with("overview") {
                app.select(
                    if name == "implemented" {
                        std::env::var("PATCHWALK_PREVIEW_ANNOTATION")
                            .ok()
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0)
                    } else {
                        1
                    },
                    false,
                );
            }
            app.view.expanded = expanded;
            app.popup = popup;
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            if name.starts_with("comment") {
                app.key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('c'),
                    crossterm::event::KeyModifiers::NONE,
                ));
                app.paste("Should validation live here, or in the domain module?");
                terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            }
            let buffer = terminal.backend().buffer();
            // Preview the same native colors against a sample terminal palette.
            let light = std::env::var("PATCHWALK_PREVIEW_THEME").as_deref() == Ok("light");
            let rgb = |c: Color, background: bool| match c {
                Color::Rgb(r, g, b) => vec![r, g, b],
                Color::Reset if background && light => vec![253, 246, 227],
                Color::Reset if background => vec![0, 43, 54],
                Color::Reset if light => vec![88, 110, 117],
                Color::Reset => vec![131, 148, 150],
                Color::DarkGray if light => vec![131, 148, 150],
                Color::DarkGray => vec![88, 110, 117],
                Color::Cyan => vec![42, 161, 152],
                Color::Blue => vec![38, 139, 210],
                Color::Magenta => vec![211, 54, 130],
                Color::Yellow => vec![181, 137, 0],
                Color::Green => vec![133, 153, 0],
                Color::Red => vec![220, 50, 47],
                _ => vec![131, 148, 150],
            };
            let cells: Vec<_> = buffer
                .content
                .iter()
                .map(|cell| {
                    serde_json::json!({
                        "text": cell.symbol(), "fg": rgb(cell.fg, false), "bg": rgb(cell.bg, true),
                        "bold": cell.modifier.contains(Modifier::BOLD)
                    })
                })
                .collect();
            let json = serde_json::json!({"width": width, "height": height, "cells": cells});
            std::fs::write(
                Path::new(&output).join(format!("{name}.json")),
                json.to_string(),
            )
            .unwrap();
        }
    }

    #[test]
    fn inline_comment_keeps_source_explanation_and_input_visible() {
        for (width, height, expanded) in [(110, 35, true), (80, 24, false)] {
            let project =
                Project::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/proposal.json"))
                    .unwrap();
            let mut app = App::new(project);
            app.select(1, false);
            app.view.expanded = expanded;
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            app.key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('c'),
                crossterm::event::KeyModifiers::NONE,
            ));
            app.paste("Why is this the right boundary?");
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            for expected in [
                "Comment",
                "Why is this the right boundary?",
                "const customer",
                "validateOrder",
            ] {
                assert!(
                    text.contains(expected),
                    "Missing {expected} at {width}×{height}"
                );
            }
            if expanded {
                assert!(text.contains("WHY HERE"));
            }
            assert!(app.popup.is_none());
        }
        let (_, cursor) = comment_layout("ab界🙂", "ab界🙂".len(), 4);
        assert_eq!(cursor, (2, 1));
    }

    #[test]
    fn startup_renders_goal_approach_and_links() {
        let project =
            Project::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/proposal.json"))
                .unwrap();
        let mut app = App::new(project);
        let mut terminal = Terminal::new(TestBackend::new(110, 35)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for expected in [
            "Overview",
            "THE GOAL",
            "THE APPROACH",
            "CHANGE SEQUENCE",
            "Cover rejection behavior",
        ] {
            assert!(text.contains(expected), "Missing {expected}");
        }
    }

    #[test]
    fn fresh_and_reopened_reviews_show_hints_without_hiding_notices() {
        let demo = crate::bundled::Demo::create().unwrap();
        let path = demo.0.join("proposal.json");
        let render = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(110, 35)).unwrap();
            terminal.draw(|frame| draw(frame, app)).unwrap();
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };
        for _ in 0..2 {
            let mut app = App::new(Project::load(&path).unwrap());
            assert!(app.notice.is_none());
            let text = render(&mut app);
            assert!(text.contains("More below") || text.contains("Enter opens a change"));
            app.select(0, false);
            assert!(render(&mut app).contains("Source unchanged"));
            app.notify("Save failed: example error".into());
            assert!(render(&mut app).contains("Save failed: example error"));
            app.project.save_review(&app.review).unwrap();
        }
        std::fs::write(path.with_extension("review.json"), "{broken").unwrap();
        let mut app = App::new(Project::load(&path).unwrap());
        assert!(app.review.load_failed);
        assert!(render(&mut app).contains("Could not load review"));
    }

    #[test]
    fn comment_counts_and_selection_are_visible_at_small_and_large_sizes() {
        for (width, height) in [(80, 24), (110, 35)] {
            let demo = crate::bundled::Demo::create().unwrap();
            let mut app = App::new(Project::load(&demo.0.join("proposal.json")).unwrap());
            app.select(0, false);
            app.review.notes[0]
                .comments
                .push("My visible comment".into());
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(text.contains("1 comment"));
            app.key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('e'),
                crossterm::event::KeyModifiers::NONE,
            ));
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(text.contains("Comments · select one to edit"));
            assert!(text.contains("My visible comment"));
            assert!(text.contains("Enter/e edit"));
        }
    }

    #[test]
    fn renders_annotation_at_small_and_large_sizes() {
        for (width, height, selected) in [(65, 16, 0), (80, 24, 1), (100, 30, 0), (160, 50, 0)] {
            let project = Project::load(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("demo/proposal.json")
                    .as_path(),
            )
            .unwrap();
            let mut app = App::new(project);
            app.select(selected, false);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            assert!(
                buffer.content.iter().all(|cell| cell.bg == Color::Reset),
                "All surfaces should inherit the terminal background"
            );
            assert!(text.contains("PATCHWALK"));
            assert!(text.contains(if selected == 0 {
                "ADD AFTER"
            } else {
                "ADD BEFORE"
            }));
            if width == 80 {
                // A short terminal must retain actual source below the proposal card.
                assert!(text.contains("payments.charge(order)"));
            }
        }
    }

    #[test]
    fn same_line_annotations_remain_individually_navigable() {
        let project = Project::load(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("demo/proposal.json")
                .as_path(),
        )
        .unwrap();
        let mut app = App::new(project);
        for index in [1, 2] {
            app.select(index, false);
            let rows = code_rows(&app, 80);
            assert_eq!(
                rows.iter().filter(|r| r.annotation == Some(index)).count(),
                1
            );
            assert!(rows.iter().any(|r| r.source == Some(8)));
        }
    }
    #[test]
    fn diff_rendering_preserves_annotation_anchors_and_colored_changes() {
        let mut project =
            Project::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/proposal.json"))
                .unwrap();
        let file = "src/orders/validation.ts";
        let before = project.files[file].join("\n") + "\n";
        let after = before.replace("length > 0", "length > 1");
        project.diffs.insert(
            file.into(),
            crate::model::FileDiff::new(crate::protocol::ImplementedFile {
                path: file.into(),
                before: Some(before),
                after: Some(after),
            }),
        );
        let mut app = App::new(project);
        app.select(0, false);
        let rows = code_rows(&app, 100);
        assert_eq!(
            rows.iter().filter(|row| row.annotation == Some(0)).count(),
            1
        );
        assert!(rows.iter().any(|row| {
            row.text
                .spans
                .iter()
                .any(|s| s.style.fg == Some(DIFF_REMOVED))
        }));
        assert!(rows.iter().any(|row| {
            row.text
                .spans
                .iter()
                .any(|s| s.style.fg == Some(DIFF_ADDED))
        }));
        for (background, emphasis, character) in [
            (DIFF_ADDED_BG, DIFF_ADDED_EMPHASIS, "1"),
            (DIFF_REMOVED_BG, DIFF_REMOVED_EMPHASIS, "0"),
        ] {
            let row = rows
                .iter()
                .find(|row| row.text.style.bg == Some(background))
                .unwrap();
            assert!(row.text.width() >= 100);
            assert_eq!(row.text.spans.last().unwrap().style.bg, Some(background));
            let highlighted: String = row
                .text
                .spans
                .iter()
                .filter(|span| span.style.bg == Some(emphasis))
                .map(|span| span.content.as_ref())
                .collect();
            assert_eq!(highlighted, character);
        }
    }
}
