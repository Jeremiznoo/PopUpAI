mod config_path;
mod context;
mod controller;
mod view_model;

use std::process::ExitCode;
use std::sync::Arc;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};
use popupai_core::config::{Config, LogLevel};
use popupai_core::orchestrator::{DefaultProviderFactory, Orchestrator};
use popupai_security::UnavailableStore;

const QML_ENTRY: &str = "qrc:/qt/qml/io/github/popupai/qml/Main.qml";

fn init_logging(level: LogLevel) {
    let level = match level {
        LogLevel::Error => tracing::Level::ERROR,
        LogLevel::Warn => tracing::Level::WARN,
        LogLevel::Info => tracing::Level::INFO,
        LogLevel::Debug => tracing::Level::DEBUG,
    };
    // Logs go to stderr. They carry metadata only, never prompts, answers or keys.
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}

fn main() -> ExitCode {
    cxx_qt::init_crate!(cxx_qt_lib);
    cxx_qt::init_qml_module!("io.github.popupai");

    // The configuration comes first: its log level decides what gets printed.
    let path = config_path::config_path();
    let (config, notice) = match Config::load(&path) {
        Ok(config) => (config, String::new()),
        Err(err) => (Config::default(), format!("Configuration problem: {err}")),
    };
    init_logging(config.logging.level);
    tracing::info!(config = %path.display(), providers = config.providers.len(), "starting");
    if !notice.is_empty() {
        tracing::warn!("{notice}");
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            tracing::error!("could not start the async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    // The orchestrator spawns tasks from the Qt thread, so that thread must be
    // inside the runtime for as long as the application runs.
    let _enter = runtime.enter();

    // The system secret store is not implemented yet. Providers that need a key
    // report that clearly; keyless local providers such as Ollama still work.
    let store = Arc::new(UnavailableStore::new(
        "the system secret store is not implemented yet",
    ));
    let (orchestrator, events) =
        match Orchestrator::new(config, store, Arc::new(DefaultProviderFactory)) {
            Ok(pair) => pair,
            Err(issues) => {
                for issue in issues {
                    tracing::error!("invalid configuration: {issue}");
                }
                return ExitCode::FAILURE;
            }
        };
    context::install(context::AppContext::new(
        orchestrator,
        runtime.handle().clone(),
        notice,
        events,
    ));

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        engine
            .as_mut()
            .on_object_creation_failed(|_, url| {
                // Without a window the application would wait forever.
                tracing::error!("the QML interface failed to load: {url}");
                std::process::exit(1);
            })
            .release();
        engine.load(&QUrl::from(QML_ENTRY));
    }
    let code = app.as_mut().map_or(1, |app| app.exec());
    tracing::info!(code, "exiting");
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
