//! Full-screen review UI. No Git mutation happens in this module.

use std::{
    io::{self, stdout},
    process::Command,
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};

use crate::{compress::CompressionReport, provider::Candidate};

pub enum ReviewAction {
    Commit(String),
    Regenerate,
    ChangeModel(String),
    Cancel,
}

pub struct ReviewInput<'a> {
    pub candidates: &'a mut Vec<Candidate>,
    pub stats: &'a str,
    pub compression: &'a CompressionReport,
    pub provider: &'a str,
    pub model: &'a str,
    pub models: &'a [String],
}

#[derive(Debug, PartialEq, Eq)]
enum ReviewDecision {
    Commit(usize),
    Regenerate,
    ChangeModel(String),
    Edit(usize),
    Copy(usize),
    Cancel,
}

#[derive(Default)]
struct ReviewState {
    selected: usize,
}

impl ReviewState {
    fn handle_key(
        &mut self,
        code: KeyCode,
        candidate_count: usize,
        models: &[String],
        current_model: &str,
    ) -> Option<ReviewDecision> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => Some(ReviewDecision::Cancel),
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(candidate_count.saturating_sub(1));
                None
            }
            KeyCode::Enter => Some(ReviewDecision::Commit(self.selected)),
            KeyCode::Char('r') => Some(ReviewDecision::Regenerate),
            KeyCode::Char('m') if !models.is_empty() => {
                let current = models
                    .iter()
                    .position(|model| model == current_model)
                    .unwrap_or(0);
                Some(ReviewDecision::ChangeModel(
                    models[(current + 1) % models.len()].clone(),
                ))
            }
            KeyCode::Char('e') => Some(ReviewDecision::Edit(self.selected)),
            KeyCode::Char('c') => Some(ReviewDecision::Copy(self.selected)),
            _ => None,
        }
    }
}

pub fn review(input: ReviewInput<'_>) -> Result<ReviewAction> {
    let mut terminal = enter_terminal()?;
    let mut guard = TerminalGuard(true);
    let mut state = ReviewState::default();
    loop {
        terminal.draw(|frame| draw(frame, &input, state.selected))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match state.handle_key(key.code, input.candidates.len(), input.models, input.model) {
            Some(ReviewDecision::Cancel) => return Ok(ReviewAction::Cancel),
            Some(ReviewDecision::Commit(index)) => {
                return Ok(ReviewAction::Commit(input.candidates[index].message()))
            }
            Some(ReviewDecision::Regenerate) => return Ok(ReviewAction::Regenerate),
            Some(ReviewDecision::ChangeModel(model)) => {
                return Ok(ReviewAction::ChangeModel(model))
            }
            Some(ReviewDecision::Edit(index)) => {
                leave_terminal(&mut terminal)?;
                guard.0 = false;
                let edited = edit_message(&input.candidates[index].message())?;
                input.candidates[index] = split_message(&edited);
                terminal = enter_terminal()?;
                guard.0 = true;
            }
            Some(ReviewDecision::Copy(index)) => {
                copy_to_clipboard(&input.candidates[index].message())?
            }
            None => {}
        }
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, input: &ReviewInput<'_>, selected: usize) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(frame.area());
    let title = Paragraph::new(format!("commit-wisp  {} / {}", input.provider, input.model))
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" AI staged commit review "),
        );
    frame.render_widget(title, areas[0]);

    let metrics = format!(
        "{}\nEstimated tokens: {} → {}   Omitted lines: {}",
        input.stats.trim(),
        input.compression.original_tokens,
        input.compression.estimated_tokens,
        input.compression.omitted_lines
    );
    frame.render_widget(
        Paragraph::new(metrics)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Staged changes "),
            )
            .wrap(Wrap { trim: true }),
        areas[1],
    );

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(areas[2]);
    let items: Vec<ListItem<'_>> = input
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            ListItem::new(Line::from(format!("{}. {}", index + 1, candidate.subject)))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(selected));
    let list = List::new(items)
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).title(" Candidates "));
    frame.render_stateful_widget(list, horizontal[0], &mut state);
    frame.render_widget(
        Paragraph::new(Text::from(input.candidates[selected].message()))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Selected message "),
            )
            .wrap(Wrap { trim: false }),
        horizontal[1],
    );

    frame.render_widget(
        Paragraph::new(
            "↑/↓ select  Enter commit  e $EDITOR  r regenerate  m next model  c copy  q cancel",
        )
        .block(Block::default().borders(Borders::ALL)),
        areas[3],
    );
}

