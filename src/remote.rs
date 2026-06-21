//! Remote save sources: download the files the app needs from a dedicated server
//! (SFTP via russh, FTP via suppaftp) into a local cache dir, then the normal local
//! scanner reads that cache. The app NEVER runs on the server — it only reads files.
//!
//! Credentials live only in `RemoteConn` (in memory, this session) — never on disk,
//! never sent to the browser.

use crate::RemoteConn;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Files pulled per save folder.
const SAVE_FILES: &[&str] = &["gameOptions.sdf", "players.xml", "main.ttw"];
/// Files pulled per generated world (dtm.raw is the heavy heightmap; kept last).
const WORLD_FILES: &[&str] = &[
    "biomes.png",
    "prefabs.xml",
    "map_info.xml",
    "splat3_half.png",
    "splat4_half.png",
    "dtm.raw",
];

fn norm_base(base: &str) -> String {
    let b = base.trim().replace('\\', "/");
    b.trim_end_matches('/').to_string()
}

/// 2 GiB hard ceiling per downloaded file: bounds a malicious/buggy server that
/// advertises a file as terabytes (would otherwise fill the user's temp disk). Real
/// 7DtD files (dtm.raw for even a 16k world ≈ 512 MB) stay well under this.
const MAX_FILE: u64 = 2 * 1024 * 1024 * 1024;

/// A path segment is safe to use as ONE local path component only if it cannot
/// traverse out of the cache dir. Rejects separators, drive colons, NUL and `..` —
/// applied to BOTH user-supplied world/save AND server-supplied filenames (a hostile
/// or compromised server otherwise gets an arbitrary-file-write primitive).
fn safe_seg(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains(':')
        && !s.contains('\0')
}

fn ensure_dirs(cache: &Path, world: &str, save: &str) -> Result<(PathBuf, PathBuf), String> {
    if !safe_seg(world) || !safe_seg(save) {
        return Err("Ungültiger Welt-/Save-Name (Pfad-Trennzeichen nicht erlaubt)".into());
    }
    let sdir = cache.join("Saves").join(world).join(save);
    let wdir = cache.join("GeneratedWorlds").join(world);
    std::fs::create_dir_all(sdir.join("Player")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&wdir).map_err(|e| e.to_string())?;
    Ok((sdir, wdir))
}

pub fn test(conn: &RemoteConn) -> Result<Value, String> {
    match conn.proto.as_str() {
        "sftp" => sftp::test(conn),
        "ftp" | "ftps" => ftp::test(conn),
        other => Err(format!("Protokoll '{other}' nicht unterstützt (sftp|ftp|ftps)")),
    }
}

pub fn list(conn: &RemoteConn) -> Result<Value, String> {
    match conn.proto.as_str() {
        "sftp" => sftp::list(conn),
        "ftp" | "ftps" => ftp::list(conn),
        other => Err(format!("Protokoll '{other}' nicht unterstützt (sftp|ftp|ftps)")),
    }
}

pub fn fetch(conn: &RemoteConn, world: &str, save: &str, cache: &Path) -> Result<Value, String> {
    match conn.proto.as_str() {
        "sftp" => sftp::fetch(conn, world, save, cache),
        "ftp" | "ftps" => ftp::fetch(conn, world, save, cache),
        other => Err(format!("Protokoll '{other}' nicht unterstützt (sftp|ftp|ftps)")),
    }
}

