<div align="center">

<img src="assets/logo.png" width="140" alt="7DtD Survival Companion logo">

# 7DtD Survival Companion

**Local, offline companion app for _7 Days to Die_** — interactive world map, perk planner, and a real settings editor that writes straight back to your save.

![Rust](https://img.shields.io/badge/Rust-backend-orange?logo=rust)
![Platform](https://img.shields.io/badge/platform-Windows-0078D6?logo=windows)
![Offline](https://img.shields.io/badge/100%25-offline-success)
![License](https://img.shields.io/badge/license-MIT-green)

### ⬇ [Download the latest release](https://github.com/Lampe332/7dtd-survival-companion/releases/latest)

</div>

---

## What it is

A single Rust executable that serves a small web app on `127.0.0.1:17873` and reads your **real** 7 Days to Die saves and generated worlds. No cloud, no account, no telemetry — everything stays on your machine.

## Features

- 🌍 **World settings editor** — edit `gameOptions.sdf` with real write-back (automatic backup + byte-exact re-encode + SandboxCode patching), one-click **difficulty presets** (Baby → Nightmare), and a plain-English explanation for every setting
- 🗺️ **3D world map** — WebGL terrain (biomes, roads, water) decoded from your world files; fly the camera, search 2000+ POIs with fly-to, filter by difficulty tier, and **click any building** for an info panel: prefab preview image, tier, size, max zombies, quest types and distance to your base
- 🔨 **Build planner** — full 56-perk catalog, guided phase plan, imports your real `.ttp` progression
- 🩸 **Horde night** — readiness checklist + special-enemy timeline by gamestage
- 📦 **Loot**, 📕 **Magazines**, 📋 **Reference** — loot-stage calculator, all crafting magazines & perk books, quick reference cards

## Download & run

1. Download **`7DtD Companion.exe`** from the [latest release](https://github.com/Lampe332/7dtd-survival-companion/releases/latest).
2. Double-click it — it opens in your default browser. No install, no dependencies: one self-contained file.
3. Hit **Scan** to load your worlds and characters.

## Build from source

```bash
cargo build --release
# output: target/release/seven-dtd-companion.exe
```

The frontend (`7DtD_Skill_Tracker.html`) and reference data (`src/refdata.json`) are baked into the binary via `include_str!`, so the `.exe` is fully self-contained.

## Tech

- **Backend:** Rust + [`tiny_http`](https://crates.io/crates/tiny_http) — file scanning, binary `.sdf` / `.ttp` parsing, settings write-back, map decoding
- **Frontend:** vanilla JS, single file, custom WebGL for the 3D map (no framework)
- **Platform:** Windows
- **UI language:** English

## License

MIT — see [LICENSE](LICENSE).
