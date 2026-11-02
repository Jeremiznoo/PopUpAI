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
    }

    impl cxx_qt::Threading for PopupController {}
    impl cxx_qt::Initialize for PopupController {}
}

use core::pin::Pin;

use cxx_qt::{CxxQtType, Initialize, Threading};
use cxx_qt_lib::QString;
use popupai_core::orchestrator::Event;

use crate::context;
use crate::view_model::ViewModel;

#[derive(Default)]
pub struct PopupControllerRust {
    question: QString,
    answer: QString,
    error_text: QString,
    status: QString,
    provider_label: QString,
    notice: QString,
    background_answer: bool,
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
        self.as_mut().set_notice(QString::from(app.notice.as_str()));
        let status = QString::from(self.model_status());
        self.as_mut().set_status(status);

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
