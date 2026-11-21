//! Signs Bootmend releases so installed copies can tell a real update from a fake one. The
//! release workflow runs it; it isn't part of the app.
//!
//! `cargo run --example sign_release -- new-key <file>`
//!     Makes a key pair, writes the secret key to <file> and prints the public key, which goes in
//!     `PUBLIC_KEY` in src/update.rs. The secret key never goes in the repository.
//!
//! `cargo run --release --example sign_release -- sign <exe> <version>`
//!     Signs <exe> with the secret key in the BOOTMEND_SIGNING_KEY environment variable and
//!     writes <exe>.minisig beside it. The signed comment says "bootmend <version>", and the app
//!     only installs an update whose signed comment names the version it asked for.

use std::io::Write;

fn main() {
    if let Err(e) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [cmd, file] if cmd == "new-key" => new_key(file),
        [cmd, exe, version] if cmd == "sign" => sign(exe, version),
        _ => Err("usage: sign_release new-key <file> | sign <exe> <version>".into()),
    }
}

fn new_key(file: &str) -> Result<(), String> {
    let kp = minisign::KeyPair::generate_unencrypted_keypair().map_err(|e| e.to_string())?;
    let secret = kp.sk.to_box(Some("Bootmend release signing key")).map_err(|e| e.to_string())?.into_string();
    // create_new: never overwrite a key that may already be in use.
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(file).map_err(|e| format!("{file}: {e}"))?;
    f.write_all(secret.as_bytes()).map_err(|e| format!("{file}: {e}"))?;
    println!("{}", kp.pk.to_base64());
    Ok(())
}

fn sign(exe: &str, version: &str) -> Result<(), String> {
    let ok = version.split('.').count() == 3 && version.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if !ok {
        return Err(format!("{version} isn't a version like 0.1.1"));
    }
    let secret = std::env::var("BOOTMEND_SIGNING_KEY").map_err(|_| "BOOTMEND_SIGNING_KEY isn't set".to_string())?;
    let sk = minisign::SecretKeyBox::from_string(&secret).and_then(|b| b.into_unencrypted_secret_key()).map_err(|e| format!("the signing key: {e}"))?;
    let data = std::fs::File::open(exe).map_err(|e| format!("{exe}: {e}"))?;
    let comment = format!("bootmend {version}");
    let sig = minisign::sign(None, &sk, data, Some(&comment), Some("Bootmend release signature")).map_err(|e| e.to_string())?.into_string();
    // Check it the way installed copies will, with the key built into the app, so a wrong secret
    // fails here instead of in everyone's update.
    accepted(exe, &sig, &comment)?;
    let out = format!("{exe}.minisig");
    std::fs::write(&out, sig).map_err(|e| format!("{out}: {e}"))?;
    println!("Signed {exe} as {comment}");
    Ok(())
}

fn accepted(exe: &str, sig: &str, comment: &str) -> Result<(), String> {
    let src = include_str!("../src/update.rs");
    let key = src.split("const PUBLIC_KEY: &str = \"").nth(1).and_then(|s| s.split('"').next()).ok_or("PUBLIC_KEY isn't in src/update.rs")?;
    let key = minisign_verify::PublicKey::from_base64(key).map_err(|e| format!("PUBLIC_KEY in src/update.rs: {e}"))?;
    let sig = minisign_verify::Signature::decode(sig).map_err(|e| e.to_string())?;
    let bin = std::fs::read(exe).map_err(|e| format!("{exe}: {e}"))?;
    key.verify(&bin, &sig, false).map_err(|_| "the signing key doesn't match PUBLIC_KEY in src/update.rs, so installed copies would refuse this release")?;
    if sig.trusted_comment() != comment {
        return Err(format!("the signature says {:?}, not {comment:?}", sig.trusted_comment()));
    }
    Ok(())
}
