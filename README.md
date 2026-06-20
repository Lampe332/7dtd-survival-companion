<div align="center">

<img src="assets/logo.png" width="140" alt="7DtD Survival Companion logo">

# 7DtD Survival Companion

**Local, offline companion app for _7 Days to Die_** — interactive 3D map, gate-correct build & perk planner, 8-player squad board, and a real settings editor that writes straight back to your save.

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

Eleven tabs — **World · 3D Map · Dashboard · Build · Perks · Horde · Squad · Loot · Magazines · Reference · Wiki** — all reading your real saves. Top-bar buttons **refresh from save** (re-read level/day/progress live) or jump **back to the world/character picker** without reloading.

- 🌍 **World settings editor** — edit `gameOptions.sdf` with real write-back (automatic backup + byte-exact re-encode + SandboxCode patching + one-click **restore/undo**). All **150** sandbox options are editable with the game's real value lists, plus one-click **difficulty presets** (Baby → Nightmare) and a plain-English explanation for every setting
- 🗺️ **3D world map** — WebGL terrain (biomes, roads, water) decoded from your world files; fly the camera, go fullscreen, search 2000+ POIs by **in-game or brand name** with fly-to, filter by difficulty tier, and **click any building** for an info panel: prefab preview image, tier, size, max zombies, quest types and distance to your base
- 🔨 **Build planner** — **19 build routes** with a guided, gate-correct phase plan that imports your real `.ttp` progression. Every step is a single skill point (each perk rank, and each attribute level with its real per-level effect); pick any perk from the 57-perk catalog and it slots into the route at the phase its gate unlocks, prerequisites auto-inserted, custom picks flagged. Each route shows its **win-condition rationale** and the **skill-point budget**, and exports to Markdown to share with your squad
- 📊 **Perk reference** — every perk with an expandable breakdown of what each of its 5 ranks does, straight from the game data
- 🩸 **Horde night** — readiness checklist, a real special-enemy threat sheet with counters (Demolisher chest charge, Cop suicide-explosion, Wight, Screamer heat…), Blood Moon sequence and combat doctrine, plus a **Combat Loadout that follows your active build**
- 👥 **Squad board** — assign up to 8 teammates a build, see **role-coverage gaps** (Anchor / Ranged / Flanker / Support), the real **party Game Stage** + per-player Blood Moon load, and a PvP counter playbook for Kill-Everyone servers
- 📦 **Loot** & 📕 **Magazines** — loot-stage → gear-tier calculator; all 23 crafting magazines with a readable **quality ladder** (Q1–Q6) plus a **Closest Unlocks** tracker (what to read next), and 19 perk-book series, every item linking to the wiki
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
