//! The QObject that connects the QML popup to the Rust core.
//!
//! It owns no logic of its own: `ViewModel` decides what is shown, the
//! `Orchestrator` does the work, and this object mirrors the state into QML
//! properties and forwards the user's actions.

// The cxx-qt bridge expands to unsafe code, and its module accepts no attributes,
// so the allowance is scoped to this one module. The rest of the crate keeps
// `deny(unsafe_code)`, and this file's own code contains no `unsafe`.
#![allow(unsafe_code)]

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, question)]
        #[qproperty(QString, answer)]
        #[qproperty(QString, error_text, cxx_name = "errorText")]
        #[qproperty(QString, status)]
        #[qproperty(QString, provider_label, cxx_name = "providerLabel")]
        #[qproperty(QString, notice)]
        #[qproperty(bool, background_answer, cxx_name = "backgroundAnswer")]
        #[qproperty(bool, start_visible, cxx_name = "startVisible")]
        #[qproperty(bool, exit_on_dismiss, cxx_name = "exitOnDismiss")]
        type PopupController = super::PopupControllerRust;

        /// The popup was shown: start a fresh session.
        #[qinvokable]
        fn open(self: Pin<&mut PopupController>);

        /// The popup was dismissed. `reason` is a short label for the log, such as
        /// "escape" or "focus lost".
        #[qinvokable]
        fn dismiss(self: Pin<&mut PopupController>, reason: &QString);

        #[qinvokable]
        fn submit(self: Pin<&mut PopupController>, text: &QString);

        #[qinvokable]
        fn stop(self: Pin<&mut PopupController>);

        /// Bring back an answer that finished in the background.
        #[qinvokable]
        fn resume(self: Pin<&mut PopupController>);

        /// Log a UI timing event. Only a fixed set of labels is accepted, so no
        /// content can reach the log through this door.
        #[qinvokable]
        fn trace(self: Pin<&mut PopupController>, event: &QString);

        /// A later invocation asked for something: "toggle", "show", "hide" or "quit".
        #[qsignal]
        fn command(self: Pin<&mut PopupController>, name: QString);
    }

    impl cxx_qt::Threading for PopupController {}
    impl cxx_qt::Initialize for PopupController {}
}

use core::pin::Pin;

use cxx_qt::{CxxQtType, Initialize, Threading};
use cxx_qt_lib::QString;
use popupai_core::orchestrator::Event;
use popupai_platform::Command;
use popupai_platform::shortcut::ShortcutStatus;

use crate::context;
use crate::shortcut_notice;
use crate::view_model::ViewModel;

#[derive(Default)]
pub struct PopupControllerRust {
    question: QString,
    answer: QString,
    error_text: QString,
    status: QString,
    provider_label: QString,
    notice: QString,
    /// Why the configuration could not be used, if it could not.
    config_notice: String,
    /// What the portal said about the global shortcut.
    shortcut_notice: String,
    background_answer: bool,
    start_visible: bool,
    exit_on_dismiss: bool,
    model: ViewModel,
}

impl Initialize for qobject::PopupController {
    fn initialize(mut self: Pin<&mut Self>) {
        let Some(app) = context::get() else {
            tracing::error!(
                "the application context was not installed before the QML engine started"
            );
            return;
        };
        self.as_mut().set_provider_label(QString::from(
            app.orchestrator
                .provider_label()
                .unwrap_or_default()
                .as_str(),
        ));
        self.as_mut().rust_mut().get_mut().config_notice = app.notice.clone();
        self.as_mut().refresh_notice();
        self.as_mut().set_start_visible(app.start_visible);
        self.as_mut().set_exit_on_dismiss(app.exit_on_dismiss);
        let status = QString::from(self.model_status());
        self.as_mut().set_status(status);

        // Commands from later invocations reach QML as a signal, on the Qt thread.
        if let Some(mut commands) = app.take_commands() {
            let qt_thread = self.qt_thread();
            app.runtime.spawn(async move {
                while let Some(command) = commands.recv().await {
                    let name = match command {
                        Command::Toggle => "toggle",
                        Command::Show => "show",
                        Command::Hide => "hide",
                        Command::Quit => "quit",
                    };
                    tracing::info!(command = %name, "remote command received");
                    if qt_thread
                        .queue(move |controller| controller.command(QString::from(name)))
                        .is_err()
                    {
                        break; // the QObject is gone
                    }
                }
            });
        }

        // The global shortcut reports whether the portal accepted it.
        if let Some(mut statuses) = app.take_shortcut() {
            let qt_thread = self.qt_thread();
            app.runtime.spawn(async move {
                while let Some(status) = statuses.recv().await {
                    match &status {
                        ShortcutStatus::Active {
                            trigger,
                            registration,
                        } => {
                            tracing::info!(%trigger, "global shortcut active");
                            if let Some(note) = registration {
                                tracing::warn!(%note, "application registration with the portal");
                            }
                        }
                        ShortcutStatus::Unassigned => {
                            tracing::warn!("global shortcut has no key assigned");
                        }
                        ShortcutStatus::Unavailable { reason } => {
                            tracing::warn!(%reason, "global shortcut unavailable");
                        }
                    }
                    let text = shortcut_notice::notice(&status);
                    if qt_thread
                        .queue(move |mut controller| {
                            controller.as_mut().rust_mut().get_mut().shortcut_notice = text;
                            controller.refresh_notice();
                        })
                        .is_err()
                    {
                        break; // the QObject is gone
                    }
                }
            });
        }

        // Deliver orchestrator events to the Qt thread, where the QObject lives.
        let Some(mut events) = app.take_events() else {
            return;
        };
        let qt_thread = self.qt_thread();
        app.runtime.spawn(async move {
            while let Some(event) = events.recv().await {
                if qt_thread
                    .queue(move |controller| controller.on_event(&event))
                    .is_err()
                {
                    break; // the QObject is gone
                }
            }
        });
    }
}