// ---------------------------------------------------------------- SFTP (russh)
mod sftp {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// SFTP client handler with TOFU host-key pinning: the server's key fingerprint is
    /// recorded on first connect (`known_hosts`) and verified on every later connect.
    /// A changed key (possible MITM) aborts the handshake BEFORE the password is sent.
    struct Client {
        host_label: String,
        known_hosts: PathBuf,
        mismatch: Arc<Mutex<Option<String>>>,
    }
    impl russh::client::Handler for Client {
        type Error = russh::Error;
        async fn check_server_key(
            &mut self,
            key: &russh::keys::ssh_key::PublicKey,
        ) -> Result<bool, Self::Error> {
            let fp = key
                .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
                .to_string();
            match known_hosts_lookup(&self.known_hosts, &self.host_label) {
                Some(stored) if stored == fp => Ok(true),
                Some(stored) => {
                    if let Ok(mut g) = self.mismatch.lock() {
                        *g = Some(format!(
                            "Host-Key von {} weicht vom gespeicherten Fingerprint ab — mögliches MITM!\n  gespeichert: {}\n  jetzt:       {}\nWenn der Server-Key wirklich legitim neu ist, entferne die Zeile in:\n  {}",
                            self.host_label, stored, fp, self.known_hosts.display()
                        ));
                    }
                    Ok(false)
                }
                None => {
                    let _ = known_hosts_add(&self.known_hosts, &self.host_label, &fp);
                    Ok(true)
                }
            }
        }
    }

