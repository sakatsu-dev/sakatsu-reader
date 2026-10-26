//! Release tooling only. Uses Minisign's implementation and Tauri's versioned comment format.
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{error::Error, fs, io::Cursor};

fn decoded(text: &str) -> Result<String, Box<dyn Error>> {
    Ok(String::from_utf8(STANDARD.decode(text.trim())?)?)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [action, file, version] if action == "sign" => {
            let key = decoded(&std::env::var("TAURI_SIGNING_PRIVATE_KEY")?)?;
            let key = minisign::SecretKeyBox::from_string(&key)?.into_secret_key(Some(
                std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default(),
            ))?;
            let signature = minisign::sign(
                None,
                &key,
                fs::File::open(file)?,
                Some(&format!("version:{version}")),
                None,
            )?;
            fs::write(format!("{file}.sig"), STANDARD.encode(signature.to_bytes()))?;
        }
        [action, file, signature, pubkey, version] if action == "verify" => {
            let pk = minisign::PublicKeyBox::from_string(&decoded(&fs::read_to_string(pubkey)?)?)?
                .into_public_key()?;
            let signature =
                minisign::SignatureBox::from_string(&decoded(&fs::read_to_string(signature)?)?)?;
            minisign::verify(
                &pk,
                &signature,
                Cursor::new(fs::read(file)?),
                true,
                false,
                false,
            )?;
            if signature
                .trusted_comment()?
                .split('\t')
                .find_map(|s| s.strip_prefix("version:"))
                != Some(version)
            {
                return Err("The signed version does not match the release version.".into());
            }
        }
        _ => {
            return Err(
                "Use: verify-release sign FILE VERSION | verify FILE SIG PUBLIC_KEY_FILE VERSION"
                    .into(),
            )
        }
    }
    println!("Release signature operation succeeded.");
    Ok(())
}
