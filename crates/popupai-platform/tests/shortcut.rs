//! The global shortcut client against a mock portal on a private bus.
//!
//! The mock follows the portal specification, not KDE's implementation: these tests
//! show that the client speaks the protocol and handles every kind of reply. Whether
//! KDE accepts the binding is established separately, on the real session.

mod support;

use std::time::Duration;

use popupai_core::shortcut::Chord;
use popupai_platform::shortcut::{SHORTCUT_ID, Settings, ShortcutStatus, spawn};
use popupai_platform::{Bus, Command};
use support::PrivateBus;
use support::portal::{Bind, MockPortal, Options};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::time::timeout;

const APP_ID: &str = "io.github.jeremiznoo.PopUpAI";

struct Run {
    status: UnboundedReceiver<ShortcutStatus>,
    commands: UnboundedReceiver<Command>,
    task: tokio::task::JoinHandle<()>,
}

fn settings() -> Settings {
    Settings {
        app_id: APP_ID.to_owned(),
        chord: "CTRL+SHIFT+SPACE".parse::<Chord>().unwrap(),
    }
}

fn start_client(bus: &PrivateBus) -> Run {
    let (command_tx, commands) = unbounded_channel();
    let (status_tx, status) = unbounded_channel();
    let task = spawn(
        Bus::Address(bus.address.clone()),
        settings(),
        command_tx,
        status_tx,
    );
    Run {
        status,
        commands,
        task,
    }
}

async fn next_status(run: &mut Run) -> ShortcutStatus {
    timeout(Duration::from_secs(5), run.status.recv())
        .await
        .expect("timed out waiting for a status")
        .expect("the status channel closed")
}

