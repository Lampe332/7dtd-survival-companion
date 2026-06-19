# Generates app.ico for the 7DtD Survival Companion (blood-moon + sledgehammer).
# One purpose, in-place. Run: python make_icon.py
import math
from PIL import Image, ImageDraw, ImageFilter

SS = 1024  # supersample canvas


def rounded_mask(size, radius):
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=255)
    return m


def vgrad(size, top, bot):
    g = Image.new("RGB", (1, size))
    for y in range(size):
        t = y / (size - 1)
        g.putpixel((0, y), tuple(int(top[i] + (bot[i] - top[i]) * t) for i in range(3)))
    return g.resize((size, size))


def radial(size, center, r, inner, outer):
    img = Image.new("RGB", (size, size), outer)
    px = img.load()
    cx, cy = center
    for y in range(size):
        for x in range(size):
            d = math.hypot(x - cx, y - cy) / r
            if d > 1.4:
                continue
            t = min(d, 1.0)
            px[x, y] = tuple(int(inner[i] + (outer[i] - inner[i]) * t) for i in range(3))
    return img


def main():
    base = Image.new("RGBA", (SS, SS), (0, 0, 0, 0))

    # dark rounded background w/ vertical gradient
    bg = vgrad(SS, (26, 20, 16), (10, 8, 6)).convert("RGBA")
    mask = rounded_mask(SS, int(SS * 0.18))
    base.paste(bg, (0, 0), mask)

    # subtle inner border
    bd = ImageDraw.Draw(base)
    bd.rounded_rectangle([6, 6, SS - 7, SS - 7], radius=int(SS * 0.17),
                         outline=(60, 42, 28, 180), width=8)

    # blood moon
    mcx, mcy, mr = int(SS * 0.585), int(SS * 0.42), int(SS * 0.27)
    glow = Image.new("RGBA", (SS, SS), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse([mcx - mr * 1.5, mcy - mr * 1.5, mcx + mr * 1.5, mcy + mr * 1.5],
               fill=(255, 80, 20, 120))
    glow = glow.filter(ImageFilter.GaussianBlur(SS * 0.05))
    base = Image.alpha_composite(base, glow)

    moon = radial(SS, (mcx, mcy), mr, (255, 150, 50), (150, 24, 6)).convert("RGBA")
    mmask = Image.new("L", (SS, SS), 0)
    ImageDraw.Draw(mmask).ellipse([mcx - mr, mcy - mr, mcx + mr, mcy + mr], fill=255)
    base.paste(moon, (0, 0), mmask)
    # craters
    cd = ImageDraw.Draw(base)
    for (ox, oy, rr) in [(-0.35, -0.2, 0.16), (0.3, 0.25, 0.2), (0.1, -0.4, 0.1), (-0.1, 0.4, 0.13)]:
        x, y = mcx + ox * mr, mcy + oy * mr
        r2 = rr * mr
        cd.ellipse([x - r2, y - r2, x + r2, y + r2], fill=(120, 18, 6, 90))

    # sledgehammer (handle + head) on its own layer, then composite
    ham = Image.new("RGBA", (SS, SS), (0, 0, 0, 0))
    hd = ImageDraw.Draw(ham)
    x0, y0 = int(SS * 0.24), int(SS * 0.82)   # handle bottom
    x1, y1 = int(SS * 0.74), int(SS * 0.34)   # handle top (under head)
    hw = int(SS * 0.07)
    # handle shadow then wood
    hd.line([x0, y0, x1, y1], fill=(20, 12, 6, 255), width=hw + 14)
    hd.line([x0, y0, x1, y1], fill=(150, 96, 50, 255), width=hw)
    hd.line([x0 + 6, y0 - 6, x1 + 6, y1 - 6], fill=(190, 130, 74, 255), width=int(hw * 0.35))
    # pommel
    hd.ellipse([x0 - hw * 0.7, y0 - hw * 0.7, x0 + hw * 0.7, y0 + hw * 0.7], fill=(120, 78, 40, 255))

    # head: rounded rect drawn flat then rotated to be perpendicular to handle
    ang = math.degrees(math.atan2(y1 - y0, x1 - x0))   # handle angle
    hlen, hwid = int(SS * 0.40), int(SS * 0.165)
    head = Image.new("RGBA", (hlen, hwid), (0, 0, 0, 0))
    hdd = ImageDraw.Draw(head)
    hdd.rounded_rectangle([0, 0, hlen - 1, hwid - 1], radius=int(hwid * 0.22), fill=(120, 126, 134, 255))
    hdd.rounded_rectangle([0, 0, hlen - 1, hwid - 1], radius=int(hwid * 0.22), outline=(40, 44, 50, 255), width=10)
    # steel shading bands
    hdd.rectangle([0, 0, hlen - 1, int(hwid * 0.42)], fill=(168, 174, 182, 120))
    hdd.rectangle([0, int(hwid * 0.78), hlen - 1, hwid - 1], fill=(70, 74, 80, 140))
    # orange rim light on striking faces
    hdd.rectangle([0, 0, int(hlen * 0.06), hwid - 1], fill=(255, 130, 40, 200))
    hdd.rectangle([int(hlen * 0.94), 0, hlen - 1, hwid - 1], fill=(255, 130, 40, 200))
    head = head.rotate(-(ang + 90), expand=True, resample=Image.BICUBIC)
    hx, hy = x1 - head.width // 2, y1 - head.height // 2
    ham.alpha_composite(head, (hx, hy))

    base = Image.alpha_composite(base, ham)
    # clip everything to rounded mask
    out = Image.new("RGBA", (SS, SS), (0, 0, 0, 0))
    out.paste(base, (0, 0), mask)

    # preview + multi-size ico
    out.resize((256, 256), Image.LANCZOS).save("icon_preview.png")
    sizes = [(s, s) for s in (16, 24, 32, 48, 64, 128, 256)]
    out.save("app.ico", format="ICO", sizes=sizes)
    print("wrote app.ico + icon_preview.png")


if __name__ == "__main__":
    main()
