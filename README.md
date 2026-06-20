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

Ten tabs — **World · 3D Map · Dashboard · Build · Perks · Horde · Loot · Magazines · Reference · Wiki** — all reading your real saves. A ⌂ button reopens the world/character picker anytime, so you can switch save without reloading.

- 🌍 **World settings editor** — edit `gameOptions.sdf` with real write-back (automatic backup + byte-exact re-encode + SandboxCode patching + one-click **restore/undo**). All **150** sandbox options are editable with the game's real value lists, plus one-click **difficulty presets** (Baby → Nightmare) and a plain-English explanation for every setting
- 🗺️ **3D world map** — WebGL terrain (biomes, roads, water) decoded from your world files; fly the camera, go fullscreen, search 2000+ POIs by **in-game or brand name** with fly-to, filter by difficulty tier, and **click any building** for an info panel: prefab preview image, tier, size, max zombies, quest types and distance to your base
- 🔨 **Build planner** — 10 build routes with a guided phase plan that imports your real `.ttp` progression. Pick any perk from the full 57-perk catalog and it slots into the route **rank-by-rank at the phase its attribute gate unlocks**, with the attribute-leveling steps auto-inserted as prerequisites (each showing the real per-level attribute buff) and your custom picks flagged. Switch character/world anytime — no reload
- 📊 **Perk reference** — every perk with an expandable breakdown of what each of its 5 ranks does, straight from the game data
- 🩸 **Horde night** — readiness checklist + special-enemy timeline by gamestage
- 📦 **Loot** & 📕 **Magazines** — loot-stage calculator; all 23 crafting magazines with a readable **quality ladder** (Q1–Q6 unlock breakpoints) and 19 perk-book series, every item linking to the wiki
- 📚 **Wiki** — live in-app search of the 7 Days to Die Fandom wiki
- 📋 **Reference** — quick reference cards (settings, sledge rules, infection, base design, biomes, attributes, crafting, buffs, trader, gamestage thresholds)

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