fn unavailable_reason(status: ShortcutStatus) -> String {
    match status {
        ShortcutStatus::Unavailable { reason } => reason,
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_shortcut_is_registered_bound_and_a_press_asks_for_a_toggle() {
    let bus = PrivateBus::start();
    let portal = MockPortal::start(&bus.address, Options::default()).await;
    let mut run = start_client(&bus);

    assert_eq!(
        next_status(&mut run).await,
        ShortcutStatus::Active {
            trigger: "Ctrl+Shift+Space".to_owned(),
            registration: None,
        }
    );
    assert_eq!(portal.registered_app_ids(), [APP_ID]);
    assert_eq!(
        portal.bound(),
        [(SHORTCUT_ID.to_owned(), Some("CTRL+SHIFT+space".to_owned()))],
        "the preferred trigger is sent in the portal format"
    );

    portal.activate(SHORTCUT_ID).await;
    let command = timeout(Duration::from_secs(5), run.commands.recv())
        .await
        .expect("timed out waiting for the toggle");
    assert_eq!(command, Some(Command::Toggle));

    // Another shortcut of the same session is not ours.
    portal.activate("something-else").await;
    assert!(
        timeout(Duration::from_millis(300), run.commands.recv())
            .await
            .is_err(),
        "an unrelated shortcut must not toggle the popup"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trigger_changed_in_the_system_settings_is_reported() {
    let bus = PrivateBus::start();
    let portal = MockPortal::start(&bus.address, Options::default()).await;
    let mut run = start_client(&bus);
    assert!(matches!(
        next_status(&mut run).await,
        ShortcutStatus::Active { .. }
    ));

    portal.change_trigger(SHORTCUT_ID, "Meta+A").await;
    assert_eq!(
        next_status(&mut run).await,
        ShortcutStatus::Active {
            trigger: "Meta+A".to_owned(),
            registration: None,
        }
    );

    portal.change_trigger(SHORTCUT_ID, "").await;
    assert_eq!(next_status(&mut run).await, ShortcutStatus::Unassigned);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shortcut_with_no_key_assigned_is_reported_as_such() {
    let bus = PrivateBus::start();
    let _portal = MockPortal::start(
        &bus.address,
        Options {
            bind: Bind::NoTrigger,
            ..Options::default()
        },
    )
    .await;
    let mut run = start_client(&bus);
    assert_eq!(next_status(&mut run).await, ShortcutStatus::Unassigned);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declined_request_is_reported_with_its_reason() {
    let bus = PrivateBus::start();
    let _portal = MockPortal::start(
        &bus.address,
        Options {
            bind: Bind::Cancelled,
            ..Options::default()
        },
    )
    .await;
    let mut run = start_client(&bus);
    let reason = unavailable_reason(next_status(&mut run).await);
    assert!(reason.contains("declined"), "{reason}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_without_our_shortcut_is_an_error_not_a_success() {
    let bus = PrivateBus::start();
    let _portal = MockPortal::start(
        &bus.address,
        Options {
            bind: Bind::NoShortcut,
            ..Options::default()
        },
    )
    .await;
    let mut run = start_client(&bus);
    let reason = unavailable_reason(next_status(&mut run).await);
    assert!(reason.contains("did not register"), "{reason}");
}

#[tokio::test(flavor = "multi_thread")]
async fn no_portal_on_the_bus_is_reported_plainly() {
    let bus = PrivateBus::start();
    let mut run = start_client(&bus);
    let reason = unavailable_reason(next_status(&mut run).await);
    assert!(reason.contains("no desktop portal"), "{reason}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_portal_without_global_shortcuts_is_reported_as_such() {
    let bus = PrivateBus::start();
    let _portal = MockPortal::start(
        &bus.address,
        Options {
            global_shortcuts: false,
            ..Options::default()
        },
    )
    .await;
    let mut run = start_client(&bus);
    let reason = unavailable_reason(next_status(&mut run).await);
    assert!(
        reason.contains("does not offer global shortcuts"),
        "{reason}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_portal_without_a_registry_still_works_and_says_so() {
    let bus = PrivateBus::start();
    let _portal = MockPortal::start(
        &bus.address,
        Options {
            registry: false,
            ..Options::default()
        },
    )
    .await;
    let mut run = start_client(&bus);
    match next_status(&mut run).await {
        ShortcutStatus::Active {
            trigger,
            registration: Some(note),
        } => {
            assert_eq!(trigger, "Ctrl+Shift+Space");
            assert!(note.contains("registry"), "{note}");
        }
        other => panic!("expected Active with a registration note, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failure_after_a_refused_registration_mentions_the_registration() {
    let bus = PrivateBus::start();
    let _portal = MockPortal::start(
        &bus.address,
        Options {
            registry: false,
            bind: Bind::Cancelled,
            ..Options::default()
        },
    )
    .await;
    let mut run = start_client(&bus);
    let reason = unavailable_reason(next_status(&mut run).await);
    assert!(reason.contains("declined"), "{reason}");
    assert!(reason.contains("no application registry"), "{reason}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_closed_by_the_portal_is_reported() {
    let bus = PrivateBus::start();
    let portal = MockPortal::start(&bus.address, Options::default()).await;
    let mut run = start_client(&bus);
    assert!(matches!(
        next_status(&mut run).await,
        ShortcutStatus::Active { .. }
    ));

    portal.close_session().await;
    let reason = unavailable_reason(next_status(&mut run).await);
    assert!(reason.contains("closed"), "{reason}");
    timeout(Duration::from_secs(5), run.task)
        .await
        .expect("the task ends once the shortcut stopped working")
        .expect("the task does not panic");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_task_ends_quietly_when_nobody_listens_for_commands_any_more() {
    let bus = PrivateBus::start();
    let portal = MockPortal::start(&bus.address, Options::default()).await;
    let mut run = start_client(&bus);
    assert!(matches!(
        next_status(&mut run).await,
        ShortcutStatus::Active { .. }
    ));

    drop(run.commands);
    portal.activate(SHORTCUT_ID).await;
    timeout(Duration::from_secs(5), run.task)
        .await
        .expect("the task ends when the command channel is closed")
        .expect("the task does not panic");
    assert!(
        run.status.try_recv().is_err(),
        "ending because the application exits is not a failure"
    );
}
