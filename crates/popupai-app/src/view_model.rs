//! What the popup shows, kept free of Qt so the rules can be unit-tested.
//!
//! The QObject in `controller.rs` only mirrors this state into QML properties.

use std::fmt;

use popupai_core::orchestrator::{Event, SessionView};
use popupai_core::session::{Role, SessionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// Open, nothing asked yet.
    #[default]
    Idle,
    Streaming,
    Done,
    Failed,
    /// Stopped by the user; the partial answer is kept.
    Stopped,
}

impl Status {
    /// The value QML compares against.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Streaming => "streaming",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Default)]
pub struct ViewModel {
    session: Option<SessionId>,
    pub question: String,
    pub answer: String,
    pub error: String,
    pub status: Status,
}

/// The question and the answer are private: `Debug` shows only their sizes.
impl fmt::Debug for ViewModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewModel")
            .field("session", &self.session)
            .field("status", &self.status)
            .field("question_bytes", &self.question.len())
            .field("answer_bytes", &self.answer.len())
            .finish()
    }
}

impl ViewModel {
    /// The popup was opened: a fresh, empty view for a new session.
    pub fn begin(&mut self, session: SessionId) {
        *self = Self {
            session: Some(session),
            ..Self::default()
        };
    }

    /// A question was sent in the current session.
    pub fn ask(&mut self, session: SessionId, question: &str) {
        self.session = Some(session);
        self.question = question.trim().to_owned();
        self.answer.clear();
        self.error.clear();
        self.status = Status::Streaming;
    }

    /// The question could not even be sent (for example it was empty).
    pub fn fail_locally(&mut self, message: &str) {
        self.error = message.to_owned();
        self.status = Status::Failed;
    }

    /// The user pressed Stop.
    pub fn stop(&mut self) {
        if self.status == Status::Streaming {
            self.status = Status::Stopped;
        }
    }

    /// Apply an orchestrator event. Returns whether anything changed; events for
    /// another session (a background answer, a stale request) are ignored.
    pub fn apply(&mut self, event: &Event) -> bool {
        match event {
            Event::Chunk { session, text } if self.is_current(*session) => {
                if self.status != Status::Streaming {
                    return false; // late text after Stop
                }
                self.answer.push_str(text);
                true
            }
            Event::Finished { session } if self.is_current(*session) => {
                if self.status != Status::Streaming {
                    return false;
                }
                self.status = Status::Done;
                true
            }
            Event::Failed { session, message } if self.is_current(*session) => {
                if self.status != Status::Streaming {
                    return false;
                }
                self.error = message.clone();
                self.status = Status::Failed;
                true
            }
            _ => false,
        }
    }

    /// Show a recalled background answer (`Ctrl+R`).
    pub fn restore(&mut self, view: &SessionView) {
        use popupai_core::session::Phase;
        self.session = Some(view.id);
        self.question = view
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.text.clone())
            .unwrap_or_default();
        self.answer = view
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.text.clone())
            .unwrap_or_default();
        self.error.clear();
        self.status = match view.phase {
            Phase::Idle => Status::Idle,
            Phase::Streaming => Status::Streaming,
            Phase::Complete => Status::Done,
            Phase::Cancelled => Status::Stopped,
            Phase::Failed => Status::Failed,
        };
    }

    fn is_current(&self, session: SessionId) -> bool {
        self.session == Some(session)
    }
}

#[cfg(test)]
mod tests {
    use popupai_core::orchestrator::SessionView;
    use popupai_core::session::{Message, Phase};

    use super::*;

    fn chunk(session: SessionId, text: &str) -> Event {
        Event::Chunk {
            session,
            text: text.into(),
        }
    }

    #[test]
    fn a_new_opening_starts_empty_whatever_was_shown_before() {
        let mut vm = ViewModel::default();
        vm.begin(1);
        vm.ask(1, "What is DNS?");
        vm.apply(&chunk(1, "A naming system."));

        vm.begin(2);
        assert_eq!(vm.question, "");
        assert_eq!(vm.answer, "");
        assert_eq!(vm.error, "");
        assert_eq!(vm.status, Status::Idle);
    }

    #[test]
    fn chunks_accumulate_and_finish_marks_the_answer_done() {
        let mut vm = ViewModel::default();
        vm.begin(1);
        vm.ask(1, "  question  ");
        assert_eq!(vm.question, "question");
        assert_eq!(vm.status, Status::Streaming);

        assert!(vm.apply(&chunk(1, "Hel")));
        assert!(vm.apply(&chunk(1, "lo")));
        assert!(vm.apply(&Event::Finished { session: 1 }));
        assert_eq!(vm.answer, "Hello");
        assert_eq!(vm.status, Status::Done);
    }

