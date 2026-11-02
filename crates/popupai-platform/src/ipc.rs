//! Single instance and remote commands over D-Bus.
//!
//! The first process claims a well-known name on the session bus and serves a tiny
//! interface. A later process finds the name taken, forwards one command to the
//! running instance, and exits.
//!
//! The interface has **no method that takes any input**. It cannot be used to put
//! text into the popup, which is a deliberate limit (ADR 0002): any program of the
//! same user can call the session bus, so what it can ask for must stay harmless.

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{Connection, connection};

/// Provisional application id: the owner has not confirmed the final one yet.
pub const BUS_NAME: &str = "io.github.popupai.PopUpAI";
pub const OBJECT_PATH: &str = "/io/github/popupai/PopUpAI";
pub const INTERFACE: &str = "io.github.popupai.PopUpAI";

/// What a second invocation can ask the running instance to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Show the popup if hidden, hide it if shown.
    Toggle,
    Show,
    Hide,
    Quit,
}

impl Command {
    fn method(self) -> &'static str {
        match self {
            Self::Toggle => "Toggle",
            Self::Show => "Show",
            Self::Hide => "Hide",
            Self::Quit => "Quit",
        }
    }
}

/// Which bus to use. `Address` exists for tests, which run on a private bus so the
/// user's session bus is never touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    Session,
    Address(String),
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("could not use the D-Bus session bus: {0}")]
    Bus(String),
    #[error("could not send the command to the running instance: {0}")]
    Send(String),
}

/// Keeps the well-known name for as long as it lives. Dropping it releases the name,
/// so another instance can take over.
pub struct Instance {
    _connection: Connection,
}

/// A handle on the instance that already runs.
pub struct Existing {
    connection: Connection,
}

pub enum Claim {
    /// This process is the instance. Commands from later invocations arrive on
    /// `commands`.
    Primary {
        instance: Instance,
        commands: UnboundedReceiver<Command>,
    },
    /// Another process already runs.
    Existing(Existing),
}

impl Existing {
    pub async fn send(&self, command: Command) -> Result<(), IpcError> {
        self.connection
            .call_method(
                Some(BUS_NAME),
                OBJECT_PATH,
                Some(INTERFACE),
                command.method(),
                &(),
            )
            .await
            .map(|_| ())
            .map_err(|err| IpcError::Send(err.to_string()))
    }
}

struct Service {
    commands: UnboundedSender<Command>,
}

// The interface name must be a literal here; a test checks it equals `INTERFACE`.
#[zbus::interface(name = "io.github.popupai.PopUpAI")]
impl Service {
    fn toggle(&self) {
        let _ = self.commands.send(Command::Toggle);
    }

    fn show(&self) {
        let _ = self.commands.send(Command::Show);
    }

    fn hide(&self) {
        let _ = self.commands.send(Command::Hide);
    }

    fn quit(&self) {
        let _ = self.commands.send(Command::Quit);
    }
}

fn bus_error(err: zbus::Error) -> IpcError {
    IpcError::Bus(err.to_string())
}

fn builder(bus: &Bus) -> Result<connection::Builder<'_>, IpcError> {
    match bus {
        Bus::Session => connection::Builder::session(),
        Bus::Address(address) => connection::Builder::address(address.as_str()),
    }
    .map_err(bus_error)
}

/// Become the running instance, or find the one that already runs.
pub async fn claim(bus: &Bus) -> Result<Claim, IpcError> {
    let (commands, receiver) = unbounded_channel();
    // zbus asks for names with `AllowReplacement | ReplaceExisting` by default, which
    // would let a second launch steal the name from the running instance and leave
    // it orphaned. Neither is wanted: the name must stay with the first process.
    let served = builder(bus)?
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .name(BUS_NAME)
        .map_err(bus_error)?
        .serve_at(OBJECT_PATH, Service { commands })
        .map_err(bus_error)?
        .build()
        .await;
    match served {
        Ok(connection) => {
            tracing::info!(name = BUS_NAME, "claimed the instance name");
            Ok(Claim::Primary {
                instance: Instance {
                    _connection: connection,
                },
                commands: receiver,
            })
        }
        Err(zbus::Error::NameTaken) => {
            let connection = builder(bus)?.build().await.map_err(bus_error)?;
            Ok(Claim::Existing(Existing { connection }))
        }
        Err(err) => Err(bus_error(err)),
    }
}
