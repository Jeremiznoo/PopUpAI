//! Session lifecycle, as decided in `docs/ADR/0003-session-and-cancellation-policy.md`.
//!
//! This is a pure state machine: it performs no I/O. Callers carry out the
//! returned [`Effect`]s, which keeps every rule testable without a desktop or a
//! network connection.

use std::fmt;
use std::time::{Duration, Instant};

/// Identifies one popup opening. Ids are never reused within a process.
pub type SessionId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Open, nothing submitted yet.
    Idle,
    Streaming,
    Complete,
    /// Stopped by the user; the partial answer is kept.
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    id: SessionId,
    messages: Vec<Message>,
    phase: Phase,
}

impl Session {
    fn new(id: SessionId) -> Self {
        Self {
            id,
            messages: Vec::new(),
            phase: Phase::Idle,
        }
    }

    pub fn id(&self) -> SessionId {
        self.id
    }

    /// The conversation so far. Right after `submit` it ends with the user's
    /// message, which is exactly what a provider request needs.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    fn is_streaming(&self) -> bool {
        self.phase == Phase::Streaming
    }
}

/// What happens to a response that is still being generated when the popup is
/// dismissed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DismissPolicy {
    /// Keep generating in the background and hold the result in memory.
    #[default]
    FinishInBackground,
    /// Abort the request immediately.
    CancelOnDismiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    pub dismiss: DismissPolicy,
    /// How long a detached answer stays available before it is dropped.
    pub detached_ttl: Duration,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            dismiss: DismissPolicy::default(),
            detached_ttl: Duration::from_secs(600),
        }
    }
}

/// Work the caller must perform on behalf of the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Abort the in-flight provider request belonging to this session.
    CancelRequest(SessionId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub id: SessionId,
    /// A background answer can be recalled with [`SessionManager::resume_detached`].
    pub detached_available: bool,
    pub effects: Vec<Effect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submitted {
    pub id: SessionId,
    pub effects: Vec<Effect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    NoActiveSession,
    EmptyPrompt,
    /// A response is already being generated in this session.
    Busy,
}

impl fmt::Display for SubmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoActiveSession => "no active session",
            Self::EmptyPrompt => "the prompt is empty",
            Self::Busy => "a response is already being generated",
        })
    }
}

impl std::error::Error for SubmitError {}

#[derive(Debug)]
struct Detached {
    session: Session,
    since: Instant,
}

/// Owns the visible session and at most one detached (background) session.
///
/// Nothing here is ever written to disk. Invariant: at most one provider
/// request is in flight at any time.
#[derive(Debug)]
pub struct SessionManager {
    policy: SessionPolicy,
    next_id: SessionId,
    active: Option<Session>,
    detached: Option<Detached>,
}

impl SessionManager {
    pub fn new(policy: SessionPolicy) -> Self {
        Self {
            policy,
            next_id: 1,
            active: None,
            detached: None,
        }
    }

    pub fn active(&self) -> Option<&Session> {
        self.active.as_ref()
    }

    pub fn detached(&self) -> Option<&Session> {
        self.detached.as_ref().map(|d| &d.session)
    }

    /// The popup was opened: start a fresh, empty session that inherits nothing.
    /// If a session is still open (a missed dismissal), the dismissal policy is
    /// applied to it first.
    pub fn open(&mut self, now: Instant) -> Opened {
        let mut effects = self.expire(now);
        effects.extend(self.dismiss(now));
        let id = self.next_id;
        self.next_id += 1;
        self.active = Some(Session::new(id));
        Opened {
            id,
            detached_available: self.detached.is_some(),
            effects,
        }
    }

    /// The popup was dismissed (outside click, Escape or the shortcut). This
    /// never cancels on its own unless the policy says so; only [`Self::stop`]
    /// does.
    pub fn dismiss(&mut self, now: Instant) -> Vec<Effect> {
        let Some(session) = self.active.take() else {
            return Vec::new();
        };
        if !session.is_streaming() {
            return Vec::new();
        }
        match self.policy.dismiss {
            DismissPolicy::CancelOnDismiss => vec![Effect::CancelRequest(session.id)],
            DismissPolicy::FinishInBackground => self
                .detached
                .replace(Detached {
                    session,
                    since: now,
                })
                .filter(|old| old.session.is_streaming())
                .map(|old| Effect::CancelRequest(old.session.id))
                .into_iter()
                .collect(),
        }
    }

