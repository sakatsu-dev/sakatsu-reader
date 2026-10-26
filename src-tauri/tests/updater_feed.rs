//! Exercise Tauri's real feed parser and signature verifier, never a browser or installer.
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;

use base64::Engine;
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;

fn encoded(value: impl AsRef<[u8]>) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

fn fixture(
    mode: &str,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    std::thread::JoinHandle<()>,
) {
    let keys = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let wrong = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let payload = b"signed fixture bytes, not an executable".to_vec();
    let signature = minisign::sign(
        Some(&keys.pk),
        &keys.sk,
        Cursor::new(&payload),
        Some("version:99.0.0"),
        None,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let key = if mode == "wrong_key" {
        &wrong.pk
    } else {
        &keys.pk
    };
    let pubkey = encoded(key.to_box().unwrap().to_bytes());
    let version = if mode == "older" {
        "0.0.0"
    } else if mode == "version_mismatch" {
        "98.0.0"
    } else {
        "99.0.0"
    };
    let feed = json!({"version":version,"platforms":{"windows-x86_64":{"url":format!("http://{address}/installer"),"signature":encoded(signature.to_bytes())}}});
    let mode = mode.to_string();
    let thread = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut requests = 0;
        while std::time::Instant::now() < deadline && requests < 2 {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut buf = [0; 8192];
            let count = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..count]);
            if mode == "duplicate" {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            let body = if request.starts_with("GET /installer ") {
                if mode == "tampered" {
                    b"altered fixture".to_vec()
                } else {
                    payload.clone()
                }
            } else if mode == "malformed" {
                b"{bad json".to_vec()
            } else {
                serde_json::to_vec(&feed).unwrap()
            };
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",body.len()).unwrap();
            stream.write_all(&body).unwrap();
            requests += 1;
            if mode == "older" || mode == "malformed" || mode == "duplicate" {
                break;
            }
        }
    });
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        json!({
            "pubkey":pubkey, "endpoints":[format!("http://{address}/feed")],
            "dangerousInsecureTransportProtocol":true, "requireSignedVersion":true
        }),
    );
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    (app, thread)
}

#[test]
fn real_plugin_verifies_valid_payload_and_rejects_tampering_and_other_keys() {
    for mode in ["valid", "tampered", "wrong_key", "version_mismatch"] {
        let (app, server) = fixture(mode);
        let result = tauri::async_runtime::block_on(async {
            let update = app
                .updater_builder()
                .target("windows-x86_64")
                .no_proxy()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap()
                .check()
                .await
                .unwrap()
                .unwrap();
            update.download(|_, _| {}, || {}).await
        });
        assert_eq!(result.is_ok(), mode == "valid", "{mode}: {result:?}");
        server.join().unwrap();
    }
}

#[test]
fn real_plugin_rejects_malformed_feed_and_does_not_offer_older_release() {
    for mode in ["older", "malformed"] {
        let (app, server) = fixture(mode);
        let result = tauri::async_runtime::block_on(
            app.updater_builder()
                .target("windows-x86_64")
                .no_proxy()
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap()
                .check(),
        );
        if mode == "older" {
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.is_err());
        }
        server.join().unwrap();
    }
}

#[test]
fn production_commands_refuse_install_and_download_without_configuration_or_bytes() {
    use tauri::test::{get_ipc_response, INVOKE_KEY};
    let app = mock_builder()
        .manage(sakatsu_desktop_lib::update::UpdateState::default())
        .invoke_handler(sakatsu_desktop_lib::reader_commands!())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    for command in ["update_status", "update_download", "update_install"] {
        let response = get_ipc_response(
            &window,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(json!({})),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        );
        if command == "update_status" {
            assert_eq!(
                response
                    .unwrap()
                    .deserialize::<serde_json::Value>()
                    .unwrap()["phase"],
                "not_configured"
            );
        } else {
            assert!(response.is_err());
        }
    }
}

/// The plugin reports every network chunk. A second of them reaches the interface as five events,
/// the first at once, while the count stays exact.
#[test]
fn download_progress_reaches_the_interface_once_per_interval() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tauri::{Listener, Manager};

    use sakatsu_desktop_lib::update::{Throttle, UpdateState, PROGRESS_INTERVAL};

    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    app.manage(UpdateState::new(true));
    let events = Arc::new(AtomicUsize::new(0));
    let counted = events.clone();
    app.listen_any("update-status", move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    let state = app.state::<UpdateState>();
    let mut throttle = Throttle::new(PROGRESS_INTERVAL);
    let start = Instant::now();
    for ms in 0..1000 {
        state.chunk(
            app.handle(),
            &mut throttle,
            start + Duration::from_millis(ms),
            10,
            Some(10_000),
        );
        if ms == 0 {
            assert_eq!(events.load(Ordering::SeqCst), 1, "the first chunk was held");
        }
    }
    assert_eq!(events.load(Ordering::SeqCst), 5);
    let status = sakatsu_desktop_lib::update::update_status(app.state());
    assert_eq!(status.downloaded, 10_000.0);
    assert_eq!(status.total, Some(10_000.0));
}

#[test]
fn duplicate_production_checks_are_refused_and_an_offline_check_is_never_current() {
    use tauri::Manager;
    let (app, server) = fixture("duplicate");
    let root = std::env::temp_dir().join(format!("sakatsu-update-check-{}", std::process::id()));
    assert!(!root.exists(), "stale test directory");
    app.manage(sakatsu_desktop_lib::commands::StoreState {
        store: Some(sakatsu_core::store::Store::open(&root).unwrap()),
        error: None,
    });
    app.manage(sakatsu_desktop_lib::update::UpdateState::new(true));
    let first_app = app.handle().clone();
    let first = tauri::async_runtime::spawn(async move {
        sakatsu_desktop_lib::update::update_check(first_app, true).await
    });
    for _ in 0..100 {
        if sakatsu_desktop_lib::update::update_status(app.state()).phase
            == sakatsu_core::update::UpdatePhase::Checking
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let result = tauri::async_runtime::block_on(sakatsu_desktop_lib::update::update_check(
        app.handle().clone(),
        true,
    ));
    assert!(result.unwrap_err().contains("already running"));
    tauri::async_runtime::block_on(first).unwrap().unwrap();
    server.join().unwrap();
    let offline = tauri::async_runtime::block_on(sakatsu_desktop_lib::update::update_check(
        app.handle().clone(),
        true,
    ))
    .unwrap();
    assert!(matches!(
        offline.phase,
        sakatsu_core::update::UpdatePhase::Offline | sakatsu_core::update::UpdatePhase::Failed
    ));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
