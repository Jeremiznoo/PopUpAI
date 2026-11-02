//! The tray icon against a mock StatusNotifier host, on a private bus.
//!
//! ksni connects to the session bus named by the environment and cannot be pointed
//! elsewhere, and setting an environment variable from a test needs `unsafe`, which this
//! workspace forbids. So the test runs itself again as a child process with
//! `DBUS_SESSION_BUS_ADDRESS` set to the private bus: `tray_client` is that child, and
//! does nothing unless `POPUPAI_TRAY_CHILD` is set.
//!
//! The mock follows the StatusNotifierItem specification, not Plasma. Whether the icon
//! shows in the real panel is checked on the real session.

mod support;

use std::io::{BufRead, BufReader};
use std::process::{Child, Command as Process, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use popupai_platform::{Command, tray};
use support::PrivateBus;
use tokio::sync::mpsc::unbounded_channel;
use tokio::time::{sleep, timeout};
use zbus::{connection, interface};

struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, service: String) {
        self.items.lock().unwrap().push(service);
    }

    fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The child process: show the tray, report when it is up, then report what it receives.
#[tokio::test(flavor = "multi_thread")]
async fn tray_client() {
    if std::env::var_os("POPUPAI_TRAY_CHILD").is_none() {
        return; // only meaningful when started by `a_click_on_the_icon_toggles_the_popup`
    }
    let (commands, mut received) = unbounded_channel();
    let _tray = tray::spawn(commands).await.expect("the tray starts");
    println!("TRAY-READY");
    match timeout(Duration::from_secs(20), received.recv()).await {
        Ok(Some(command)) => println!("RECEIVED {command:?}"),
        other => println!("NOTHING {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_click_on_the_icon_toggles_the_popup() {
    let bus = PrivateBus::start();
    let items = Arc::new(Mutex::new(Vec::new()));
    let _watcher = connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.kde.StatusNotifierWatcher")
        .unwrap()
        .serve_at(
            "/StatusNotifierWatcher",
            Watcher {
                items: items.clone(),
            },
        )
        .unwrap()
        .build()
        .await
        .expect("start the mock watcher");

    let child = Process::new(std::env::current_exe().unwrap())
        .args(["--exact", "tray_client", "--nocapture", "--test-threads=1"])
        .env("POPUPAI_TRAY_CHILD", "1")
        .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start the tray client");
    let mut child = KillOnDrop(child);
    let stdout = child.0.stdout.take().unwrap();
    let lines = tokio::task::spawn_blocking(move || {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .find(|line| line.starts_with("RECEIVED") || line.starts_with("NOTHING"))
    });

    // The icon registers itself with the host.
    let service = timeout(Duration::from_secs(15), async {
        loop {
            if let Some(service) = items.lock().unwrap().first().cloned() {
                return service;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the tray registered with the StatusNotifier host");
    assert!(
        service.contains("StatusNotifierItem") || service.starts_with(':'),
        "unexpected item name {service}"
    );

    // A click on the icon is an `Activate` call on the item.
    let host = connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    // The service may need a moment after registering before it answers.
    let mut activated = false;
    for _ in 0..40 {
        if host
            .call_method(
                Some(service.as_str()),
                "/StatusNotifierItem",
                Some("org.kde.StatusNotifierItem"),
                "Activate",
                &(0_i32, 0_i32),
            )
            .await
            .is_ok()
        {
            activated = true;
            break;
        }
        sleep(Duration::from_millis(100)).await;
    }
    assert!(activated, "the item answers Activate");

    let outcome = timeout(Duration::from_secs(20), lines)
        .await
        .expect("the client reported in time")
        .expect("reading the client output")
        .expect("the client printed a result");
    assert_eq!(outcome, format!("RECEIVED {:?}", Command::Toggle));
}
