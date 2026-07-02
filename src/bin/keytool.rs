//! Offline release-signing tool — NOT shipped to users.
//!
//! This is a separate developer-only binary so the end-user app
//! (`seven-dtd-companion.exe`) contains only what it needs to *verify* an update,
//! never the code to *generate keys* or *sign* binaries. Build with the project
//! (`cargo build --release`) → `target/release/keytool.exe`; keep it local, never
//! upload it as a release asset.
//!
//! Usage:
//!   keytool genkey [out]          Generate an Ed25519 signing key.
//!                                 Secret (64 hex) → [out] (default release_ed25519.key),
//!                                 public key (hex) printed to stdout → paste into
//!                                 RELEASE_PUBKEY_HEX in src/main.rs.
//!   keytool sign <exe> <keyfile>  Sign <exe> with the secret key → writes <exe>.sig
//!                                 (base64 Ed25519 signature over the exact exe bytes).
//!
//! The .sig format is exactly what the app's `verify_release_sig` expects.

use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use std::{env, fs, process::exit};

/// Lowercase-hex string → bytes. None on malformed/odd-length input.
fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.is_empty() || !s.len().is_multiple_of(2) {
        return None;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Some(out)
}

/// bytes → lowercase hex.
fn to_hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn fail(msg: &str) -> ! {
    eprintln!("keytool: {msg}");
    exit(1);
}

fn genkey(out: &str, force: bool) {
    // Never clobber an existing signing key: overwriting it would silently invalidate
    // every already-published .sig (users could no longer verify updates). Require --force.
    if !force && fs::metadata(out).is_ok() {
        fail(&format!(
            "key file {out} already exists — refusing to overwrite (pass --force to replace it)"
        ));
    }
    let mut seed = [0u8; 32];
    if getrandom::getrandom(&mut seed).is_err() {
        fail("secure RNG unavailable");
    }
    let sk = SigningKey::from_bytes(&seed);
    let pk = sk.verifying_key();
    if let Err(e) = fs::write(out, to_hex(&seed)) {
        fail(&format!("could not write secret key to {out}: {e}"));
    }
    println!("Secret key written to: {out}");
    println!("  (KEEP OFFLINE — never commit, push, or upload this file.)");
    println!("Public key (paste into RELEASE_PUBKEY_HEX in src/main.rs):");
    println!("{}", to_hex(pk.as_bytes()));
}

fn sign(exe: &str, keyfile: &str) {
    let key_hex = fs::read_to_string(keyfile)
        .unwrap_or_else(|e| fail(&format!("could not read key file {keyfile}: {e}")));
    let seed = hex_to_bytes(&key_hex)
        .filter(|v| v.len() == 32)
        .unwrap_or_else(|| fail("key file invalid (expected 64 hex characters)"));
    let seed_arr: [u8; 32] = seed.as_slice().try_into().unwrap();
    let sk = SigningKey::from_bytes(&seed_arr);
    let data = fs::read(exe).unwrap_or_else(|e| fail(&format!("could not read {exe}: {e}")));
    let sig = sk.sign(&data);
    let out = format!("{exe}.sig");
    if let Err(e) = fs::write(&out, STANDARD.encode(sig.to_bytes())) {
        fail(&format!("could not write signature to {out}: {e}"));
    }
    println!("Signed {} ({} bytes) → {}", exe, data.len(), out);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("genkey") => {
            let force = args.iter().any(|a| a == "--force");
            // The output path is the first positional arg that isn't the --force flag.
            let out = args
                .iter()
                .skip(2)
                .find(|a| a.as_str() != "--force")
                .map(String::as_str)
                .unwrap_or("release_ed25519.key");
            genkey(out, force);
        }
        Some("sign") => {
            let exe = args.get(2).unwrap_or_else(|| fail("sign: missing <exe> argument"));
            let key = args.get(3).unwrap_or_else(|| fail("sign: missing <keyfile> argument"));
            sign(exe, key);
        }
        _ => {
            eprintln!("keytool — offline release-signing tool (not shipped)");
            eprintln!("Usage:");
            eprintln!("  keytool genkey [out] [--force]  Generate an Ed25519 signing key (--force overwrites)");
            eprintln!("  keytool sign <exe> <keyfile>    Write <exe>.sig for the release exe");
            exit(2);
        }
    }
}
