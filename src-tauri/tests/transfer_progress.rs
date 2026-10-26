//! A controlled transfer through the production setup orchestration.
//!
//! The transport is loopback HTTP with fixture bytes and a known digest. That keeps BITS and the
//! production catalog out of the test while retaining the production event, verification,
//! cancellation and publication sequence.

use sakatsu_core::setup::{After, Artifact, Component, Tier};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Listener;

mod common;

fn serve(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let address = listener.local_addr().expect("listener address");
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("one local request");
        for line in BufReader::new(stream.try_clone().expect("clone stream"))
            .lines()
            .map_while(Result::ok)
        {
            if line.is_empty() {
                break;
            }
        }
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(header.as_bytes())
            .expect("response header");
        // Cancellation may close the socket before all fixture bytes are accepted.
        let _ = stream.write_all(&body);
    });
    (format!("http://{address}/artifact"), worker)
}

fn component(url: String, body: &[u8]) -> &'static Component {
    let url = Box::leak(url.into_boxed_str());
    let hash = Box::leak(sakatsu_core::hashing::hex(Sha256::digest(body)).into_boxed_str());
    let artifacts = Box::leak(
        vec![Artifact {
            url,
            file: "fixture.bin",
            min_bytes: body.len() as u64,
            expected_bytes: Some(body.len() as u64),
            sha256: Some(hash),
        }]
        .into_boxed_slice(),
    );
    Box::leak(Box::new(Component {
        id: "transfer-fixture",
        label: "Transfer fixture",
        about: "Exercises setup transfer orchestration without production resources.",
        bytes: body.len() as u64,
        licence: "test fixture",
        home: "http://127.0.0.1/",
        tier: Tier::Optional,
        artifacts,
        after: After::Nothing,
        script: None,
    }))
}

fn app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock application")
}

fn observed(app: &tauri::App<tauri::test::MockRuntime>) -> Arc<Mutex<Vec<serde_json::Value>>> {
    let values = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&values);
    app.listen("fetch-progress", move |event| {
        sink.lock()
            .expect("event sink")
            .push(serde_json::from_str(event.payload()).expect("typed progress payload"));
    });
    values
}

fn local_http(
    _: &str,
    url: &str,
    target: &Path,
    min_bytes: u64,
    verification: sakatsu_core::setup::Verification<'_>,
    cancel: &Arc<AtomicBool>,
    progress: &mut dyn FnMut(sakatsu_core::setup::Progress),
) -> Result<u64, String> {
    sakatsu_core::setup::download_verified(url, target, min_bytes, verification, cancel, progress)
        .map_err(|error| error.to_string())
}

#[test]
fn a_controlled_transfer_emits_real_progress_and_publishes_verified_bytes() {
    let scratch = common::Scratch::new("transfer-progress", "progress");
    let body = vec![0x5a; 700 * 1024];
    let (url, server) = serve(body.clone());
    let component = component(url, &body);
    let app = app();
    let events = observed(&app);

    let detail = sakatsu_desktop_lib::commands::run_fetch_with(
        app.handle(),
        component,
        &scratch,
        &Arc::new(AtomicBool::new(false)),
        None,
        None,
        &mut local_http,
    )
    .expect("the local fixture installs");
    server.join().expect("fixture server");

    assert_eq!(detail, "1 verified files");
    assert_eq!(
        std::fs::read(scratch.join("fixture.bin")).expect("published fixture"),
        body
    );
    let events = events.lock().expect("events");
    assert!(
        events.iter().any(|event| event["done"] == 0),
        "the starting event is emitted: {events:?}"
    );
    assert!(
        events.iter().any(|event| {
            event["done"].as_u64().is_some_and(|done| done > 0)
                && event["total"] == serde_json::json!(body.len())
        }),
        "the downloader's measured progress reaches the production event: {events:?}"
    );
}

#[test]
fn cancellation_observed_between_chunks_never_publishes_the_artifact() {
    let scratch = common::Scratch::new("transfer-progress", "cancel");
    let body = vec![0x39; 1024 * 1024];
    let (url, server) = serve(body.clone());
    let component = component(url, &body);
    let app = app();
    let events = observed(&app);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut cancelling_http =
        |_: &str,
         url: &str,
         target: &Path,
         min_bytes: u64,
         verification: sakatsu_core::setup::Verification<'_>,
         flag: &Arc<AtomicBool>,
         progress: &mut dyn FnMut(sakatsu_core::setup::Progress)| {
            sakatsu_core::setup::download_verified(
                url,
                target,
                min_bytes,
                verification,
                flag,
                &mut |value| {
                    progress(value);
                    flag.store(true, Ordering::Release);
                },
            )
            .map_err(|error| error.to_string())
        };

    let error = sakatsu_desktop_lib::commands::run_fetch_with(
        app.handle(),
        component,
        &scratch,
        &cancel,
        None,
        None,
        &mut cancelling_http,
    )
    .expect_err("cancellation stops the production sequence");
    server.join().expect("fixture server");

    assert_eq!(error, "paused");
    assert!(!scratch.join("fixture.bin").exists());
    let events = events.lock().expect("events");
    assert!(
        events
            .iter()
            .any(|event| event["done"].as_u64().is_some_and(|done| done > 0)),
        "the cancellation followed a real progress event: {events:?}"
    );
}
