//! Finding the Discord logins the vanilla desktop client already keeps on this
//! device, so Yapfast can link the same accounts without the phone step.
//!
//! Discord's desktop app is Chromium, and keeps each login in its local
//! storage, under `Local Storage/leveldb`. The value is Discord's own
//! obfuscation prefix, [`PREFIX`], then the login encrypted the way Chromium
//! encrypts its passwords: `v10`, a 12-byte nonce, the ciphertext, and a
//! 16-byte tag, AES-256-GCM, under a key the OS keeps for the app (DPAPI on
//! Windows). This reads those files the way the client itself does, decrypts
//! the logins, and hands them back; nothing leaves the device, and the files
//! are only read.
//!
//! The OS decides whether this works: a process can read another app's DPAPI
//! secret only as the same user, which is the case here. Importing is offered
//! only where [`supported`] is true.

use std::{fs, path::PathBuf};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD};

/// Discord's obfuscation prefix on each stored login: a rickroll, as a joke.
/// The encrypted login is the base64 that follows it.
const PREFIX: &[u8] = b"dQw4w9WgXcQ:";
/// Chromium's marker on a login encrypted with the app's key.
const SCHEME: &[u8] = b"v10";
/// AES-GCM's nonce, then its tag, in bytes.
const NONCE: usize = 12;
const TAG: usize = 16;

/// One of Discord's desktop flavours, each its own install with its own data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    Stable,
    Ptb,
    Canary,
}

impl Flavour {
    /// Every flavour, in the order they're looked for.
    const ALL: [Flavour; 3] = [Flavour::Stable, Flavour::Ptb, Flavour::Canary];

    /// What the account switcher can call where a login came from.
    pub fn label(self) -> &'static str {
        match self {
            Flavour::Stable => "Discord",
            Flavour::Ptb => "Discord PTB",
            Flavour::Canary => "Discord Canary",
        }
    }

    /// The name of this flavour's data directory under the per-user config.
    fn dir(self) -> &'static str {
        match self {
            Flavour::Stable => "discord",
            Flavour::Ptb => "discordptb",
            Flavour::Canary => "discordcanary",
        }
    }
}

/// A login read from the vanilla client, and which flavour it came from. The
/// token is as Discord stored it; whether the account is still good is for the
/// caller to check against the API, as the files may hold logged-out logins.
#[derive(Clone, Debug)]
pub struct Found {
    pub token: String,
    pub flavour: Flavour,
}

/// Whether importing from the vanilla client works on this system. The UI
/// hides the button where it doesn't, rather than offer something that fails.
pub fn supported() -> bool {
    cfg!(windows)
}

/// The logins the vanilla Discord client keeps on this device, across every
/// flavour installed, with duplicates removed. An empty list means the client
/// is installed but holds no readable login; the error cases are a system
/// that can't be read ([`Unsupported`]) or a flavour whose key wouldn't
/// decrypt.
pub fn discover() -> Result<Vec<Found>> {
    if !supported() {
        return Err(Unsupported.into());
    }
    let mut found: Vec<Found> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for flavour in Flavour::ALL {
        let Some(dir) = data_dir(flavour) else {
            continue;
        };
        if !dir.is_dir() {
            continue;
        }
        let logins = read_flavour(&dir).with_context(|| {
            format!("could not read the {} logins", flavour.label())
        })?;
        for token in logins {
            if seen.iter().any(|other| other == &token) {
                continue;
            }
            seen.push(token.clone());
            found.push(Found { token, flavour });
        }
    }
    Ok(found)
}

/// Reads and decrypts every login in one flavour's data directory.
fn read_flavour(dir: &std::path::Path) -> Result<Vec<String>> {
    let key = decrypt_key(&fs::read_to_string(dir.join("Local State")).with_context(|| {
        "could not read the Discord app's Local State".to_string()
    })?)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|_| anyhow!("the Discord app's key is the wrong size"))?;

    let mut logins = Vec::new();
    let leveldb = dir.join("Local Storage").join("leveldb");
    let entries = match fs::read_dir(&leveldb) {
        Ok(entries) => entries,
        // No local storage yet: the app ran but no one signed in.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(logins),
        Err(error) => return Err(error).context("could not read the Discord app's local storage"),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // LevelDB keeps its data in .ldb tables and a .log of recent writes;
        // a login can be in either. Other files hold no logins.
        let ext = path.extension().and_then(|ext| ext.to_str());
        if !matches!(ext, Some("ldb") | Some("log")) {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        for blob in encrypted_logins(&bytes) {
            if let Some(token) = decrypt_login(&cipher, blob) {
                if !logins.contains(&token) {
                    logins.push(token);
                }
            }
        }
    }
    Ok(logins)
}

/// Every encrypted login in a LevelDB file: the base64 after each [`PREFIX`],
/// decoded. Scanning for the prefix, rather than parsing LevelDB, is how the
/// value is found whichever table or log holds it, and across the format's
/// versions.
fn encrypted_logins(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut blobs = Vec::new();
    let mut from = 0;
    while let Some(at) = find(&bytes[from..], PREFIX) {
        let start = from + at + PREFIX.len();
        let end = start
            + bytes[start..]
                .iter()
                .take_while(|byte| is_base64(**byte))
                .count();
        if let Ok(blob) = STANDARD.decode(&bytes[start..end]) {
            blobs.push(blob);
        }
        from = end.max(start + 1);
    }
    blobs
}

