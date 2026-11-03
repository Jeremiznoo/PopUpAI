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
        #[qproperty(QString, answer_html, cxx_name = "answerHtml")]
        #[qproperty(QString, error_text, cxx_name = "errorText")]
        #[qproperty(QString, status)]
        #[qproperty(QString, provider_label, cxx_name = "providerLabel")]
        #[qproperty(QString, notice)]
        #[qproperty(bool, background_answer, cxx_name = "backgroundAnswer")]
        #[qproperty(bool, needs_key, cxx_name = "needsKey")]
        #[qproperty(bool, can_configure_shortcut, cxx_name = "canConfigureShortcut")]
        #[qproperty(QString, models_json, cxx_name = "modelsJson")]
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

        /// Open the desktop's own shortcut settings, to assign the global shortcut.
        #[qinvokable]
        #[cxx_name = "configureShortcut"]
        fn configure_shortcut(self: Pin<&mut PopupController>);

        /// Fill `modelsJson` with the models that can be picked.
        #[qinvokable]
        #[cxx_name = "loadModels"]
        fn load_models(self: Pin<&mut PopupController>);

        /// Use this model for the next questions.
        #[qinvokable]
        #[cxx_name = "chooseModel"]
        fn choose_model(self: Pin<&mut PopupController>, provider: &QString, model: &QString);

        /// Mark or unmark a model as a favourite.
        #[qinvokable]
        #[cxx_name = "toggleFavorite"]
        fn toggle_favorite(self: Pin<&mut PopupController>, provider: &QString, model: &QString);

        /// Store the API key pasted for the current provider in the system secret
        /// store. The caller clears its own copy afterwards.
        #[qinvokable]
        #[cxx_name = "saveKey"]
        fn save_key(self: Pin<&mut PopupController>, key: &QString);

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
use popupai_core::markdown;
use popupai_core::orchestrator::Event;
use popupai_core::state::{ModelRef, UserState};
use popupai_platform::Command;
use popupai_platform::shortcut::ShortcutStatus;
use popupai_security::Secret;
use zeroize::Zeroizing;

use crate::context;
use crate::key_input;
use crate::picker;
use crate::shortcut_notice;
use crate::view_model::ViewModel;