impl qobject::PopupController {
    fn model_status(&self) -> &'static str {
        self.rust().model.status.as_str()
    }

    /// Show the configuration and shortcut problems together.
    fn refresh_notice(mut self: Pin<&mut Self>) {
        let text = {
            let rust = self.rust();
            [rust.config_notice.as_str(), rust.shortcut_notice.as_str()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.as_mut().set_notice(QString::from(text.as_str()));
    }

    /// Mirror the view model into the QML properties.
    fn sync(mut self: Pin<&mut Self>) {
        let (question, answer, error, status) = {
            let model = &self.rust().model;
            (
                QString::from(model.question.as_str()),
                QString::from(model.answer.as_str()),
                QString::from(model.error.as_str()),
                QString::from(model.status.as_str()),
            )
        };
        self.as_mut().set_question(question);
        self.as_mut().set_answer(answer);
        self.as_mut().set_error_text(error);
        self.as_mut().set_status(status);
    }

    fn on_event(mut self: Pin<&mut Self>, event: &Event) {
        let changed = self.as_mut().rust_mut().get_mut().model.apply(event);
        if changed {
            self.sync();
        }
    }

    fn open(mut self: Pin<&mut Self>) {
        let Some(app) = context::get() else {
            return;
        };
        let opening = app.orchestrator.open();
        tracing::info!(
            session = opening.session,
            background_answer = opening.background_answer,
            "popup opened"
        );
        self.as_mut()
            .rust_mut()
            .get_mut()
            .model
            .begin(opening.session);
        self.as_mut()
            .set_background_answer(opening.background_answer);
        self.sync();
    }

    fn trace(self: Pin<&mut Self>, event: &QString) {
        const LABELS: [&str; 4] = ["visible", "first frame", "active", "inactive"];
        let event = event.to_string();
        if let Some(label) = LABELS.iter().find(|label| **label == event) {
            tracing::info!(event = %label, "ui");
        }
    }

    fn dismiss(self: Pin<&mut Self>, reason: &QString) {
        if let Some(app) = context::get() {
            app.orchestrator.dismiss();
            tracing::info!(%reason, "popup dismissed");
        }
    }

    fn submit(mut self: Pin<&mut Self>, text: &QString) {
        let Some(app) = context::get() else {
            return;
        };
        let text = text.to_string();
        match app.orchestrator.submit(&text) {
            Ok(session) => {
                self.as_mut().rust_mut().get_mut().model.ask(session, &text);
                self.as_mut().set_background_answer(false);
            }
            Err(err) => {
                self.as_mut()
                    .rust_mut()
                    .get_mut()
                    .model
                    .fail_locally(&err.to_string());
            }
        }
        self.sync();
    }

    fn stop(mut self: Pin<&mut Self>) {
        let Some(app) = context::get() else {
            return;
        };
        app.orchestrator.stop();
        self.as_mut().rust_mut().get_mut().model.stop();
        self.sync();
    }

    fn resume(mut self: Pin<&mut Self>) {
        let Some(app) = context::get() else {
            return;
        };
        if app.orchestrator.resume_detached().is_none() {
            return;
        }
        if let Some(view) = app.orchestrator.view() {
            self.as_mut().rust_mut().get_mut().model.restore(&view);
            self.as_mut().set_background_answer(false);
            self.sync();
        }
    }
}