    fn known_hosts_path() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("7DtD-Companion")
            .join("known_hosts")
    }
    fn known_hosts_lookup(path: &Path, host: &str) -> Option<String> {
        let txt = std::fs::read_to_string(path).ok()?;
        for line in txt.lines() {
            let mut it = line.splitn(2, ' ');
            if it.next() == Some(host) {
                if let Some(fp) = it.next() {
                    return Some(fp.trim().to_string());
                }
            }
        }
        None
    }
    fn known_hosts_add(path: &Path, host: &str, fp: &str) -> std::io::Result<()> {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(f, "{host} {fp}")
    }

    fn rt() -> Result<tokio::runtime::Runtime, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
    }

    async fn connect(conn: &RemoteConn) -> Result<russh_sftp::client::SftpSession, String> {
        let config = Arc::new(russh::client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(30)),
            ..Default::default()
        });
        let mismatch = Arc::new(Mutex::new(None));
        let client = Client {
            host_label: format!("{}:{}", conn.host, conn.port),
            known_hosts: known_hosts_path(),
            mismatch: mismatch.clone(),
        };
        let connect_fut =
            russh::client::connect(config, (conn.host.as_str(), conn.port), client);
        let mut h = match tokio::time::timeout(std::time::Duration::from_secs(20), connect_fut).await
        {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                // A host-key mismatch returns Ok(false) from the handler, which russh
                // surfaces as a generic handshake error — replace it with the clear reason.
                if let Some(msg) = mismatch.lock().ok().and_then(|g| g.clone()) {
                    return Err(msg);
                }
                return Err(format!(
                    "Verbindung zu {}:{} fehlgeschlagen: {e}",
                    conn.host, conn.port
                ));
            }
            Err(_) => {
                return Err(format!(
                    "Zeitüberschreitung beim Verbinden zu {}:{}",
                    conn.host, conn.port
                ))
            }
        };
        let ok = h
            .authenticate_password(&conn.user, &conn.pass)
            .await
            .map_err(|e| format!("Auth-Fehler: {e}"))?
            .success();
        if !ok {
            return Err("SFTP-Login abgelehnt (User/Passwort?)".into());
        }
        let ch = h
            .channel_open_session()
            .await
            .map_err(|e| format!("SFTP-Kanal: {e}"))?;
        ch.request_subsystem(true, "sftp")
            .await
            .map_err(|e| format!("SFTP-Subsystem: {e}"))?;
        russh_sftp::client::SftpSession::new(ch.into_stream())
            .await
            .map_err(|e| format!("SFTP-Init: {e}"))
    }

    async fn subdirs(sftp: &russh_sftp::client::SftpSession, path: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(entries) = sftp.read_dir(path.to_string()).await {
            for e in entries {
                if e.file_type().is_dir() {
                    let n = e.file_name();
                    if n != "." && n != ".." {
                        out.push(n);
                    }
                }
            }
        }
        out.sort_by_key(|s| s.to_lowercase());
        out
    }

    async fn download(
        sftp: &russh_sftp::client::SftpSession,
        remote: &str,
        local: &Path,
    ) -> Result<(), String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let rf = sftp.open(remote).await.map_err(|e| format!("{remote}: {e}"))?;
        let mut limited = rf.take(MAX_FILE);
        let mut lf = tokio::fs::File::create(local)
            .await
            .map_err(|e| e.to_string())?;
        tokio::io::copy(&mut limited, &mut lf)
            .await
            .map_err(|e| format!("Download {remote}: {e}"))?;
        lf.flush().await.ok();
        Ok(())
    }

    pub fn test(conn: &RemoteConn) -> Result<Value, String> {
        rt()?.block_on(async {
            let sftp = connect(conn).await?;
            let base = norm_base(&conn.base);
            let entries = sftp
                .read_dir(base.clone())
                .await
                .map_err(|e| format!("Basis-Ordner '{base}' nicht lesbar: {e}"))?;
            let names: Vec<String> = entries.map(|e| e.file_name()).take(40).collect();
            Ok(json!({"ok": true, "base": base, "entries": names}))
        })
    }

    pub fn list(conn: &RemoteConn) -> Result<Value, String> {
        rt()?.block_on(async {
            let sftp = connect(conn).await?;
            let base = norm_base(&conn.base);
            let listpath = if base.is_empty() { "/".to_string() } else { base.clone() };
            let top = sftp
                .read_dir(listpath)
                .await
                .map_err(|e| format!("Basis-Ordner '{}' nicht lesbar: {e}", conn.base))?;
            let names: Vec<String> = top.map(|e| e.file_name()).collect();
            if !names.iter().any(|n| n == "Saves") && !names.iter().any(|n| n == "GeneratedWorlds") {
                return Err(format!("'{}' enthält kein Saves/GeneratedWorlds — ist das der 7DaysToDie-Ordner?", conn.base));
            }
            let mut worlds = Vec::new();
            for w in subdirs(&sftp, &format!("{base}/Saves")).await {
                let saves = subdirs(&sftp, &format!("{base}/Saves/{w}")).await;
                worlds.push(json!({"world": w, "saves": saves}));
            }
            let gen = subdirs(&sftp, &format!("{base}/GeneratedWorlds")).await;
            Ok(json!({"ok": true, "base": base, "worlds": worlds, "genWorlds": gen}))
        })
    }

    pub fn fetch(conn: &RemoteConn, world: &str, save: &str, cache: &Path) -> Result<Value, String> {
        rt()?.block_on(async {
            let sftp = connect(conn).await?;
            let base = norm_base(&conn.base);
            let (sdir, wdir) = ensure_dirs(cache, world, save)?;
            let mut got = 0;
            let mut errs: Vec<String> = Vec::new();
            for f in SAVE_FILES {
                let rp = format!("{base}/Saves/{world}/{save}/{f}");
                match download(&sftp, &rp, &sdir.join(f)).await {
                    Ok(()) => got += 1,
                    Err(e) => errs.push(format!("{f}: {e}")),
                }
            }
            if !errs.is_empty() {
                // A partial fetch must not masquerade as success — the app would scan a
                // half-populated cache as a valid save. Fail loud with the per-file errors.
                return Err(format!(
                    "Pflicht-Dateien konnten nicht geladen werden (nichts übernommen):\n{}",
                    errs.join("\n")
                ));
            }
            let pdir = format!("{base}/Saves/{world}/{save}/Player");
            if let Ok(entries) = sftp.read_dir(pdir.clone()).await {
                for e in entries {
                    let n = e.file_name();
                    if (n.ends_with(".ttp") || n.ends_with(".ttp.meta")) && safe_seg(&n) {
                        let rp = format!("{pdir}/{n}");
                        if download(&sftp, &rp, &sdir.join("Player").join(&n)).await.is_ok() {
                            got += 1;
                        }
                    }
                }
            }
            for f in WORLD_FILES {
                let rp = format!("{base}/GeneratedWorlds/{world}/{f}");
                if download(&sftp, &rp, &wdir.join(f)).await.is_ok() {
                    got += 1;
                }
            }
            if got == 0 {
                return Err("Keine Dateien geladen — Pfad/Welt/Save prüfen.".into());
            }
            Ok(json!({"ok": true, "files": got}))
        })
    }
}