#[derive(Default)]
pub struct PopupControllerRust {
    question: QString,
    answer: QString,
    answer_html: QString,
    error_text: QString,
    status: QString,
    provider_label: QString,
    notice: QString,
    /// Why the configuration could not be used, if it could not.
    config_notice: String,
    /// What the portal said about the global shortcut.
    shortcut_notice: String,
    background_answer: bool,
    needs_key: bool,
    can_configure_shortcut: bool,
    models_json: QString,
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
        self.as_mut().refresh_needs_key();

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
                        ShortcutStatus::ConfigureFailed { reason } => {
                            tracing::warn!(%reason, "shortcut settings could not be opened");
                        }
                    }
                    let text = shortcut_notice::notice(&status);
                    let configurable = shortcut_notice::offers_configuration(&status);
                    if qt_thread
                        .queue(move |mut controller| {
                            controller.as_mut().rust_mut().get_mut().shortcut_notice = text;
                            controller.as_mut().set_can_configure_shortcut(configurable);
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
        let (question, answer, html, error, status) = {
            let model = &self.rust().model;
            (
                QString::from(model.question.as_str()),
                QString::from(model.answer.as_str()),
                QString::from(markdown::render(&model.answer).as_str()),
                QString::from(model.error.as_str()),
                QString::from(model.status.as_str()),
            )
        };
        self.as_mut().set_question(question);
        self.as_mut().set_answer(answer);
        self.as_mut().set_answer_html(html);
        self.as_mut().set_error_text(error);
        self.as_mut().set_status(status);
    }

    fn on_event(mut self: Pin<&mut Self>, event: &Event) {
        let changed = self.as_mut().rust_mut().get_mut().model.apply(event);
        if changed {
            self.as_mut().sync();
        }
        // A failure may mean the key was refused: ask again whether a new one is needed.
        if matches!(event, Event::Failed { .. }) {
            self.refresh_needs_key();
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
        self.as_mut().refresh_needs_key();
        self.sync();
    }

    /// Ask the secret store whether the current provider still lacks its key, off
    /// the Qt thread (the store may have to prompt to unlock the wallet).
    fn refresh_needs_key(self: Pin<&mut Self>) {
        let Some(app) = context::get() else {
            return;
        };
        let orchestrator = app.orchestrator.clone();
        let qt_thread = self.qt_thread();
        app.runtime.spawn(async move {
            let needs_key = orchestrator.current_needs_key().await;
            let _ =
                qt_thread.queue(move |mut controller| controller.as_mut().set_needs_key(needs_key));
        });
    }

    fn configure_shortcut(self: Pin<&mut Self>) {
        if let Some(configure) = context::get().and_then(|app| app.shortcut_configure.as_ref()) {
            tracing::info!("opening the desktop shortcut settings");
            let _ = configure.send(());
        }
    }

    /// Build the picker list off the Qt thread (a local server may take a moment).
    fn load_models(self: Pin<&mut Self>) {
        let Some(app) = context::get() else {
            return;
        };
        let orchestrator = app.orchestrator.clone();
        let state_path = app.state_path.clone();
        let qt_thread = self.qt_thread();
        app.runtime.spawn(async move {
            let choices = orchestrator.available_models().await;
            let state = UserState::load(&state_path);
            let current = orchestrator
                .current_provider()
                .map(|info| ModelRef::new(info.id, info.model));
            let json = picker::to_json(&picker::build(&choices, &state, current.as_ref()));
            let _ = qt_thread.queue(move |mut controller| {
                controller
                    .as_mut()
                    .set_models_json(QString::from(json.as_str()));
            });
        });
    }

    fn choose_model(mut self: Pin<&mut Self>, provider: &QString, model: &QString) {
        let Some(app) = context::get() else {
            return;
        };
        let (provider, model) = (provider.to_string(), model.to_string());
        if !app.orchestrator.select_model(&provider, &model) {
            return;
        }
        tracing::info!(provider = %provider, "model selected");
        if let Some(info) = app.orchestrator.current_provider() {
            self.as_mut()
                .set_provider_label(QString::from(info.label.as_str()));
        }
        // Remember it (it is also what the application starts on next time).
        let path = app.state_path.clone();
        app.runtime.spawn_blocking(move || {
            let mut state = UserState::load(&path);
            state.touch(&ModelRef::new(provider, model));
            if let Err(err) = state.save(&path) {
                tracing::warn!("could not remember the model choice: {err}");
            }
        });
        self.as_mut().refresh_needs_key();
    }

    fn toggle_favorite(self: Pin<&mut Self>, provider: &QString, model: &QString) {
        let Some(app) = context::get() else {
            return;
        };
        let wanted = ModelRef::new(provider.to_string(), model.to_string());
        let path = app.state_path.clone();
        let qt_thread = self.qt_thread();
        let orchestrator = app.orchestrator.clone();
        app.runtime.spawn(async move {
            let saved = {
                let path = path.clone();
                tokio::task::spawn_blocking(move || {
                    let mut state = UserState::load(&path);
                    state.toggle_favorite(&wanted);
                    state.save(&path)
                })
                .await
            };
            if !matches!(saved, Ok(Ok(()))) {
                tracing::warn!("could not save the favourite");
            }
            let choices = orchestrator.available_models().await;
            let state = UserState::load(&path);
            let current = orchestrator
                .current_provider()
                .map(|info| ModelRef::new(info.id, info.model));
            let json = picker::to_json(&picker::build(&choices, &state, current.as_ref()));
            let _ = qt_thread.queue(move |mut controller| {
                controller
                    .as_mut()
                    .set_models_json(QString::from(json.as_str()));
            });
        });
    }

    fn save_key(mut self: Pin<&mut Self>, key: &QString) {
        let Some(app) = context::get() else {
            return;
        };
        // The pasted text is wiped from this copy once it has been handed over.
        let pasted = Zeroizing::new(key.to_string());
        let checked = match key_input::check(&pasted) {
            Ok(checked) => checked.to_owned(),
            Err(message) => {
                self.as_mut()
                    .rust_mut()
                    .get_mut()
                    .model
                    .fail_locally(message);
                self.sync();
                return;
            }
        };
        let Some(provider) = app.orchestrator.current_provider() else {
            return;
        };
        let secret = Secret::new(checked);
        let orchestrator = app.orchestrator.clone();
        let qt_thread = self.qt_thread();
        app.runtime.spawn(async move {
            let result = orchestrator.save_api_key(&provider.id, secret).await;
            let outcome = match &result {
                Ok(()) => {
                    tracing::info!(provider = %provider.id, "API key saved in the secret store");
                    Ok(())
                }
                Err(err) => {
                    tracing::warn!(provider = %provider.id, "API key could not be saved");
                    Err(format!("The key could not be saved: {err}."))
                }
            };
            let _ = qt_thread.queue(move |mut controller| match outcome {
                Ok(()) => {
                    controller.as_mut().rust_mut().get_mut().model.clear_error();
                    controller.as_mut().set_needs_key(false);
                    controller.sync();
                }
                Err(message) => {
                    controller
                        .as_mut()
                        .rust_mut()
                        .get_mut()
                        .model
                        .fail_locally(&message);
                    controller.sync();
                }
            });
        });
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
        if *self.needs_key() {
            return; // the key field is shown instead of the question field
        }
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
