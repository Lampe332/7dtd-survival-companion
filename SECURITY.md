# Security & Trust

Downloading a random `.exe` from the internet *should* feel sketchy. This page explains
exactly what this program does, what it can and can't reach, and how to **verify** that the
file you downloaded is the real, unmodified release — or skip the download entirely and build
it yourself.

## What it is (and isn't)

- **Local and offline.** It's a single Windows `.exe`. It runs a small web UI on
  `127.0.0.1:17873` (your machine only) and opens it in your browser. No account, no login,
  no cloud, no telemetry, no background service.
- **Read-only on your game.** It *reads* your 7 Days to Die saves and worlds. The **only**
  thing it ever writes to your game is the world settings you change yourself in the World
  tab — and only after it makes a timestamped backup you can restore with one click.
- **Open source.** The entire backend (`src/`) and the frontend (`7DtD_Skill_Tracker.html`)
  are in this repo. The `.exe` is just those files compiled — `cargo build --release` (see
  the README). If you'd rather not trust a prebuilt binary, build it yourself.

## What it can reach on the network

The page's Content-Security-Policy hard-blocks every destination except two, and both are
optional and user-triggered:

1. **`7daystodie.fandom.com`** — only when *you* run a Wiki search in the app.
2. **`api.github.com` / `github.com`** — a once-a-day check for a newer release, and the
   download itself if *you* choose to update.

That's it. Your save data is never uploaded anywhere. Close the tab and nothing runs.

## How the local server is protected

A local web server can be poked by any website your browser visits, so this one defends
against that:

- Binds **`127.0.0.1` only** (not reachable from your network/LAN).
- Validates the **`Host` header** (blocks DNS-rebinding attacks) **and** the **`Origin`**
  header (blocks cross-site requests from other web pages).
- Mutating endpoints require `Content-Type: application/json` (CSRF defense-in-depth) and
  cap request body size.
- File-serving endpoints reject path traversal (`..`, absolute paths).

## How updates are protected

The in-app updater does **not** blindly trust GitHub:

- Each release `.exe` is signed with an **Ed25519 private key kept offline** — it is never in
  this repo and never on GitHub.
- The app has the matching **public key compiled in** and verifies the release's `.sig` over
  the exact downloaded bytes **before** installing.
- It **fails closed**: a missing or invalid signature aborts the update.
- So even if the GitHub account or a release were compromised, an attacker still can't push
  code the app will run — they can't produce a valid signature without the offline key.

## Verify your download

Every release ships a `SHA256SUMS.txt`. To check the file you downloaded matches the
published hash (PowerShell):

```powershell
Get-FileHash ".\7DtD Companion.exe" -Algorithm SHA256
```

Compare the result to the line in `SHA256SUMS.txt` on the release page. If it doesn't match,
**do not run it** — delete it and download again.

Releases also include a `.sig` (the Ed25519 signature used by the in-app updater).

## Unsigned binary / SmartScreen

This `.exe` is **not** signed with a paid Authenticode certificate, so Windows SmartScreen may
warn on first run ("Windows protected your PC"). That warning means "unknown publisher," not
"known malware." Your options:

- Verify the SHA-256 against `SHA256SUMS.txt` (above), then **More info → Run anyway**, or
- Build it yourself from source (`cargo build --release`), or
- Scan it with your AV / VirusTotal first.

## Reporting a security issue

Found something? Open a GitHub issue, or contact the maintainer through the repo. Concrete
exploit paths are very welcome — this tool has already been hardened from community review.