    #[test]
    fn events_for_other_sessions_are_ignored() {
        let mut vm = ViewModel::default();
        vm.begin(2);
        vm.ask(2, "q");
        assert!(!vm.apply(&chunk(1, "from a background answer")));
        assert!(!vm.apply(&Event::Finished { session: 1 }));
        assert!(!vm.apply(&Event::Failed {
            session: 1,
            message: "x".into()
        }));
        assert_eq!(vm.answer, "");
        assert_eq!(vm.status, Status::Streaming);
    }

    #[test]
    fn stop_keeps_the_partial_answer_and_ignores_late_text() {
        let mut vm = ViewModel::default();
        vm.begin(1);
        vm.ask(1, "q");
        vm.apply(&chunk(1, "part"));

        vm.stop();

        assert_eq!(vm.status, Status::Stopped);
        assert!(!vm.apply(&chunk(1, "late")));
        assert!(!vm.apply(&Event::Finished { session: 1 }));
        assert_eq!(vm.answer, "part");
    }

    #[test]
    fn failures_show_their_message_and_keep_the_text_received_so_far() {
        let mut vm = ViewModel::default();
        vm.begin(1);
        vm.ask(1, "q");
        vm.apply(&chunk(1, "partial"));
        assert!(vm.apply(&Event::Failed {
            session: 1,
            message: "Could not reach Ollama.".into()
        }));
        assert_eq!(vm.status, Status::Failed);
        assert_eq!(vm.error, "Could not reach Ollama.");
        assert_eq!(vm.answer, "partial");
    }

    #[test]
    fn asking_again_replaces_the_previous_exchange() {
        let mut vm = ViewModel::default();
        vm.begin(1);
        vm.ask(1, "first");
        vm.apply(&chunk(1, "answer one"));
        vm.apply(&Event::Finished { session: 1 });

        vm.ask(1, "second");

        assert_eq!((vm.question.as_str(), vm.answer.as_str()), ("second", ""));
        assert_eq!(vm.status, Status::Streaming);
    }

    #[test]
    fn a_local_failure_is_shown_without_an_answer() {
        let mut vm = ViewModel::default();
        vm.begin(1);
        vm.fail_locally("a response is already being generated");
        assert_eq!(vm.status, Status::Failed);
        assert_eq!(vm.error, "a response is already being generated");
    }

    #[test]
    fn a_recalled_background_answer_is_restored_with_its_state() {
        let view = SessionView {
            id: 7,
            phase: Phase::Complete,
            messages: vec![
                Message {
                    role: Role::User,
                    text: "long question".into(),
                },
                Message {
                    role: Role::Assistant,
                    text: "long answer".into(),
                },
            ],
        };
        let mut vm = ViewModel::default();
        vm.begin(8);

        vm.restore(&view);

        assert_eq!(vm.question, "long question");
        assert_eq!(vm.answer, "long answer");
        assert_eq!(vm.status, Status::Done);
        assert!(
            !vm.apply(&chunk(7, "late")),
            "a finished answer ignores late text"
        );
        assert_eq!(vm.answer, "long answer");
    }

    #[test]
    fn a_recalled_answer_still_being_generated_keeps_receiving_text() {
        let view = SessionView {
            id: 7,
            phase: Phase::Streaming,
            messages: vec![
                Message {
                    role: Role::User,
                    text: "long question".into(),
                },
                Message {
                    role: Role::Assistant,
                    text: "so far".into(),
                },
            ],
        };
        let mut vm = ViewModel::default();
        vm.begin(8);

        vm.restore(&view);

        assert_eq!(vm.status, Status::Streaming);
        assert!(
            vm.apply(&chunk(7, ", and more")),
            "the restored session is the current one"
        );
        assert_eq!(vm.answer, "so far, and more");
        assert!(
            !vm.apply(&chunk(8, "from the opening that was replaced")),
            "the previous opening is no longer current"
        );
    }

    #[test]
    fn debug_output_never_contains_the_conversation() {
        let mut vm = ViewModel::default();
        vm.begin(1);
        vm.ask(1, "a very private question");
        vm.apply(&chunk(1, "a very private answer"));
        let shown = format!("{vm:?}");
        assert!(!shown.contains("private"), "{shown}");
    }
}