/// Decrypts one login blob with the app's key, or `None` if it isn't a login
/// of the form this understands or the key doesn't open it (a stale blob from
/// another key, say).
fn decrypt_login(cipher: &Aes256Gcm, blob: Vec<u8>) -> Option<String> {
    if blob.len() < SCHEME.len() + NONCE + TAG || &blob[..SCHEME.len()] != SCHEME {
        return None;
    }
    let (nonce, body) = blob[SCHEME.len()..].split_at(NONCE);
    let plain = cipher.decrypt(Nonce::from_slice(nonce), body).ok()?;
    let token = String::from_utf8(plain).ok()?;
    // Logins are ASCII; anything else is a blob that happened to decrypt.
    (!token.is_empty() && token.is_ascii()).then_some(token)
}

/// The data directory of one Discord flavour, or `None` where the per-user
/// config directory can't be found.
fn data_dir(flavour: Flavour) -> Option<PathBuf> {
    config_root().map(|root| root.join(flavour.dir()))
}

/// Where the Discord flavours keep their per-user data.
#[cfg(windows)]
fn config_root() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(not(windows))]
fn config_root() -> Option<PathBuf> {
    // Reached only through tests; importing is unsupported off Windows.
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

/// Unwraps the app's AES key from its `Local State` file, decrypting it with
/// the OS secret the app wrapped it under.
#[cfg(windows)]
fn decrypt_key(local_state: &str) -> Result<Vec<u8>> {
    let state: serde_json::Value =
        serde_json::from_str(local_state).context("the Discord app's Local State isn't JSON")?;
    let wrapped = state
        .get("os_crypt")
        .and_then(|crypt| crypt.get("encrypted_key"))
        .and_then(|key| key.as_str())
        .ok_or_else(|| anyhow!("the Discord app's Local State has no stored key"))?;
    let mut wrapped = STANDARD
        .decode(wrapped)
        .context("the Discord app's stored key isn't base64")?;
    // Chromium prefixes the wrapped key with "DPAPI"; the rest is what the OS
    // secret protects.
    const MARK: &[u8] = b"DPAPI";
    if wrapped.len() < MARK.len() || &wrapped[..MARK.len()] != MARK {
        bail!("the Discord app's key isn't in the expected form");
    }
    wrapped.drain(..MARK.len());
    dpapi_unprotect(&wrapped)
}

#[cfg(not(windows))]
fn decrypt_key(_local_state: &str) -> Result<Vec<u8>> {
    Err(Unsupported.into())
}

/// Hands a blob to the Windows data-protection API, which returns it in the
/// clear when the current user is the one it was protected for, as it is for
/// anything the Discord app on this account stored.
#[cfg(windows)]
fn dpapi_unprotect(wrapped: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData},
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: wrapped.len() as u32,
        pbData: wrapped.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: `input` points at `wrapped` for the length given, and `output`
    // is read only after a success and freed with LocalFree, as the API asks.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            &mut output,
        )
    };
    if ok == 0 {
        bail!("Windows wouldn't unlock the Discord app's key");
    }
    // SAFETY: on success the API gives a buffer of cbData bytes at pbData.
    let key =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    // SAFETY: the buffer is the API's to free, with LocalFree.
    unsafe {
        LocalFree(output.pbData as *mut _);
    }
    Ok(key)
}

/// The first index of `needle` in `haystack`, or `None`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Whether a byte is one base64 uses, the alphabet and its padding.
fn is_base64(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
}

/// Returned where the vanilla client's logins can't be read on this system,
/// so the caller can say so rather than show it as a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported;

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Importing from the Discord app isn't supported on this system yet.")
    }
}

impl std::error::Error for Unsupported {}

#[cfg(test)]
mod tests {
    use super::*;

    fn seal(key: &[u8], token: &str) -> Vec<u8> {
        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        let nonce = [9u8; NONCE];
        let body = cipher.encrypt(Nonce::from_slice(&nonce), token.as_bytes()).unwrap();
        let mut blob = SCHEME.to_vec();
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&body);
        blob
    }

    #[test]
    fn reads_every_login_from_a_leveldb_file() {
        let key = [3u8; 32];
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let first = seal(&key, "first.token.value");
        let second = seal(&key, "second.token.value");
        // A file as LevelDB lays it out: other bytes around each login, and
        // each login behind the prefix as base64.
        let mut bytes = b"\x00\x01key\x00\x02".to_vec();
        bytes.extend_from_slice(PREFIX);
        bytes.extend_from_slice(STANDARD.encode(&first).as_bytes());
        bytes.extend_from_slice(b"\x00\x02other\x00");
        bytes.extend_from_slice(PREFIX);
        bytes.extend_from_slice(STANDARD.encode(&second).as_bytes());
        bytes.extend_from_slice(b"\xff\xfftrailing");

        let blobs = encrypted_logins(&bytes);
        assert_eq!(blobs.len(), 2);
        let tokens: Vec<_> = blobs
            .into_iter()
            .filter_map(|blob| decrypt_login(&cipher, blob))
            .collect();
        assert_eq!(tokens, ["first.token.value", "second.token.value"]);
    }

    #[test]
    fn ignores_a_blob_another_key_sealed() {
        let ours = Aes256Gcm::new_from_slice(&[1u8; 32]).unwrap();
        let theirs = seal(&[2u8; 32], "someone.elses.token");
        assert_eq!(decrypt_login(&ours, theirs), None);
    }

    #[test]
    fn ignores_bytes_that_only_look_like_a_login() {
        let cipher = Aes256Gcm::new_from_slice(&[4u8; 32]).unwrap();
        let mut bytes = PREFIX.to_vec();
        bytes.extend_from_slice(b"not-valid-base64-or-a-login!!!");
        // No panic, and nothing taken as a login.
        let blobs = encrypted_logins(&bytes);
        assert!(blobs.iter().all(|blob| decrypt_login(&cipher, blob.clone()).is_none()));
    }

    #[test]
    fn unsupported_off_windows_is_the_only_reason_discover_errs_there() {
        if !supported() {
            assert!(discover().unwrap_err().is::<Unsupported>());
        }
    }
}