// --------------------------------------------------------- FTP + FTPS (suppaftp)
// Plain FTP and explicit FTP-over-TLS (FTPES) share one generic core over any
// `ImplFtpStream<T: TlsStream>`; only the connect/login step differs.
mod ftp {
    use super::*;
    use suppaftp::types::FileType;
    use suppaftp::{FtpStream, ImplFtpStream, NativeTlsConnector, NativeTlsFtpStream, TlsStream};

    /// TLS context for FTPS with FULL certificate + hostname validation (SChannel on
    /// Windows → uses the OS trust store, no OpenSSL). Disabling validation would make
    /// the TLS layer encrypt-but-not-authenticate: any MITM presenting a self-signed
    /// cert would harvest the FTP password. A server with a self-signed cert should be
    /// reached over SFTP (host-key TOFU) instead.
    fn tls_connector() -> Result<NativeTlsConnector, String> {
        let c = suppaftp::native_tls::TlsConnector::builder()
            .build()
            .map_err(|e| format!("TLS-Setup: {e}"))?;
        Ok(NativeTlsConnector::from(c))
    }

    fn connect_plain(conn: &RemoteConn) -> Result<FtpStream, String> {
        let addr = format!("{}:{}", conn.host, conn.port);
        let mut s = FtpStream::connect(&addr).map_err(|e| format!("Verbindung {addr}: {e}"))?;
        s.login(&conn.user, &conn.pass)
            .map_err(|e| format!("FTP-Login abgelehnt: {e}"))?;
        s.transfer_type(FileType::Binary)
            .map_err(|e| format!("Binary-Mode: {e}"))?;
        Ok(s)
    }

    fn connect_tls(conn: &RemoteConn) -> Result<NativeTlsFtpStream, String> {
        let addr = format!("{}:{}", conn.host, conn.port);
        let plain = NativeTlsFtpStream::connect(&addr)
            .map_err(|e| format!("Verbindung {addr}: {e}"))?;
        let mut s = plain
            .into_secure(tls_connector()?, &conn.host)
            .map_err(|e| format!("FTPS/TLS-Handshake fehlgeschlagen ({}): {e}", conn.host))?;
        s.login(&conn.user, &conn.pass)
            .map_err(|e| format!("FTPS-Login abgelehnt: {e}"))?;
        s.transfer_type(FileType::Binary)
            .map_err(|e| format!("Binary-Mode: {e}"))?;
        Ok(s)
    }

    /// Child entries of a remote path via NLST (returns real names — robust to
    /// spaces, unlike parsing `ls -l` LIST lines). At the Saves/ and Saves/<world>/
    /// levels every child is a directory, so no dir/file filtering is needed.
    fn subdirs<T: TlsStream>(s: &mut ImplFtpStream<T>, path: &str) -> Vec<String> {
        let mut dirs = Vec::new();
        if let Ok(names) = s.nlst(Some(path)) {
            for n in names {
                let base = n.rsplit('/').next().unwrap_or(&n).trim().to_string();
                if !base.is_empty() && base != "." && base != ".." {
                    dirs.push(base);
                }
            }
        }
        dirs.sort_by_key(|s| s.to_lowercase());
        dirs.dedup();
        dirs
    }

    fn retr_to<T: TlsStream>(s: &mut ImplFtpStream<T>, remote: &str, local: &Path) -> Result<(), String> {
        use std::io::Read as _;
        s.retr(remote, |stream| {
            let mut f = std::fs::File::create(local).map_err(suppaftp::FtpError::ConnectionError)?;
            std::io::copy(&mut stream.take(MAX_FILE), &mut f)
                .map_err(suppaftp::FtpError::ConnectionError)?;
            Ok(())
        })
        .map_err(|e| format!("{remote}: {e}"))
    }

    fn core_test<T: TlsStream>(s: &mut ImplFtpStream<T>, conn: &RemoteConn) -> Result<Value, String> {
        let base = norm_base(&conn.base);
        let names = s
            .nlst(Some(base.as_str()))
            .map_err(|e| format!("Basis-Ordner '{base}': {e}"))?;
        let _ = s.quit();
        Ok(json!({"ok": true, "base": base, "entries": names.into_iter().take(40).collect::<Vec<_>>()}))
    }

