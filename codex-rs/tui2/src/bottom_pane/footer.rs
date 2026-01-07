use crate::chatwidget::Mode;
#[cfg(target_os = "linux")]
use crate::clipboard_paste::is_probably_wsl;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::render::line_utils::prefix_lines;
use crate::status::RateLimitSnapshotDisplay;
use crate::status::format_tokens_compact;
use crate::transcript_copy_action::TranscriptCopyFeedback;
use crate::ui_consts::FOOTER_INDENT_COLS;
use codex_core::protocol::SandboxPolicy;
use codex_protocol::openai_models::ReasoningEffort;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

#[derive(Clone, Debug)]
pub(crate) struct FooterProps {
    pub(crate) mode: FooterMode,
    pub(crate) esc_backtrack_hint: bool,
    pub(crate) use_shift_enter_hint: bool,
    pub(crate) is_task_running: bool,
    pub(crate) operation_mode: Mode,
    pub(crate) model_name: String,
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
    pub(crate) sandbox_policy: Option<SandboxPolicy>,
    pub(crate) context_window_percent: Option<i64>,
    pub(crate) context_window_used_tokens: Option<i64>,
    pub(crate) rate_limit_snapshot: Option<RateLimitSnapshotDisplay>,
    pub(crate) transcript_scrolled: bool,
    pub(crate) transcript_selection_active: bool,
    pub(crate) transcript_scroll_position: Option<(usize, usize)>,
    pub(crate) transcript_copy_selection_key: KeyBinding,
    pub(crate) transcript_copy_feedback: Option<TranscriptCopyFeedback>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FooterMode {
    CtrlCReminder,
    ShortcutSummary,
    ShortcutOverlay,
    EscHint,
    ContextOnly,
}

pub(crate) fn toggle_shortcut_mode(current: FooterMode, ctrl_c_hint: bool) -> FooterMode {
    if ctrl_c_hint && matches!(current, FooterMode::CtrlCReminder) {
        return current;
    }

    match current {
        FooterMode::ShortcutOverlay | FooterMode::CtrlCReminder => FooterMode::ShortcutSummary,
        _ => FooterMode::ShortcutOverlay,
    }
}

pub(crate) fn esc_hint_mode(current: FooterMode, is_task_running: bool) -> FooterMode {
    if is_task_running {
        current
    } else {
        FooterMode::EscHint
    }
}

pub(crate) fn reset_mode_after_activity(current: FooterMode) -> FooterMode {
    match current {
        FooterMode::EscHint
        | FooterMode::ShortcutOverlay
        | FooterMode::CtrlCReminder
        | FooterMode::ContextOnly => FooterMode::ShortcutSummary,
        other => other,
    }
}

pub(crate) fn footer_height(props: &FooterProps) -> u16 {
    footer_lines(props).len() as u16
}

pub(crate) fn render_footer(area: Rect, buf: &mut Buffer, props: &FooterProps) {
    Paragraph::new(prefix_lines(
        footer_lines(props),
        " ".repeat(FOOTER_INDENT_COLS).into(),
        " ".repeat(FOOTER_INDENT_COLS).into(),
    ))
    .render(area, buf);
}

fn footer_lines(props: &FooterProps) -> Vec<Line<'static>> {
    fn apply_copy_feedback(lines: &mut [Line<'static>], feedback: Option<TranscriptCopyFeedback>) {
        let Some(line) = lines.first_mut() else {
            return;
        };
        let Some(feedback) = feedback else {
            return;
        };

        line.push_span(" · ".dim());
        match feedback {
            TranscriptCopyFeedback::Copied => line.push_span("Copied".green().bold()),
            TranscriptCopyFeedback::Failed => line.push_span("Copy failed".red().bold()),
        }
    }

    // Show the context indicator on the left, appended after the primary hint
    // (e.g., "? for shortcuts"). Keep it visible even when typing (i.e., when
    // the shortcut hint is hidden). Hide it only for the multi-line
    // ShortcutOverlay.
    let mut lines = match props.mode {
        FooterMode::CtrlCReminder => vec![ctrl_c_reminder_line(CtrlCReminderState {
            is_task_running: props.is_task_running,
        })],
        FooterMode::ShortcutSummary => {
            let mut line = context_window_line(
                props.context_window_percent,
                props.context_window_used_tokens,
                props.rate_limit_snapshot.as_ref(),
            );
            line.push_span(" · ".dim());
            line.extend(vec![
                key_hint::plain(KeyCode::Char('?')).into(),
                " for shortcuts".dim(),
            ]);
            if props.transcript_scrolled {
                line.push_span(" · ".dim());
                line.push_span(key_hint::plain(KeyCode::PageUp));
                line.push_span("/");
                line.push_span(key_hint::plain(KeyCode::PageDown));
                line.push_span(" scroll".dim());
                line.push_span(" · ".dim());
                line.push_span(key_hint::plain(KeyCode::Home));
                line.push_span("/");
                line.push_span(key_hint::plain(KeyCode::End));
                line.push_span(" jump".dim());
                if let Some((current, total)) = props.transcript_scroll_position {
                    line.push_span(" · ".dim());
                    line.push_span(Span::from(format!("{current}/{total}")).dim());
                }
            }
            if props.transcript_selection_active {
                line.push_span(" · ".dim());
                line.push_span(props.transcript_copy_selection_key);
                line.push_span(" copy selection".dim());
            }
            vec![line]
        }
        FooterMode::ShortcutOverlay => {
            #[cfg(target_os = "linux")]
            let is_wsl = is_probably_wsl();
            #[cfg(not(target_os = "linux"))]
            let is_wsl = false;

            let state = ShortcutsState {
                use_shift_enter_hint: props.use_shift_enter_hint,
                esc_backtrack_hint: props.esc_backtrack_hint,
                is_wsl,
            };
            shortcut_overlay_lines(state)
        }
        FooterMode::EscHint => vec![esc_hint_line(props.esc_backtrack_hint)],
        FooterMode::ContextOnly => vec![context_window_line(
            props.context_window_percent,
            props.context_window_used_tokens,
            props.rate_limit_snapshot.as_ref(),
        )],
    };

    if matches!(
        props.mode,
        FooterMode::ShortcutSummary | FooterMode::ContextOnly
    ) && let Some(line) = lines.first_mut()
    {
        prepend_mode_label(
            line,
            props.operation_mode,
            &props.model_name,
            props.reasoning_effort,
            props.sandbox_policy.as_ref(),
        );
    }

    apply_copy_feedback(&mut lines, props.transcript_copy_feedback);
    lines
}

fn prepend_mode_label(
    line: &mut Line<'static>,
    operation_mode: Mode,
    model_name: &str,
    reasoning_effort: Option<ReasoningEffort>,
    sandbox_policy: Option<&SandboxPolicy>,
) {
    let label = match operation_mode {
        Mode::Plan => "Plan".yellow().bold(),
        Mode::Default => "Default".green().bold(),
        Mode::AcceptEdits => "Accept edits".magenta().bold(),
    };

    let mut spans = Vec::with_capacity(line.spans.len() + 8);
    spans.push(label);
    spans.push(" · ".dim());
    spans.push(Span::from(model_name.to_string()).cyan());

    // Add reasoning effort if set
    if let Some(effort) = reasoning_effort {
        let effort_label = match effort {
            ReasoningEffort::None => "none",
            ReasoningEffort::Minimal => "minimal",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::XHigh => "x-high",
        };
        spans.push(" · ".dim());
        spans.push(Span::from(effort_label).dim());
    }

    // Add sandbox policy label
    if let Some(policy) = sandbox_policy {
        let sandbox_label = match policy {
            SandboxPolicy::ReadOnly => "read-only",
            SandboxPolicy::WorkspaceWrite { .. } => "project",
            SandboxPolicy::DangerFullAccess => "full-access",
            SandboxPolicy::ExternalSandbox { .. } => "external",
        };
        spans.push(" · ".dim());
        spans.push(Span::from(sandbox_label).dim());
    }

    spans.push(" · ".dim());
    spans.append(&mut line.spans);
    *line = Line::from(spans);
}

#[derive(Clone, Copy, Debug)]
struct CtrlCReminderState {
    is_task_running: bool,
}

#[derive(Clone, Copy, Debug)]
struct ShortcutsState {
    use_shift_enter_hint: bool,
    esc_backtrack_hint: bool,
    is_wsl: bool,
}

fn ctrl_c_reminder_line(state: CtrlCReminderState) -> Line<'static> {
    let action = if state.is_task_running {
        "interrupt"
    } else {
        "quit"
    };
    Line::from(vec![
        key_hint::ctrl(KeyCode::Char('c')).into(),
        format!(" again to {action}").into(),
    ])
    .dim()
}

