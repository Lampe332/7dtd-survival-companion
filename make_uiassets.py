# Build src/uiassets.json: every UI image downscaled -> JPEG -> base64 data-URI,
# keyed by semantic name, so the single-file exe can show them without shipping loose files.
# Source = the generated 7dtd_assets renders. Run: python make_uiassets.py
import os, json, base64, io
from PIL import Image

SRC = r"D:\flux\ComfyUI-Zluda\output\7dtd_assets"
OUT = os.path.join(os.path.dirname(__file__), "src", "uiassets.json")

# key, asset number, max size px, jpeg quality
KEYMAP = [
    ("bg", 2, 1000, 82),
    ("tab_world", 3, 110, 86), ("tab_map", 4, 110, 86), ("tab_3d", 5, 110, 86),
    ("tab_dash", 6, 110, 86), ("tab_build", 7, 110, 86), ("tab_horde", 8, 110, 86),
    ("tab_loot", 9, 110, 86), ("tab_mags", 10, 110, 86), ("tab_ref", 11, 110, 86),
    ("attr_perception", 12, 150, 88), ("attr_strength", 13, 150, 88), ("attr_fortitude", 14, 150, 88),
    ("attr_agility", 15, 150, 88), ("attr_intellect", 16, 150, 88),
    ("loot_weapons", 17, 110, 86), ("loot_ammo", 18, 110, 86), ("loot_medical", 19, 110, 86),
    ("loot_food", 20, 110, 86), ("loot_tools", 21, 110, 86), ("loot_armor", 22, 110, 86),
    ("loot_resources", 23, 110, 86), ("loot_mods", 24, 110, 86), ("loot_vehicles", 25, 110, 86),
    ("loot_treasure", 26, 110, 86),
    ("enemy_feral", 27, 190, 85), ("enemy_wight", 28, 190, 85), ("enemy_cop", 29, 190, 85),
    ("enemy_demolisher", 30, 190, 85), ("enemy_vulture", 31, 190, 85), ("enemy_spider", 32, 190, 85),
    ("enemy_dog", 33, 190, 85), ("enemy_screamer", 34, 190, 85),
    ("biome_forest", 35, 300, 82), ("biome_desert", 36, 300, 82), ("biome_snow", 37, 300, 82),
    ("biome_burnt", 38, 300, 82), ("biome_wasteland", 39, 300, 82),
]

out, total = {}, 0
for key, num, size, q in KEYMAP:
    p = os.path.join(SRC, f"asset_{num:05d}_.png")
    im = Image.open(p).convert("RGB")
    im.thumbnail((size, size), Image.LANCZOS)
    buf = io.BytesIO()
    im.save(buf, "JPEG", quality=q, optimize=True)
    b = base64.b64encode(buf.getvalue()).decode()
    out[key] = "data:image/jpeg;base64," + b
    total += len(b)

json.dump(out, open(OUT, "w", encoding="utf-8"), separators=(",", ":"))
print("wrote", OUT, "| keys:", len(out), "| approx base64 KB:", total // 1024)