    fn core_list<T: TlsStream>(s: &mut ImplFtpStream<T>, conn: &RemoteConn) -> Result<Value, String> {
        let base = norm_base(&conn.base);
        let listpath = if base.is_empty() { "/".to_string() } else { base.clone() };
        let top = s
            .nlst(Some(listpath.as_str()))
            .map_err(|e| format!("Basis-Ordner '{}' nicht lesbar: {e}", conn.base))?;
        let has = |n: &str| top.iter().any(|x| x.rsplit('/').next().unwrap_or(x) == n);
        if !has("Saves") && !has("GeneratedWorlds") {
            let _ = s.quit();
            return Err(format!("'{}' enthält kein Saves/GeneratedWorlds — ist das der 7DaysToDie-Ordner?", conn.base));
        }
        let mut worlds = Vec::new();
        for w in subdirs(s, &format!("{base}/Saves")) {
            let saves = subdirs(s, &format!("{base}/Saves/{w}"));
            worlds.push(json!({"world": w, "saves": saves}));
        }
        let gen = subdirs(s, &format!("{base}/GeneratedWorlds"));
        let _ = s.quit();
        Ok(json!({"ok": true, "base": base, "worlds": worlds, "genWorlds": gen}))
    }

    fn core_fetch<T: TlsStream>(s: &mut ImplFtpStream<T>, conn: &RemoteConn, world: &str, save: &str, cache: &Path) -> Result<Value, String> {
        let base = norm_base(&conn.base);
        let (sdir, wdir) = ensure_dirs(cache, world, save)?;
        let mut got = 0;
        let mut errs: Vec<String> = Vec::new();
        for f in SAVE_FILES {
            let rp = format!("{base}/Saves/{world}/{save}/{f}");
            match retr_to(s, &rp, &sdir.join(f)) {
                Ok(()) => got += 1,
                Err(e) => errs.push(format!("{f}: {e}")),
            }
        }
        if !errs.is_empty() {
            let _ = s.quit();
            return Err(format!(
                "Pflicht-Dateien konnten nicht geladen werden (nichts übernommen):\n{}",
                errs.join("\n")
            ));
        }
        let pdir = format!("{base}/Saves/{world}/{save}/Player");
        if let Ok(names) = s.nlst(Some(pdir.as_str())) {
            for n in names {
                let bn = n.rsplit('/').next().unwrap_or(&n).to_string();
                if (bn.ends_with(".ttp") || bn.ends_with(".ttp.meta")) && safe_seg(&bn) {
                    let rp = format!("{pdir}/{bn}");
                    if retr_to(s, &rp, &sdir.join("Player").join(&bn)).is_ok() {
                        got += 1;
                    }
                }
            }
        }
        for f in WORLD_FILES {
            let rp = format!("{base}/GeneratedWorlds/{world}/{f}");
            if retr_to(s, &rp, &wdir.join(f)).is_ok() {
                got += 1;
            }
        }
        let _ = s.quit();
        if got == 0 {
            return Err("Keine Dateien geladen — Pfad/Welt/Save prüfen.".into());
        }
        Ok(json!({"ok": true, "files": got}))
    }

    fn is_tls(conn: &RemoteConn) -> bool {
        conn.proto == "ftps"
    }

    pub fn test(conn: &RemoteConn) -> Result<Value, String> {
        if is_tls(conn) {
            core_test(&mut connect_tls(conn)?, conn)
        } else {
            core_test(&mut connect_plain(conn)?, conn)
        }
    }

    pub fn list(conn: &RemoteConn) -> Result<Value, String> {
        if is_tls(conn) {
            core_list(&mut connect_tls(conn)?, conn)
        } else {
            core_list(&mut connect_plain(conn)?, conn)
        }
    }

    pub fn fetch(conn: &RemoteConn, world: &str, save: &str, cache: &Path) -> Result<Value, String> {
        if is_tls(conn) {
            core_fetch(&mut connect_tls(conn)?, conn, world, save, cache)
        } else {
            core_fetch(&mut connect_plain(conn)?, conn, world, save, cache)
        }
    }
}
