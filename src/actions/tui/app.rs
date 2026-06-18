use std::time::Instant;

use ratatui::crossterm::event::KeyCode;
use ratatui::text::Line;
use tokio::sync::mpsc;

use crate::agent::ApprovalDecision;
use crate::config::Config;
use crate::llm::message::ChatMessage;

use super::render::message_lines;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    User,
    Assistant,
    /// A tool call or its result, rendered as an arrowed, dimmed aside.
    Tool,
    Error,
    /// A local note from the client itself (e.g. after `/clear`), shown
    /// dimmed and without a speaker label.
    System,
}

#[derive(Debug, Clone)]
pub(super) struct DisplayMessage {
    pub(super) role: Role,
    pub(super) content: String,
}

/// A tool call awaiting the user's approval, captured so the confirmation line
/// can name it once a key is pressed.
pub(super) struct PendingApproval {
    pub(super) name: String,
    pub(super) args: String,
}

pub(super) struct App {
    pub(super) endpoint: String,
    pub(super) model: String,
    pub(super) messages: Vec<DisplayMessage>,
    /// Authoritative conversation sent to the model (system + turns).
    pub(super) history: Vec<ChatMessage>,
    pub(super) input: String,
    /// Scroll position measured in rows up from the bottom of the transcript.
    /// 0 means "stick to the latest message".
    pub(super) scroll_from_bottom: usize,
    pub(super) thinking: bool,
    /// When true, the status bar also shows the endpoint.
    /// Toggled with Tab (Ctrl+I); off by default to keep the chrome minimal.
    pub(super) show_details: bool,
    /// Index of the in-progress assistant bubble, if streaming.
    pub(super) streaming_idx: Option<usize>,
    /// Approval channel back to the running agent turn, for the (Phase 6)
    /// approval UI. Present only while a turn is in flight.
    pub(super) appr_tx: Option<mpsc::UnboundedSender<ApprovalDecision>>,
    /// The mutating tool call currently awaiting a y/n/a keypress, if any.
    pub(super) pending_approval: Option<PendingApproval>,
    /// Live chain-of-thought for the in-flight turn, shown dimmed and cleared
    /// ("collapsed") once the real answer streams or the turn ends.
    pub(super) reasoning: String,
    /// Whether the next request lets the model think. Toggled with Ctrl+T.
    pub(super) think: bool,
    /// Start time, used for the steady, mode-independent bird wing-beat.
    pub(super) started: Instant,
    /// Accumulated drift phase for the scrolling sky and waves. Advanced each
    /// frame by the elapsed time, faster while `thinking`, so entering thinking
    /// mode accelerates the scene from where it is instead of jumping.
    pub(super) anim_phase: f64,
    /// Wall-clock instant of the previous frame, used to measure that elapsed
    /// time.
    pub(super) last_tick: Instant,
    /// Handle to the in-flight turn's agent task, kept so Esc can abort it.
    pub(super) agent_task: Option<tokio::task::JoinHandle<()>>,
    /// Cached wrapped transcript lines, keyed by `(messages.len(), last-message
    /// content length, width)`. The streaming bubble only ever mutates the last
    /// message in place, so this fingerprint catches every rendering-relevant
    /// change and lets the 50ms redraw skip re-wrapping the whole transcript.
    pub(super) transcript_cache: Option<(usize, usize, usize, Vec<Line<'static>>)>,
}

impl App {
    pub(super) fn new(cfg: &Config, model: String) -> Self {
        Self {
            endpoint: cfg.base_url.clone(),
            model,
            messages: Vec::new(),
            history: Vec::new(),
            input: String::new(),
            scroll_from_bottom: 0,
            thinking: false,
            show_details: false,
            streaming_idx: None,
            appr_tx: None,
            pending_approval: None,
            reasoning: String::new(),
            think: false,
            started: Instant::now(),
            anim_phase: 0.0,
            last_tick: Instant::now(),
            agent_task: None,
            transcript_cache: None,
        }
    }

    pub(super) fn transcript_lines(&mut self, width: usize) -> Vec<Line<'static>> {
        let key = (
            self.messages.len(),
            self.messages.last().map_or(0, |m| m.content.len()),
            width,
        );
        let fresh = matches!(&self.transcript_cache, Some((l, c, w, _)) if (*l, *c, *w) == key);
        if !fresh {
            let lines = message_lines(&self.messages, width);
            self.transcript_cache = Some((key.0, key.1, key.2, lines));
        }
        self.transcript_cache.as_ref().unwrap().3.clone()
    }

    pub(super) fn push(&mut self, role: Role, content: String) {
        self.messages.push(DisplayMessage { role, content });
        // New content should be visible immediately.
        self.scroll_from_bottom = 0;
    }

    /// Open a fresh assistant bubble for streaming deltas.
    pub(super) fn begin_assistant(&mut self) {
        self.messages.push(DisplayMessage {
            role: Role::Assistant,
            content: String::new(),
        });
        self.streaming_idx = Some(self.messages.len() - 1);
        self.scroll_from_bottom = 0;
    }

    pub(super) fn push_delta(&mut self, delta: &str) {
        if self.streaming_idx.is_none() {
            self.begin_assistant();
        }
        let idx = self.streaming_idx.unwrap();
        self.messages[idx].content.push_str(delta);
        self.scroll_from_bottom = 0;
    }

    pub(super) fn end_assistant(&mut self) {
        self.streaming_idx = None;
    }

    pub(super) fn stop_turn(&mut self) {
        if let Some(handle) = self.agent_task.take() {
            handle.abort();
        }
        self.thinking = false;
        self.end_assistant();
        self.reasoning.clear();
        self.appr_tx = None;
        self.pending_approval = None;
        self.push(Role::System, "Stopped.".into());
    }

    pub(super) fn approval_key(&self, code: KeyCode) -> Option<ApprovalDecision> {
        match code {
            KeyCode::Char('y') | KeyCode::Enter => Some(ApprovalDecision::Approve),
            KeyCode::Char('a') => Some(ApprovalDecision::ApproveAlways),
            KeyCode::Char('n') => Some(ApprovalDecision::Deny),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
