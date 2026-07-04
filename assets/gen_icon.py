# Derives the EasyQuickScreenshot icon set from the ducky master art.
# Pads the (non-square) source onto a square transparent canvas, then emits the
# PNG sizes the app embeds plus a multi-resolution .ico for the exe/taskbar.
# Run from the assets/ folder:  python gen_icon.py

from PIL import Image
import os

HERE = os.path.dirname(os.path.abspath(__file__))
MASTER = os.path.join(HERE, "ducky.ico")

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


def square_master():
    """Load the ducky, trim to its content, and center it on a square RGBA canvas
    with a small even margin so it reads well as an app icon."""
    im = Image.open(MASTER).convert("RGBA")
    bbox = im.split()[3].getbbox()
    content = im.crop(bbox) if bbox else im

    # ~8% margin around the art; square canvas sized to the larger content dimension
    side = max(content.size)
    canvas = int(side / 0.84)
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    out.paste(
        content,
        ((canvas - content.width) // 2, (canvas - content.height) // 2),
        content,
    )
    return out.resize((256, 256), Image.LANCZOS)


def main():
    master = square_master()
    master.save(os.path.join(HERE, "icon-256.png"))
    master.resize((64, 64), Image.LANCZOS).save(os.path.join(HERE, "icon-64.png"))
    master.save(os.path.join(HERE, "icon.ico"), sizes=[(s, s) for s in ICO_SIZES])
    print("written:", sorted(f for f in os.listdir(HERE) if f.startswith("icon")))


if __name__ == "__main__":
    main()
