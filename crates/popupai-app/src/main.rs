mod cli;
mod config_path;
mod context;
mod controller;
mod key_input;
mod mcp_setup;
mod picker;
mod settings_page;
mod shortcut_notice;
mod view_model;

use std::process::ExitCode;
use std::sync::Arc;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};
use popupai_core::config::{Config, LogLevel};
use popupai_core::orchestrator::{DefaultProviderFactory, Orchestrator};
use popupai_platform::shortcut::{Settings as ShortcutSettings, spawn as spawn_shortcut};
use popupai_platform::{Autostart, BUS_NAME, Bus, Claim, Command, SecretServiceStore};
use tokio::sync::mpsc::unbounded_channel;

use cli::Action;

const QML_ENTRY: &str = "qrc:/qt/qml/io/github/jeremiznoo/popupai/qml/Main.qml";

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

/// Make the autostart entry match the `launch_at_startup` setting.
fn sync_autostart(wanted: bool) {
    let Some(autostart) = Autostart::user() else {
        return;
    };
    if autostart.is_enabled() == wanted {
        return;
    }
    let result = if wanted {
        std::env::current_exe().and_then(|exe| autostart.enable(&exe))
    } else {
        autostart.disable()
    };
    match result {
        Ok(()) => tracing::info!(enabled = wanted, "autostart entry updated"),
        Err(err) => tracing::warn!("could not update the autostart entry: {err}"),
    }
}

