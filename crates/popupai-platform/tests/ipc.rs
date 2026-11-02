//! Single instance and remote commands, over a private D-Bus bus.

mod support;

use std::time::Duration;

use popupai_platform::{BUS_NAME, Bus, Claim, Command, INTERFACE, IpcError, OBJECT_PATH, claim};
use support::PrivateBus;
use tokio::time::timeout;

fn primary(
    claim: Claim,
) -> (
    popupai_platform::Instance,
    tokio::sync::mpsc::UnboundedReceiver<Command>,
) {
    match claim {
        Claim::Primary { instance, commands } => (instance, commands),
        Claim::Existing(_) => panic!("expected to become the primary instance"),
    }
}

fn existing(claim: Claim) -> popupai_platform::Existing {
    match claim {
        Claim::Existing(existing) => existing,
        Claim::Primary { .. } => panic!("expected to find a running instance"),
    }
}

async fn next(commands: &mut tokio::sync::mpsc::UnboundedReceiver<Command>) -> Command {
    timeout(Duration::from_secs(3), commands.recv())
        .await
        .expect("timed out waiting for a command")
        .expect("the command channel closed")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_process_is_the_instance_and_later_ones_forward_commands_to_it() {
    let bus = PrivateBus::start();
    let address = Bus::Address(bus.address.clone());

    let (_instance, mut commands) = primary(claim(&address).await.unwrap());
    let client = existing(claim(&address).await.unwrap());

    for command in [Command::Toggle, Command::Show, Command::Hide, Command::Quit] {
        client.send(command).await.unwrap();
        assert_eq!(next(&mut commands).await, command);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_third_process_also_finds_the_same_instance() {
    let bus = PrivateBus::start();
    let address = Bus::Address(bus.address.clone());
    let (_instance, mut commands) = primary(claim(&address).await.unwrap());

    for _ in 0..3 {
        existing(claim(&address).await.unwrap())
            .send(Command::Toggle)
            .await
            .unwrap();
    }
    for _ in 0..3 {
        assert_eq!(next(&mut commands).await, Command::Toggle);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_name_is_released_when_the_instance_goes_away() {
    let bus = PrivateBus::start();
    let address = Bus::Address(bus.address.clone());
    let (instance, commands) = primary(claim(&address).await.unwrap());
    drop((instance, commands));

    let mut taken_over = false;
    for _ in 0..50 {
        if matches!(claim(&address).await.unwrap(), Claim::Primary { .. }) {
            taken_over = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    assert!(taken_over, "a new instance should be able to take the name");
}

#[tokio::test(flavor = "multi_thread")]
async fn sending_to_an_instance_that_has_gone_fails_with_a_clear_error() {
    let bus = PrivateBus::start();
    let address = Bus::Address(bus.address.clone());
    let (instance, commands) = primary(claim(&address).await.unwrap());
    let client = existing(claim(&address).await.unwrap());
    drop((instance, commands));
    tokio::time::sleep(Duration::from_millis(200)).await;

    let error = client.send(Command::Show).await.unwrap_err();

    assert!(matches!(error, IpcError::Send(_)), "{error}");
}

/// The security property of this interface: nothing in it takes input, so it cannot
/// be used to inject a prompt, and nothing but the four commands exists.
#[tokio::test(flavor = "multi_thread")]
async fn the_interface_has_only_four_commands_and_none_takes_an_argument() {
    let bus = PrivateBus::start();
    let address = Bus::Address(bus.address.clone());
    let (_instance, _commands) = primary(claim(&address).await.unwrap());

    let connection = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let reply = connection
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some("org.freedesktop.DBus.Introspectable"),
            "Introspect",
            &(),
        )
        .await
        .unwrap();
    let xml: String = reply.body().deserialize().unwrap();

    let marker = format!("<interface name=\"{INTERFACE}\">");
    let start = xml
        .find(&marker)
        .unwrap_or_else(|| panic!("the interface `{INTERFACE}` is not exported:\n{xml}"));
    let block = &xml[start
        ..xml[start..]
            .find("</interface>")
            .map(|i| start + i)
            .unwrap()];

    let mut methods: Vec<&str> = block
        .match_indices("<method name=\"")
        .map(|(i, m)| {
            let rest = &block[i + m.len()..];
            &rest[..rest.find('"').unwrap()]
        })
        .collect();
    methods.sort_unstable();
    assert_eq!(methods, ["Hide", "Quit", "Show", "Toggle"], "{block}");
    assert!(
        !block.contains("<arg"),
        "no command may take or return an argument:\n{block}"
    );
    assert!(
        !block.contains("<property"),
        "no properties are exposed:\n{block}"
    );
    assert!(
        !block.contains("<signal"),
        "no signals are exposed:\n{block}"
    );
}
