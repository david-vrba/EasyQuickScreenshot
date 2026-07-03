# Generates the EasyQuickScreenshot icon set: dark badge, gradient scan-crosshair glyph.
from PIL import Image, ImageDraw, ImageFilter
import os

S = 2048  # supersampled master
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)))
os.makedirs(OUT, exist_ok=True)


def rounded_rect_mask(size, radius):
    m = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=255)
    return m


def diagonal_gradient(size, c1, c2):
    g = Image.linear_gradient("L").resize((size, size)).rotate(45, expand=False)
    a = Image.new("RGB", (size, size), c1)
    b = Image.new("RGB", (size, size), c2)
    return Image.composite(b, a, g)


def vertical_gradient(size, c1, c2):
    g = Image.linear_gradient("L").resize((size, size))
    a = Image.new("RGB", (size, size), c1)
    b = Image.new("RGB", (size, size), c2)
    return Image.composite(b, a, g)


def thick_line(draw, p1, p2, width):
    draw.line([p1, p2], fill=255, width=width)
    r = width // 2
    for (x, y) in (p1, p2):
        draw.ellipse([x - r, y - r, x + r, y + r], fill=255)


# --- glyph mask (white on black) --------------------------------------------
glyph = Image.new("L", (S, S), 0)
d = ImageDraw.Draw(glyph)
c = S // 2
k = S / 1024.0  # design units -> pixels

stroke = int(58 * k)
inset = int(240 * k)
arm = int(150 * k)

# 4 corner brackets
for (cx, cy, dx, dy) in [
    (inset, inset, 1, 1),
    (S - inset, inset, -1, 1),
    (inset, S - inset, 1, -1),
    (S - inset, S - inset, -1, -1),
]:
    thick_line(d, (cx, cy), (cx + dx * arm, cy), stroke)
    thick_line(d, (cx, cy), (cx, cy + dy * arm), stroke)

# crosshair segments with a gap around the center dot
seg_in = int(96 * k)   # gap radius (from center)
seg_out = int(228 * k)
lw = int(46 * k)
thick_line(d, (c - seg_out, c), (c - seg_in, c), lw)
thick_line(d, (c + seg_in, c), (c + seg_out, c), lw)
thick_line(d, (c, c - seg_out), (c, c - seg_in), lw)
thick_line(d, (c, c + seg_in), (c, c + seg_out), lw)

# center dot
dot = int(52 * k)
d.ellipse([c - dot, c - dot, c + dot, c + dot], fill=255)

# --- badge -------------------------------------------------------------------
badge_mask = rounded_rect_mask(S, int(230 * k))
badge = vertical_gradient(S, (27, 39, 68), (9, 13, 26))  # slate blue -> near black

icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
icon.paste(badge, (0, 0), badge_mask)

# --- glow under the glyph ------------------------------------------------------
glow_mask = glyph.filter(ImageFilter.GaussianBlur(int(34 * k)))
glow = Image.new("RGBA", (S, S), (56, 189, 248, 0))
glow.putalpha(glow_mask.point(lambda v: v * 55 // 100))
icon = Image.alpha_composite(icon, glow)

# --- gradient glyph ------------------------------------------------------------
grad = diagonal_gradient(S, (56, 199, 255), (167, 139, 250)).convert("RGBA")  # cyan -> violet
grad.putalpha(glyph)
icon = Image.alpha_composite(icon, grad)

# keep everything inside the rounded badge
final = Image.new("RGBA", (S, S), (0, 0, 0, 0))
final.paste(icon, (0, 0), badge_mask)

# --- exports -------------------------------------------------------------------
final.resize((256, 256), Image.LANCZOS).save(f"{OUT}/icon-256.png")
final.resize((64, 64), Image.LANCZOS).save(f"{OUT}/icon-64.png")
final.resize((256, 256), Image.LANCZOS).save(
    f"{OUT}/icon.ico",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)
print("written:", os.listdir(OUT))