struct TerminalGuard(bool);

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.0 {
            let _ = disable_raw_mode();
            let _ = execute!(stdout(), LeaveAlternateScreen);
        }
    }
}

fn enter_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    Ok(terminal)
}

fn leave_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn edit_message(message: &str) -> Result<String> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(message.as_bytes())?;
    file.flush()?;
    let editor = std::env::var("GIT_EDITOR")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let status = Command::new(editor)
        .arg(file.path())
        .status()
        .context("Could not launch editor")?;
    anyhow::ensure!(status.success(), "Editor exited unsuccessfully");
    let edited = std::fs::read_to_string(file.path())?;
    anyhow::ensure!(
        !edited.trim().is_empty(),
        "Edited commit message cannot be empty"
    );
    Ok(edited.trim().into())
}

fn split_message(message: &str) -> Candidate {
    let mut parts = message.splitn(2, "\n\n");
    Candidate {
        subject: parts.next().unwrap_or_default().trim().into(),
        body: parts.next().map(|body| body.trim().into()),
    }
}

fn copy_to_clipboard(message: &str) -> Result<()> {
    use std::io::Write;
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pbcopy", &[])
    } else if cfg!(target_os = "windows") {
        ("clip", &[])
    } else {
        ("wl-copy", &[])
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("No supported clipboard command found")?;
    child
        .stdin
        .as_mut()
        .context("Clipboard stdin unavailable")?
        .write_all(message.as_bytes())?;
    anyhow::ensure!(child.wait()?.success(), "Clipboard command failed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    #[test]
    fn renders_candidates_metrics_and_controls() {
        let mut candidates = vec![
            Candidate {
                subject: "feat: first".into(),
                body: Some("Details".into()),
            },
            Candidate {
                subject: "fix: second".into(),
                body: None,
            },
        ];
        let compression = CompressionReport {
            content: "diff".into(),
            original_tokens: 100,
            estimated_tokens: 40,
            omitted_lines: 12,
        };
        let models = vec!["model-a".into(), "model-b".into()];
        let input = ReviewInput {
            candidates: &mut candidates,
            stats: "src/lib.rs | 3 ++-",
            compression: &compression,
            provider: "openai-compatible",
            model: "model-a",
            models: &models,
        };
        let backend = TestBackend::new(120, 28);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal.draw(|frame| draw(frame, &input, 0)).expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("feat: first"));
        assert!(rendered.contains("100 → 40"));
        assert!(rendered.contains("Enter commit"));
    }

    #[test]
    fn splits_subject_and_body_after_editor() {
        let candidate = split_message("fix(cli): correct review\n\nExplain the correction.\n");
        assert_eq!(candidate.subject, "fix(cli): correct review");
        assert_eq!(candidate.body.as_deref(), Some("Explain the correction."));
        assert_eq!(split_message("docs: readme").body, None);
    }

    #[test]
    fn review_state_handles_navigation_and_actions() {
        let models = vec!["model-a".into(), "model-b".into()];
        let mut state = ReviewState::default();
        assert_eq!(state.handle_key(KeyCode::Down, 2, &models, "model-a"), None);
        assert_eq!(state.selected, 1);
        assert_eq!(state.handle_key(KeyCode::Down, 2, &models, "model-a"), None);
        assert_eq!(state.selected, 1);
        assert_eq!(state.handle_key(KeyCode::Up, 2, &models, "model-a"), None);
        assert_eq!(state.selected, 0);
        assert_eq!(
            state.handle_key(KeyCode::Enter, 2, &models, "model-a"),
            Some(ReviewDecision::Commit(0))
        );
        assert_eq!(
            state.handle_key(KeyCode::Char('r'), 2, &models, "model-a"),
            Some(ReviewDecision::Regenerate)
        );
        assert_eq!(
            state.handle_key(KeyCode::Char('m'), 2, &models, "model-a"),
            Some(ReviewDecision::ChangeModel("model-b".into()))
        );
        assert_eq!(
            state.handle_key(KeyCode::Char('e'), 2, &models, "model-a"),
            Some(ReviewDecision::Edit(0))
        );
        assert_eq!(
            state.handle_key(KeyCode::Char('c'), 2, &models, "model-a"),
            Some(ReviewDecision::Copy(0))
        );
        assert_eq!(
            state.handle_key(KeyCode::Esc, 2, &models, "model-a"),
            Some(ReviewDecision::Cancel)
        );
        assert_eq!(
            state.handle_key(KeyCode::Char('x'), 2, &[], "missing"),
            None
        );
    }
}