fn esc_hint_line(esc_backtrack_hint: bool) -> Line<'static> {
    let esc = key_hint::plain(KeyCode::Esc);
    if esc_backtrack_hint {
        Line::from(vec![esc.into(), " again to edit previous message".into()]).dim()
    } else {
        Line::from(vec![
            esc.into(),
            " ".into(),
            esc.into(),
            " to edit previous message".into(),
        ])
        .dim()
    }
}

fn shortcut_overlay_lines(state: ShortcutsState) -> Vec<Line<'static>> {
    let mut commands = Line::from("");
    let mut newline = Line::from("");
    let mut file_paths = Line::from("");
    let mut paste_image = Line::from("");
    let mut toggle_plan_mode = Line::from("");
    let mut edit_previous = Line::from("");
    let mut quit = Line::from("");
    let mut show_transcript = Line::from("");

    for descriptor in SHORTCUTS {
        if let Some(text) = descriptor.overlay_entry(state) {
            match descriptor.id {
                ShortcutId::Commands => commands = text,
                ShortcutId::InsertNewline => newline = text,
                ShortcutId::FilePaths => file_paths = text,
                ShortcutId::PasteImage => paste_image = text,
                ShortcutId::TogglePlanMode => toggle_plan_mode = text,
                ShortcutId::EditPrevious => edit_previous = text,
                ShortcutId::Quit => quit = text,
                ShortcutId::ShowTranscript => show_transcript = text,
            }
        }
    }

    let ordered = vec![
        commands,
        newline,
        file_paths,
        paste_image,
        toggle_plan_mode,
        edit_previous,
        quit,
        Line::from(""),
        show_transcript,
    ];

    build_columns(ordered)
}

