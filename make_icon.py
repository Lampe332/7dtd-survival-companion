# Build app.ico + logo.png from the AI-generated icon asset (assets/icon.png).
# One purpose, in-place. Run: python make_icon.py
from PIL import Image, ImageDraw

SRC = "assets/icon.png"
img = Image.open(SRC).convert("RGBA").resize((1024, 1024), Image.LANCZOS)
mask = Image.new("L", (1024, 1024), 0)
ImageDraw.Draw(mask).rounded_rectangle([0, 0, 1023, 1023], radius=180, fill=255)
out = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
out.paste(img, (0, 0), mask)
out.resize((256, 256), Image.LANCZOS).save("logo.png")
out.save("app.ico", sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
print("wrote app.ico + logo.png from", SRC)