    /// Send a prompt in the active session. A new question always supersedes any
    /// background answer, which is dropped (and cancelled if still running).
    pub fn submit(&mut self, prompt: &str) -> Result<Submitted, SubmitError> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(SubmitError::EmptyPrompt);
        }
        let session = self.active.as_mut().ok_or(SubmitError::NoActiveSession)?;
        if session.is_streaming() {
            return Err(SubmitError::Busy);
        }
        session.messages.push(Message {
            role: Role::User,
            text: prompt.to_owned(),
        });
        session.phase = Phase::Streaming;
        let id = session.id;
        Ok(Submitted {
            id,
            effects: self.drop_detached(),
        })
    }

    /// Append streamed text. Returns `false` and changes nothing if the session
    /// is unknown or no longer streaming, so late chunks from a cancelled or
    /// dropped request are ignored.
    pub fn push_chunk(&mut self, id: SessionId, text: &str) -> bool {
        let Some(session) = self.find_mut(id) else {
            return false;
        };
        if !session.is_streaming() {
            return false;
        }
        match session.messages.last_mut() {
            Some(last) if last.role == Role::Assistant => last.text.push_str(text),
            _ => session.messages.push(Message {
                role: Role::Assistant,
                text: text.to_owned(),
            }),
        }
        true
    }

    pub fn finish(&mut self, id: SessionId) -> bool {
        self.transition(id, Phase::Complete)
    }

    pub fn fail(&mut self, id: SessionId) -> bool {
        self.transition(id, Phase::Failed)
    }

    /// The user pressed Stop: abort generation in the visible session.
    pub fn stop(&mut self) -> Vec<Effect> {
        match self.active.as_mut() {
            Some(session) if session.is_streaming() => {
                session.phase = Phase::Cancelled;
                vec![Effect::CancelRequest(session.id)]
            }
            _ => Vec::new(),
        }
    }

    /// Bring the background answer back into view. Refused if the visible
    /// session already has content, or if the answer has expired.
    pub fn resume_detached(&mut self, now: Instant) -> Option<SessionId> {
        if self.active.as_ref().is_some_and(|s| !s.messages.is_empty()) || self.is_expired(now) {
            return None;
        }
        let detached = self.detached.take()?;
        let id = detached.session.id;
        self.active = Some(detached.session);
        Some(id)
    }

    /// Drop the background answer once its retention time has passed. Call this
    /// from a timer; it also runs on every [`Self::open`].
    pub fn expire(&mut self, now: Instant) -> Vec<Effect> {
        if self.is_expired(now) {
            self.drop_detached()
        } else {
            Vec::new()
        }
    }

    fn is_expired(&self, now: Instant) -> bool {
        self.detached
            .as_ref()
            .is_some_and(|d| now.saturating_duration_since(d.since) >= self.policy.detached_ttl)
    }

    fn drop_detached(&mut self) -> Vec<Effect> {
        match self.detached.take() {
            Some(d) if d.session.is_streaming() => vec![Effect::CancelRequest(d.session.id)],
            _ => Vec::new(),
        }
    }

    fn transition(&mut self, id: SessionId, to: Phase) -> bool {
        match self.find_mut(id) {
            Some(session) if session.is_streaming() => {
                session.phase = to;
                true
            }
            _ => false,
        }
    }

    fn find_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        if let Some(session) = self.active.as_mut().filter(|s| s.id == id) {
            return Some(session);
        }
        self.detached
            .as_mut()
            .map(|d| &mut d.session)
            .filter(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(600);

    fn manager(dismiss: DismissPolicy) -> (SessionManager, Instant) {
        let policy = SessionPolicy {
            dismiss,
            detached_ttl: TTL,
        };
        (SessionManager::new(policy), Instant::now())
    }

    fn streaming_sessions(m: &SessionManager) -> usize {
        [m.active(), m.detached()]
            .into_iter()
            .flatten()
            .filter(|s| s.phase() == Phase::Streaming)
            .count()
    }

    #[test]
    fn each_opening_is_a_fresh_session_with_no_inherited_context() {
        let (mut m, t) = manager(DismissPolicy::default());
        let first = m.open(t);
        m.submit("What is DNS?").unwrap();
        m.push_chunk(first.id, "A naming system.");
        m.finish(first.id);
        assert!(m.dismiss(t).is_empty());

        let second = m.open(t);
        assert_ne!(first.id, second.id);
        assert!(m.active().unwrap().messages().is_empty());
        assert_eq!(m.active().unwrap().phase(), Phase::Idle);
        assert!(!second.detached_available);
    }

    #[test]
    fn follow_ups_within_one_opening_share_context() {
        let (mut m, t) = manager(DismissPolicy::default());
        let id = m.open(t).id;
        m.submit("What is DNS?").unwrap();
        m.push_chunk(id, "A naming ");
        m.push_chunk(id, "system.");
        m.finish(id);
        m.submit("And DHCP?").unwrap();

        let roles: Vec<_> = m
            .active()
            .unwrap()
            .messages()
            .iter()
            .map(|x| x.role)
            .collect();
        assert_eq!(roles, [Role::User, Role::Assistant, Role::User]);
        assert_eq!(m.active().unwrap().messages()[1].text, "A naming system.");
    }

    #[test]
    fn submit_rejects_empty_prompt_missing_session_and_concurrent_requests() {
        let (mut m, t) = manager(DismissPolicy::default());
        assert_eq!(m.submit("hi"), Err(SubmitError::NoActiveSession));
        m.open(t);
        assert_eq!(m.submit("   \n"), Err(SubmitError::EmptyPrompt));
        m.submit("hi").unwrap();
        assert_eq!(m.submit("again"), Err(SubmitError::Busy));
    }

    #[test]
    fn dismissing_a_finished_session_drops_it_without_effects() {
        let (mut m, t) = manager(DismissPolicy::default());
        let id = m.open(t).id;
        m.submit("hi").unwrap();
        m.finish(id);
        assert!(m.dismiss(t).is_empty());
        assert!(m.active().is_none());
        assert!(m.detached().is_none());
    }

    #[test]
    fn dismissing_while_streaming_keeps_generating_in_the_background() {
        let (mut m, t) = manager(DismissPolicy::FinishInBackground);
        let id = m.open(t).id;
        m.submit("long question").unwrap();
        m.push_chunk(id, "partial ");

        assert!(m.dismiss(t).is_empty(), "dismissal must not cancel");
        assert!(m.active().is_none());
        assert_eq!(m.detached().unwrap().phase(), Phase::Streaming);

        assert!(m.push_chunk(id, "and the rest"));
        assert!(m.finish(id));
        assert_eq!(m.detached().unwrap().phase(), Phase::Complete);

        let reopened = m.open(t);
        assert!(reopened.detached_available);
        assert!(m.active().unwrap().messages().is_empty());

        assert_eq!(m.resume_detached(t), Some(id));
        let resumed = m.active().unwrap();
        assert_eq!(resumed.messages()[1].text, "partial and the rest");
        assert!(m.detached().is_none());
    }

    #[test]
    fn a_new_question_cancels_and_drops_the_background_request() {
        let (mut m, t) = manager(DismissPolicy::FinishInBackground);
        let old = m.open(t).id;
        m.submit("first").unwrap();
        m.dismiss(t);

        m.open(t);
        let submitted = m.submit("second").unwrap();
        assert_eq!(submitted.effects, [Effect::CancelRequest(old)]);
        assert!(m.detached().is_none());
        assert_eq!(streaming_sessions(&m), 1);
        assert!(!m.push_chunk(old, "late chunk"), "late chunks are ignored");
    }

    #[test]
    fn cancel_on_dismiss_aborts_the_request_and_keeps_nothing() {
        let (mut m, t) = manager(DismissPolicy::CancelOnDismiss);
        let id = m.open(t).id;
        m.submit("question").unwrap();
        assert_eq!(m.dismiss(t), [Effect::CancelRequest(id)]);
        assert!(m.active().is_none());
        assert!(m.detached().is_none());
        assert!(!m.push_chunk(id, "late chunk"));
    }

    #[test]
    fn stop_cancels_keeps_the_partial_answer_and_ignores_late_chunks() {
        let (mut m, t) = manager(DismissPolicy::default());
        let id = m.open(t).id;
        m.submit("question").unwrap();
        m.push_chunk(id, "partial");

        assert_eq!(m.stop(), [Effect::CancelRequest(id)]);
        assert_eq!(m.active().unwrap().phase(), Phase::Cancelled);
        assert!(!m.push_chunk(id, " more"));
        assert_eq!(m.active().unwrap().messages()[1].text, "partial");
        assert!(m.stop().is_empty(), "stopping twice is a no-op");
        // After a stop the user can ask again in the same session.
        assert!(m.submit("retry").is_ok());
    }

    #[test]
    fn background_answer_expires_after_the_ttl() {
        let (mut m, t) = manager(DismissPolicy::FinishInBackground);
        let id = m.open(t).id;
        m.submit("question").unwrap();
        m.dismiss(t);

        assert!(m.expire(t + TTL - Duration::from_secs(1)).is_empty());
        assert!(m.detached().is_some());
        assert_eq!(m.expire(t + TTL), [Effect::CancelRequest(id)]);
        assert!(m.detached().is_none());
    }

    #[test]
    fn expired_finished_answer_is_dropped_without_effects() {
        let (mut m, t) = manager(DismissPolicy::FinishInBackground);
        let id = m.open(t).id;
        m.submit("question").unwrap();
        m.dismiss(t);
        m.finish(id);

        let later = t + TTL;
        let reopened = m.open(later);
        assert!(reopened.effects.is_empty());
        assert!(!reopened.detached_available);
        assert!(m.detached().is_none());
    }

    #[test]
    fn expired_answers_cannot_be_resumed() {
        let (mut m, t) = manager(DismissPolicy::FinishInBackground);
        let id = m.open(t).id;
        m.submit("question").unwrap();
        m.dismiss(t);
        m.finish(id);
        m.open(t);
        assert_eq!(m.resume_detached(t + TTL), None);
    }

    #[test]
    fn resume_is_refused_when_the_visible_session_has_content() {
        let (mut m, t) = manager(DismissPolicy::FinishInBackground);
        let old = m.open(t).id;
        m.submit("first").unwrap();
        m.dismiss(t);
        m.finish(old);

        m.open(t);
        m.submit("second").unwrap();
        assert_eq!(m.resume_detached(t), None);
    }

    #[test]
    fn opening_twice_without_dismissal_applies_the_policy_to_the_first() {
        let (mut m, t) = manager(DismissPolicy::FinishInBackground);
        let first = m.open(t).id;
        m.submit("question").unwrap();
        let second = m.open(t);
        assert_ne!(first, second.id);
        assert!(second.detached_available);
        assert_eq!(m.detached().unwrap().id(), first);
    }

    #[test]
    fn chunks_for_unknown_sessions_are_ignored() {
        let (mut m, t) = manager(DismissPolicy::default());
        m.open(t);
        assert!(!m.push_chunk(999, "x"));
        assert!(!m.finish(999));
        assert!(!m.fail(999));
    }

    #[test]
    fn failure_ends_the_request_but_keeps_the_session() {
        let (mut m, t) = manager(DismissPolicy::default());
        let id = m.open(t).id;
        m.submit("question").unwrap();
        assert!(m.fail(id));
        assert_eq!(m.active().unwrap().phase(), Phase::Failed);
        assert!(m.submit("retry").is_ok());
    }
}
