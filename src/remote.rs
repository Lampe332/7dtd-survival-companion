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

fn ensure_dirs(cache: &Path, world: &str, save: &str) -> Result<(PathBuf, PathBuf), String> {
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
    use std::sync::Arc;

    struct Client;
    impl russh::client::Handler for Client {
        type Error = russh::Error;
        // v1: accept any host key (like FileZilla's first-connect). Host-key pinning
        // is a documented follow-up — fine for a user-typed server, weak on hostile nets.
        async fn check_server_key(
            &mut self,
            _key: &russh::keys::ssh_key::PublicKey,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    fn rt() -> Result<tokio::runtime::Runtime, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())
    }

    async fn connect(conn: &RemoteConn) -> Result<russh_sftp::client::SftpSession, String> {
        let config = Arc::new(russh::client::Config::default());
        let mut h = russh::client::connect(config, (conn.host.as_str(), conn.port), Client)
            .await
            .map_err(|e| format!("Verbindung zu {}:{} fehlgeschlagen: {e}", conn.host, conn.port))?;
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
        use tokio::io::AsyncWriteExt;
        let mut rf = sftp.open(remote).await.map_err(|e| format!("{remote}: {e}"))?;
        let mut lf = tokio::fs::File::create(local)
            .await
            .map_err(|e| e.to_string())?;
        tokio::io::copy(&mut rf, &mut lf)
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
            for f in SAVE_FILES {
                let rp = format!("{base}/Saves/{world}/{save}/{f}");
                if download(&sftp, &rp, &sdir.join(f)).await.is_ok() {
                    got += 1;
                }
            }
            let pdir = format!("{base}/Saves/{world}/{save}/Player");
            if let Ok(entries) = sftp.read_dir(pdir.clone()).await {
                for e in entries {
                    let n = e.file_name();
                    if n.ends_with(".ttp") || n.ends_with(".ttp.meta") {
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

    /// TLS context for FTPS. Game servers commonly use self-signed certs (FileZilla
    /// connects with a "trust this cert?" prompt) — accept them like the SFTP host key
    /// (TOFU). Fine for a user-typed server, weak on a hostile net; cert pinning is a
    /// documented follow-up. SChannel on Windows → no OpenSSL.
    fn tls_connector() -> Result<NativeTlsConnector, String> {
        let c = suppaftp::native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
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
        s.retr(remote, |stream| {
            let mut f = std::fs::File::create(local).map_err(suppaftp::FtpError::ConnectionError)?;
            std::io::copy(stream, &mut f).map_err(suppaftp::FtpError::ConnectionError)?;
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
        for f in SAVE_FILES {
            let rp = format!("{base}/Saves/{world}/{save}/{f}");
            if retr_to(s, &rp, &sdir.join(f)).is_ok() {
                got += 1;
            }
        }
        let pdir = format!("{base}/Saves/{world}/{save}/Player");
        if let Ok(names) = s.nlst(Some(pdir.as_str())) {
            for n in names {
                let bn = n.rsplit('/').next().unwrap_or(&n).to_string();
                if bn.ends_with(".ttp") || bn.ends_with(".ttp.meta") {
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
