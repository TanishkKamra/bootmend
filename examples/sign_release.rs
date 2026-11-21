//! Makes the key pair Bootmend releases are signed with. It isn't part of the app.
//!
//! `cargo run --example sign_release -- new-key <file>`
//!     Writes the secret key to <file> and prints the public key, which goes in `PUBLIC_KEY` in
//!     src/update.rs. The secret key never goes in the repository. The release workflow signs
//!     with minisign, and its rehearsal uses this to make a throwaway key.

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
        _ => Err("usage: sign_release new-key <file>".into()),
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