fn build_columns(entries: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if entries.is_empty() {
        return Vec::new();
    }

    const COLUMNS: usize = 2;
    const COLUMN_PADDING: [usize; COLUMNS] = [4, 4];
    const COLUMN_GAP: usize = 4;

    let rows = entries.len().div_ceil(COLUMNS);
    let target_len = rows * COLUMNS;
    let mut entries = entries;
    if entries.len() < target_len {
        entries.extend(std::iter::repeat_n(
            Line::from(""),
            target_len - entries.len(),
        ));
    }

    let mut column_widths = [0usize; COLUMNS];

    for (idx, entry) in entries.iter().enumerate() {
        let column = idx % COLUMNS;
        column_widths[column] = column_widths[column].max(entry.width());
    }

    for (idx, width) in column_widths.iter_mut().enumerate() {
        *width += COLUMN_PADDING[idx];
    }

    entries
        .chunks(COLUMNS)
        .map(|chunk| {
            let mut line = Line::from("");
            for (col, entry) in chunk.iter().enumerate() {
                line.extend(entry.spans.clone());
                if col < COLUMNS - 1 {
                    let target_width = column_widths[col];
                    let padding = target_width.saturating_sub(entry.width()) + COLUMN_GAP;
                    line.push_span(Span::from(" ".repeat(padding)));
                }
            }
            line.dim()
        })
        .collect()
}

fn context_window_line(
    percent: Option<i64>,
    used_tokens: Option<i64>,
    rate_limit_snapshot: Option<&RateLimitSnapshotDisplay>,
) -> Line<'static> {
    let mut line = if let Some(percent) = percent {
        let percent = percent.clamp(0, 100);
        Line::from(vec![Span::from(format!("{percent}% context left")).dim()])
    } else if let Some(tokens) = used_tokens {
        let used_fmt = format_tokens_compact(tokens);
        Line::from(vec![Span::from(format!("{used_fmt} used")).dim()])
    } else {
        Line::from(vec![Span::from("100% context left").dim()])
    };

    // Append rate limit quota if available
    if let Some(quota_span) = rate_limit_snapshot.and_then(format_rate_limit_quota) {
        line.push_span(" · ".dim());
        line.push_span(quota_span);
    }

    line
}