fn main() -> ExitCode {
    cxx_qt::init_crate!(cxx_qt_lib);
    cxx_qt::init_qml_module!("io.github.jeremiznoo.popupai");

    let action = match cli::parse(std::env::args().skip(1)) {
        Ok(Action::Help) => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Ok(action) => action,
        Err(message) => {
            eprintln!("popupai: {message}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("popupai: could not start the async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    // The orchestrator spawns tasks from the Qt thread, so that thread must be
    // inside the runtime for as long as the application runs.
    let _enter = runtime.enter();

    // One instance only: the first process keeps running, later ones hand it their
    // command and exit.
    let claim = runtime.block_on(popupai_platform::claim(&Bus::Session));
    let (instance, commands) = match claim {
        Ok(Claim::Primary { instance, commands }) => (Some(instance), Some(commands)),
        Ok(Claim::Existing(existing)) => {
            let command = match action {
                Action::Run { .. } => Command::Show,
                Action::Toggle => Command::Toggle,
                Action::Quit => Command::Quit,
                Action::Help => return ExitCode::SUCCESS,
            };
            return match runtime.block_on(existing.send(command)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("popupai: {err}");
                    ExitCode::FAILURE
                }
            };
        }
        Err(err) => {
            if matches!(action, Action::Toggle | Action::Quit) {
                eprintln!("popupai: {err}");
                return ExitCode::FAILURE;
            }
            // Without the session bus the popup still works, but cannot be
            // controlled from outside. Say so instead of pretending otherwise.
            eprintln!(
                "popupai: {err}. Starting without remote control: `popupai --toggle` will not work."
            );
            (None, None)
        }
    };
    let has_instance = instance.is_some();
    let _instance = instance;
    if action == Action::Quit {
        eprintln!("popupai: PopUpAI is not running.");
        return ExitCode::FAILURE;
    }
    let start_visible = match action {
        Action::Run { visible } => visible,
        _ => true, // `--toggle` with nothing running starts and shows
    };

    // The configuration comes next: its log level decides what gets printed.
    let path = config_path::config_path();
    let state_path = path.with_file_name("state.toml");
    let (config, notice) = match Config::load(&path) {
        Ok(config) => (config.with_missing_builtins(), String::new()),
        Err(err) => (Config::default(), format!("Configuration problem: {err}")),
    };
    // The settings file is only written back when it was read successfully, so a broken
    // file is never overwritten with defaults.
    let config_writable = notice.is_empty();
    init_logging(config.logging.level);
    tracing::info!(config = %path.display(), providers = config.providers.len(), "starting");
    if has_instance {
        tracing::info!(name = BUS_NAME, "claimed the instance name");
    }
    if !notice.is_empty() {
        tracing::warn!("{notice}");
    }

    // Start with the session when the settings say so. Only done when the settings
    // were read successfully, so a broken file never removes the entry by accident.
    if notice.is_empty() && has_instance {
        sync_autostart(config.general.launch_at_startup);
    }

    // API keys live in the system secret store (Secret Service: KDE Wallet, GNOME
    // Keyring, KeePassXC). If none is running, providers that need a key say so, and
    // keyless local providers such as Ollama still work.
    let store = Arc::new(SecretServiceStore::new());

    // Commands come from two places: later invocations (D-Bus) and the global
    // shortcut (the desktop portal). They meet in one channel.
    let shortcut_chord = config.shortcut();
    // Keeps the tray icon for as long as the application runs.
    let mut tray = None;
    let (configure_tx, configure_rx) = unbounded_channel();
    let mut shortcut_configure = None;
    let (commands, shortcut) = match commands {
        Some(mut remote) => {
            let (merged, merged_rx) = unbounded_channel();
            let forward = merged.clone();
            runtime.spawn(async move {
                while let Some(command) = remote.recv().await {
                    if forward.send(command).is_err() {
                        break;
                    }
                }
            });
            if std::env::var_os("POPUPAI_NO_TRAY").is_some_and(|v| v == "1") {
                tracing::info!("tray icon disabled by POPUPAI_NO_TRAY");
            } else {
                match runtime.block_on(popupai_platform::tray::spawn(merged.clone())) {
                    Ok(handle) => {
                        tracing::info!("tray icon shown");
                        tray = Some(handle);
                    }
                    // Not every desktop has a tray: the shortcut and the command still work.
                    Err(err) => tracing::warn!("{err}"),
                }
            }
            let shortcut = if std::env::var_os("POPUPAI_NO_SHORTCUT").is_some_and(|v| v == "1") {
                tracing::info!("global shortcut disabled by POPUPAI_NO_SHORTCUT");
                None
            } else {
                match shortcut_chord {
                    Ok(chord) => {
                        let (status_tx, status_rx) = unbounded_channel();
                        spawn_shortcut(
                            Bus::Session,
                            ShortcutSettings {
                                app_id: BUS_NAME.to_owned(),
                                chord,
                            },
                            merged,
                            status_tx,
                            configure_rx,
                        );
                        shortcut_configure = Some(configure_tx);
                        Some(status_rx)
                    }
                    Err(err) => {
                        tracing::error!("the configured shortcut is not usable: {err}");
                        None
                    }
                }
            };
            (Some(merged_rx), shortcut)
        }
        None => (None, None),
    };
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
    // Start on the model used last time; without one, on the first provider that can
    // work now.
    if let Some(last) = popupai_core::state::UserState::load(&state_path)
        .recent
        .first()
    {
        orchestrator.select_model(&last.provider, &last.model);
    }
    runtime.block_on(orchestrator.auto_select());
    // Only the MCP servers the user switched on are ever started.
    orchestrator.set_mcp_servers(mcp_setup::enabled(
        &mcp_setup::detect(),
        &popupai_core::state::UserState::load(&state_path),
    ));
    tracing::info!(
        provider = orchestrator
            .current_provider()
            .map_or("none".to_owned(), |p| p.id),
        "provider selected"
    );
    context::install(context::AppContext::new(context::Parts {
        orchestrator,
        runtime: runtime.handle().clone(),
        notice,
        state_path,
        config_path: config_writable.then_some(path),
        start_visible,
        exit_on_dismiss: std::env::var_os("POPUPAI_EXIT_ON_DISMISS").is_some_and(|v| v == "1"),
        events,
        commands,
        shortcut,
        shortcut_configure,
    }));

    let _tray = tray;

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
