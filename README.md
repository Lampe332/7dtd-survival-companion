<div align="center">

<img src="assets/logo.png" width="116" alt="7DtD Survival Companion logo">

# 7DtD Survival Companion

**A local, offline command center for _7 Days to Die_.**
Reads your real saves and worlds — then plans your build, decodes your map, preps your horde night, and writes settings straight back to your game.

<br>

[![Latest release](https://img.shields.io/github/v/release/Lampe332/7dtd-survival-companion?style=for-the-badge&label=DOWNLOAD&color=ed762e&labelColor=14110d)](https://github.com/Lampe332/7dtd-survival-companion/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Lampe332/7dtd-survival-companion/total?style=for-the-badge&label=installs&color=3b7d3b&labelColor=14110d)](https://github.com/Lampe332/7dtd-survival-companion/releases)

![Windows](https://img.shields.io/badge/Windows-single%20.exe-1d8fc4?style=flat-square&logo=windows&logoColor=white&labelColor=14110d)
![Rust](https://img.shields.io/badge/Rust-backend-ce8b4f?style=flat-square&logo=rust&logoColor=white&labelColor=14110d)
![Offline](https://img.shields.io/badge/100%25-offline-3b7d3b?style=flat-square&labelColor=14110d)
![No telemetry](https://img.shields.io/badge/no-account%20%C2%B7%20no%20cloud-7a7a7a?style=flat-square&labelColor=14110d)
![License](https://img.shields.io/badge/license-MIT-9a9a9a?style=flat-square&labelColor=14110d)

</div>

<br>

<div align="center">
<img src="assets/screenshots/d3.png" width="900" alt="3D world map — WebGL terrain with POI boxes, roads and building labels decoded from your save">
<br>
<sub><b>Your actual world, in 3D.</b> Terrain, roads, water and 2&nbsp;000+ POIs decoded straight from your save files — fly the camera, search by in-game name, click any building for intel.</sub>
</div>

---

## What it is

One self-contained Windows executable. Double-click it and a dark, fast control panel opens in your browser at `127.0.0.1` — it scans your **real** 7 Days to Die saves and generated worlds and turns them into eleven operational tabs.

No installer. No account. No cloud. No telemetry. The `.exe` is the whole app; everything it reads and writes stays on your machine.

> **Eleven tabs** — World · 3D Map · Dashboard · Build · Perks · Horde · Squad · Loot · Magazines · Reference · Wiki

---

## A guided tour

### 🧭 Dashboard — situation at a glance
The moment you import a survivor: days to the next Blood Moon, your Game Stage, build progress, loot stage and a single readiness grade, all pulled live from the save.

<img src="assets/screenshots/dash.png" width="880" alt="Survivor dashboard with day, Blood Moon, Game Stage, build and readiness rings">

### 🌍 World Control — a settings editor that writes back
Edit `gameOptions.sdf` directly, with a **timestamped backup, byte-exact re-encode and one-click restore**. All **150** sandbox options are editable using the game's real value lists, with one-click difficulty presets (Baby → Nightmare) and a plain-English explanation for every single setting.

<img src="assets/screenshots/world.png" width="880" alt="World settings editor showing difficulty presets and editable sandbox options with risk badges">

### 🔨 Build Planner — 19 gate-correct routes
A guided, phase-by-phase plan that imports your real `.ttp` progression. **Every step is one skill point** — each perk rank and each attribute level, with its real per-level effect. Pick any of the 57 perks and it slots into the route at the phase its gate unlocks, prerequisites auto-inserted. Each route shows its win-condition rationale and skill-point budget, and exports to Markdown to share with your squad.

<img src="assets/screenshots/build.png" width="880" alt="Build planner showing a gate-correct sledgehammer route with budget and validation">

### 🩸 Horde Command — survive the night
A readiness checklist, a real special-enemy threat sheet with counters (Demolisher chest charge, Cop suicide-explosion, Wight, Screamer heat…), the Blood Moon sequence, combat doctrine, and a **combat loadout that follows your active build**.

<img src="assets/screenshots/horde.png" width="880" alt="Horde command screen with readiness grade, threat counters and combat loadout">

### 👥 Squad Board — built for 8-player servers
Assign up to eight teammates a build and instantly see **role-coverage gaps** (Anchor / Ranged / Flanker / Support), the real party Game Stage, per-player Blood Moon load and a PvP counter playbook for Kill-Everyone servers.

<img src="assets/screenshots/squad.png" width="880" alt="Squad board with role coverage, party Game Stage and PvP playbook">

### 📕 Magazine Archive — what to read next
All 23 crafting magazines with a readable **quality ladder** (Q1–Q6) and a **Closest Unlocks** tracker that tells you exactly which magazine to read next, plus 19 perk-book series — every item linking out to the wiki.

<img src="assets/screenshots/mags.png" width="880" alt="Magazine archive with closest-unlocks tracker and quality ladder">

### 📚 In-app Wiki — search without alt-tabbing
Live search of the official 7 Days to Die Fandom wiki, right inside the app.

<img src="assets/screenshots/wiki.png" width="880" alt="In-app live wiki search returning sledgehammer pages">

<details>
<summary><b>More views</b> — Perks reference · Loot intelligence</summary>

<br>

**📊 Perk Reference** — every perk with an expandable breakdown of what each of its five ranks does, straight from the game data.

<img src="assets/screenshots/perks.png" width="780" alt="Perk reference catalog">

**📦 Loot Intelligence** — loot-stage → gear-tier calculator with the real progression thresholds.

<img src="assets/screenshots/loot.png" width="780" alt="Loot intelligence calculator">

</details>

---

## Download & run

1. Download **`7DtD Companion.exe`** from the **[latest release](https://github.com/Lampe332/7dtd-survival-companion/releases/latest)**.
2. Double-click it — it opens in your default browser. No install, no dependencies, one self-contained file.
3. Press **Scan**, pick your world, pick your survivor — done.

> Windows may flag a freshly downloaded, unsigned `.exe`. If SmartScreen appears, choose **More info → Run anyway**. The source is right here in this repo — build it yourself if you prefer (below).

## How it works

The app reads, it doesn't phone home:

- **Saves** — `…/7DaysToDie/Saves/<world>/<save>/` → settings (`gameOptions.sdf`), players, and your survivor's progression (`.ttp`).
- **Worlds** — `…/7DaysToDie/GeneratedWorlds/<world>/` → terrain, biomes, water and POIs for the 3D map.
- **Game install** *(optional)* → prefab tiers and POI preview thumbnails.

The **only** thing it ever writes is the world settings you change yourself in the World tab — and only after taking a timestamped backup you can restore with one click.

## Privacy

100% local. No account, no network calls except the optional in-app wiki search you trigger yourself, no analytics, no background services. Close the browser tab and it's gone.

## Build from source

```bash
cargo build --release
# output: target/release/seven-dtd-companion.exe
```

The frontend (`7DtD_Skill_Tracker.html`) and reference data (`src/refdata.json`) are baked into the binary with `include_str!`, so the resulting `.exe` is fully self-contained — nothing to ship alongside it.

## Tech

| | |
|---|---|
| **Backend** | Rust + [`tiny_http`](https://crates.io/crates/tiny_http) — file scanning, binary `.sdf` / `.ttp` parsing, settings write-back, map decoding |
| **Frontend** | Vanilla JS, single file, hand-rolled WebGL for the 3D map — no framework, no build step |
| **Platform** | Windows · **UI language:** English |

## License

MIT — see [LICENSE](LICENSE). Not affiliated with The Fun Pimps. _7 Days to Die_ is a trademark of its respective owners.