/// Format the rate limit quota for footer display.
/// Shows the reset time and remaining percentage (e.g., "14:30 · 70%").
/// Color-coded: red (≤10%), yellow (≤25%), dim (otherwise).
fn format_rate_limit_quota(snapshot: &RateLimitSnapshotDisplay) -> Option<Span<'static>> {
    // Prioritize primary (5h) limit, then secondary (weekly)
    let window = snapshot.primary.as_ref().or(snapshot.secondary.as_ref())?;

    let remaining = (100.0_f64 - window.used_percent).round() as i64;
    let remaining = remaining.clamp(0, 100);

    let style = if remaining <= 10 {
        Style::default().red()
    } else if remaining <= 25 {
        Style::default().cyan()
    } else {
        Style::default().dim()
    };

    let text = if let Some(reset_time) = &window.resets_at {
        format!("{reset_time} · {remaining}%")
    } else {
        format!("{remaining}%")
    };

    Some(Span::styled(text, style))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShortcutId {
    Commands,
    InsertNewline,
    FilePaths,
    PasteImage,
    TogglePlanMode,
    EditPrevious,
    Quit,
    ShowTranscript,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ShortcutBinding {
    key: KeyBinding,
    condition: DisplayCondition,
}

impl ShortcutBinding {
    fn matches(&self, state: ShortcutsState) -> bool {
        self.condition.matches(state)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DisplayCondition {
    Always,
    WhenShiftEnterHint,
    WhenNotShiftEnterHint,
    WhenUnderWSL,
}

impl DisplayCondition {
    fn matches(self, state: ShortcutsState) -> bool {
        match self {
            DisplayCondition::Always => true,
            DisplayCondition::WhenShiftEnterHint => state.use_shift_enter_hint,
            DisplayCondition::WhenNotShiftEnterHint => !state.use_shift_enter_hint,
            DisplayCondition::WhenUnderWSL => state.is_wsl,
        }
    }
}

struct ShortcutDescriptor {
    id: ShortcutId,
    bindings: &'static [ShortcutBinding],
    prefix: &'static str,
    label: &'static str,
}

impl ShortcutDescriptor {
    fn binding_for(&self, state: ShortcutsState) -> Option<&'static ShortcutBinding> {
        self.bindings.iter().find(|binding| binding.matches(state))
    }

    fn overlay_entry(&self, state: ShortcutsState) -> Option<Line<'static>> {
        let binding = self.binding_for(state)?;
        let mut line = Line::from(vec![self.prefix.into(), binding.key.into()]);
        match self.id {
            ShortcutId::EditPrevious => {
                if state.esc_backtrack_hint {
                    line.push_span(" again to edit previous message");
                } else {
                    line.extend(vec![
                        " ".into(),
                        key_hint::plain(KeyCode::Esc).into(),
                        " to edit previous message".into(),
                    ]);
                }
            }
            _ => line.push_span(self.label),
        };
        Some(line)
    }
}

const SHORTCUTS: &[ShortcutDescriptor] = &[
    ShortcutDescriptor {
        id: ShortcutId::Commands,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::Char('/')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " for commands",
    },
    ShortcutDescriptor {
        id: ShortcutId::InsertNewline,
        bindings: &[
            ShortcutBinding {
                key: key_hint::shift(KeyCode::Enter),
                condition: DisplayCondition::WhenShiftEnterHint,
            },
            ShortcutBinding {
                key: key_hint::ctrl(KeyCode::Char('j')),
                condition: DisplayCondition::WhenNotShiftEnterHint,
            },
        ],
        prefix: "",
        label: " for newline",
    },
    ShortcutDescriptor {
        id: ShortcutId::FilePaths,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::Char('@')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " for file paths",
    },
    ShortcutDescriptor {
        id: ShortcutId::PasteImage,
        // Show Ctrl+Alt+V when running under WSL (terminals often intercept plain
        // Ctrl+V); otherwise fall back to Ctrl+V.
        bindings: &[
            ShortcutBinding {
                key: key_hint::ctrl_alt(KeyCode::Char('v')),
                condition: DisplayCondition::WhenUnderWSL,
            },
            ShortcutBinding {
                key: key_hint::ctrl(KeyCode::Char('v')),
                condition: DisplayCondition::Always,
            },
        ],
        prefix: "",
        label: " to paste images",
    },
    ShortcutDescriptor {
        id: ShortcutId::TogglePlanMode,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::BackTab),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to cycle modes",
    },
    ShortcutDescriptor {
        id: ShortcutId::EditPrevious,
        bindings: &[ShortcutBinding {
            key: key_hint::plain(KeyCode::Esc),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: "",
    },
    ShortcutDescriptor {
        id: ShortcutId::Quit,
        bindings: &[ShortcutBinding {
            key: key_hint::ctrl(KeyCode::Char('c')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to exit",
    },
    ShortcutDescriptor {
        id: ShortcutId::ShowTranscript,
        bindings: &[ShortcutBinding {
            key: key_hint::ctrl(KeyCode::Char('t')),
            condition: DisplayCondition::Always,
        }],
        prefix: "",
        label: " to view transcript",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::RateLimitWindowDisplay;
    use chrono::Local;
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn snapshot_footer(name: &str, props: FooterProps) {
        let height = footer_height(&props).max(1);
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|f| {
                let area = Rect::new(0, 0, f.area().width, height);
                render_footer(area, f.buffer_mut(), &props);
            })
            .unwrap();
        assert_snapshot!(name, terminal.backend());
    }

    #[test]
    fn footer_snapshots() {
        snapshot_footer(
            "footer_shortcuts_default",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_shortcuts_transcript_scrolled_and_selection",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: true,
                transcript_selection_active: true,
                transcript_scroll_position: Some((3, 42)),
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_shortcuts_shift_and_esc",
            FooterProps {
                mode: FooterMode::ShortcutOverlay,
                esc_backtrack_hint: true,
                use_shift_enter_hint: true,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_ctrl_c_quit_idle",
            FooterProps {
                mode: FooterMode::CtrlCReminder,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_ctrl_c_quit_running",
            FooterProps {
                mode: FooterMode::CtrlCReminder,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: true,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_esc_hint_idle",
            FooterProps {
                mode: FooterMode::EscHint,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_esc_hint_primed",
            FooterProps {
                mode: FooterMode::EscHint,
                esc_backtrack_hint: true,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_shortcuts_context_running",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: true,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: Some(72),
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_context_tokens_used",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: Some(123_456),
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        snapshot_footer(
            "footer_copy_feedback_copied",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: None,
                context_window_used_tokens: None,
                rate_limit_snapshot: None,
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: Some(TranscriptCopyFeedback::Copied),
            },
        );

        // Footer with rate limit quota - high remaining (dim style)
        snapshot_footer(
            "footer_with_rate_limit_high",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: Some(50),
                context_window_used_tokens: None,
                rate_limit_snapshot: Some(RateLimitSnapshotDisplay {
                    captured_at: Local::now(),
                    primary: Some(RateLimitWindowDisplay {
                        used_percent: 30.0, // 70% remaining - dim
                        resets_at: Some("14:30".to_string()),
                        window_minutes: Some(300),
                    }),
                    secondary: None,
                    credits: None,
                }),
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        // Footer with rate limit quota - warning zone (yellow style)
        snapshot_footer(
            "footer_with_rate_limit_warning",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: Some(50),
                context_window_used_tokens: None,
                rate_limit_snapshot: Some(RateLimitSnapshotDisplay {
                    captured_at: Local::now(),
                    primary: Some(RateLimitWindowDisplay {
                        used_percent: 80.0, // 20% remaining - yellow
                        resets_at: Some("15:00".to_string()),
                        window_minutes: Some(300),
                    }),
                    secondary: None,
                    credits: None,
                }),
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        // Footer with rate limit quota - critical zone (red style)
        snapshot_footer(
            "footer_with_rate_limit_critical",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: Some(50),
                context_window_used_tokens: None,
                rate_limit_snapshot: Some(RateLimitSnapshotDisplay {
                    captured_at: Local::now(),
                    primary: Some(RateLimitWindowDisplay {
                        used_percent: 95.0, // 5% remaining - red
                        resets_at: Some("15:30".to_string()),
                        window_minutes: Some(300),
                    }),
                    secondary: None,
                    credits: None,
                }),
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );

        // Footer with rate limit quota - weekly window (secondary fallback)
        snapshot_footer(
            "footer_with_rate_limit_weekly",
            FooterProps {
                mode: FooterMode::ShortcutSummary,
                esc_backtrack_hint: false,
                use_shift_enter_hint: false,
                is_task_running: false,
                operation_mode: Mode::Default,
                model_name: "gpt-4.1".to_string(),
                reasoning_effort: None,
                sandbox_policy: None,
                context_window_percent: Some(50),
                context_window_used_tokens: None,
                rate_limit_snapshot: Some(RateLimitSnapshotDisplay {
                    captured_at: Local::now(),
                    primary: None,
                    secondary: Some(RateLimitWindowDisplay {
                        used_percent: 40.0, // 60% remaining
                        resets_at: Some("12:00 on 5 Jan".to_string()),
                        window_minutes: Some(10080),
                    }),
                    credits: None,
                }),
                transcript_scrolled: false,
                transcript_selection_active: false,
                transcript_scroll_position: None,
                transcript_copy_selection_key: key_hint::ctrl_shift(KeyCode::Char('c')),
                transcript_copy_feedback: None,
            },
        );
    }

    #[test]
    fn format_rate_limit_quota_tests() {
        // Test: With reset time shows "time · percentage"
        let snapshot = RateLimitSnapshotDisplay {
            captured_at: Local::now(),
            primary: Some(RateLimitWindowDisplay {
                used_percent: 30.0,
                resets_at: Some("14:30".to_string()),
                window_minutes: Some(300),
            }),
            secondary: None,
            credits: None,
        };
        let result = format_rate_limit_quota(&snapshot);
        assert!(result.is_some());
        assert_eq!(result.unwrap().content, "14:30 · 70%");

        // Test: Without reset time shows just percentage
        let snapshot = RateLimitSnapshotDisplay {
            captured_at: Local::now(),
            primary: Some(RateLimitWindowDisplay {
                used_percent: 30.0,
                resets_at: None,
                window_minutes: Some(300),
            }),
            secondary: None,
            credits: None,
        };
        let result = format_rate_limit_quota(&snapshot);
        assert_eq!(result.unwrap().content, "70%");

        // Test: 90% used = 10% remaining (red boundary)
        let snapshot = RateLimitSnapshotDisplay {
            captured_at: Local::now(),
            primary: Some(RateLimitWindowDisplay {
                used_percent: 90.0,
                resets_at: Some("15:00".to_string()),
                window_minutes: Some(300),
            }),
            secondary: None,
            credits: None,
        };
        let result = format_rate_limit_quota(&snapshot);
        assert_eq!(result.unwrap().content, "15:00 · 10%");

        // Test: No primary, use secondary
        let snapshot = RateLimitSnapshotDisplay {
            captured_at: Local::now(),
            primary: None,
            secondary: Some(RateLimitWindowDisplay {
                used_percent: 50.0,
                resets_at: Some("12:00 on 5 Jan".to_string()),
                window_minutes: Some(10080),
            }),
            credits: None,
        };
        let result = format_rate_limit_quota(&snapshot);
        assert_eq!(result.unwrap().content, "12:00 on 5 Jan · 50%");

        // Test: No windows at all returns None
        let snapshot = RateLimitSnapshotDisplay {
            captured_at: Local::now(),
            primary: None,
            secondary: None,
            credits: None,
        };
        assert!(format_rate_limit_quota(&snapshot).is_none());

        // Test: 100% used = 0% remaining (clamped)
        let snapshot = RateLimitSnapshotDisplay {
            captured_at: Local::now(),
            primary: Some(RateLimitWindowDisplay {
                used_percent: 100.0,
                resets_at: Some("16:00".to_string()),
                window_minutes: Some(300),
            }),
            secondary: None,
            credits: None,
        };
        assert_eq!(
            format_rate_limit_quota(&snapshot).unwrap().content,
            "16:00 · 0%"
        );

        // Test: 0% used = 100% remaining
        let snapshot = RateLimitSnapshotDisplay {
            captured_at: Local::now(),
            primary: Some(RateLimitWindowDisplay {
                used_percent: 0.0,
                resets_at: Some("17:00".to_string()),
                window_minutes: Some(300),
            }),
            secondary: None,
            credits: None,
        };
        assert_eq!(
            format_rate_limit_quota(&snapshot).unwrap().content,
            "17:00 · 100%"
        );
    }
}
