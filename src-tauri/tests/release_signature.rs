//! Test the actual release tool's bytes and version binding with a disposable encrypted key.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{fs, process::Command};

#[test]
fn final_byte_signature_rejects_changes_and_a_different_announced_version() {
    let root =
        std::env::temp_dir().join(format!("sakatsu-release-signature-{}", std::process::id()));
    assert!(!root.exists());
    fs::create_dir(&root).unwrap();
    let key =
        minisign::KeyPair::generate_encrypted_keypair(Some("fixture-password".into())).unwrap();
    let payload = root.join("installer.exe");
    let public = root.join("public-key.txt");
    fs::write(&payload, b"fixture bytes").unwrap();
    fs::write(
        &public,
        STANDARD.encode(key.pk.to_box().unwrap().to_bytes()),
    )
    .unwrap();
    let signed = Command::new(env!("CARGO_BIN_EXE_verify-release"))
        .args(["sign", payload.to_str().unwrap(), "1.0.1"])
        .env(
            "TAURI_SIGNING_PRIVATE_KEY",
            STANDARD.encode(key.sk.to_box(None).unwrap().to_bytes()),
        )
        .env("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "fixture-password")
        .output()
        .unwrap();
    assert!(
        signed.status.success(),
        "{}",
        String::from_utf8_lossy(&signed.stderr)
    );
    let verify = |version: &str| {
        Command::new(env!("CARGO_BIN_EXE_verify-release"))
            .args([
                "verify",
                payload.to_str().unwrap(),
                &format!("{}.sig", payload.display()),
                public.to_str().unwrap(),
                version,
            ])
            .output()
            .unwrap()
    };
    assert!(verify("1.0.1").status.success());
    assert!(!verify("1.0.2").status.success());
    fs::write(&payload, b"modified after signing").unwrap();
    assert!(!verify("1.0.1").status.success());
    fs::remove_dir_all(root).unwrap();
}
