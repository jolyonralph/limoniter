"""Makes the website's pictures from full-size screenshots (1920x1080 PNGs, as Limoniter's --screenshot takes them).

    python web/tools/make-images.py SCREENSHOTS_FOLDER

For each screenshot it writes web/images/NAME.webp, the whole window, shown when a picture is enlarged, and
NAME-close.webp, a 16:9 part of it at a size that can be read in the slideshow and gallery. It also makes
themes.webp, four document themes side by side, and social.png, the picture shown when a link to the site is
shared. Needs Pillow.
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

SOURCE = Path(sys.argv[1])
APP = Path(__file__).resolve().parents[2]
OUT = APP / "web" / "images"

# Where the interesting part of each screenshot is: the top-left corner of a 1280x720 part of the 1920x1080 window,
# or None for the whole window made smaller.
CLOSE = {
    "teletext": None,
    "undo-tree": (0, 40),
    "send": (560, 40),
    "table": (0, 60),
    "compare": (60, 60),
    "amiga-disk": (0, 40),
    "huge-hex": (0, 50),
    "command-box": (300, 40),
    "devices": (320, 180),
    "themes-editor": (330, 40),
    "linux": None,
}
THEMES = ["t-c64", "t-solar", "t-nord", "t-amber"]


def webp(image, path, quality=82):
    image.convert("RGB").save(path, "WEBP", quality=quality, method=6)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, corner in CLOSE.items():
        image = Image.open(SOURCE / f"{name}.png")
        webp(image, OUT / f"{name}.webp")
        if corner is None:
            close = image
        else:
            x, y = corner
            close = image.crop((x, y, x + 1280, y + 720))
        webp(close.resize((1280, 720)), OUT / f"{name}-close.webp")

    # Four document themes, each the top-left of its window.
    collage = Image.new("RGB", (1920, 1080))
    for i, name in enumerate(THEMES):
        part = Image.open(SOURCE / f"{name}.png").crop((0, 40, 1280, 760)).resize((960, 540))
        collage.paste(part, ((i % 2) * 960, (i // 2) * 540))
    webp(collage, OUT / "themes.webp")
    webp(collage.resize((1280, 720)), OUT / "themes-close.webp")

    # The picture for links shared elsewhere: the name and what it is, beside a part of the teletext screenshot.
    social = Image.new("RGB", (1200, 630), (22, 17, 14))
    draw = ImageDraw.Draw(social)
    fonts = Path("/usr/share/fonts/truetype/dejavu")
    bold = ImageFont.truetype(str(fonts / "DejaVuSans-Bold.ttf"), 64)
    plain = ImageFont.truetype(str(fonts / "DejaVuSans.ttf"), 28)
    mono = ImageFont.truetype(str(fonts / "DejaVuSansMono-Bold.ttf"), 30)
    logo = Image.open(APP / "data" / "logo.png").convert("RGBA").resize((120, 120))
    social.paste(logo, (64, 70), logo)
    draw.text((64, 220), "Limoniter", font=bold, fill=(238, 229, 218))
    draw.text((64, 310), "A powerful text and hex editor", font=plain, fill=(186, 170, 152))
    draw.text((64, 350), "that speaks ", font=plain, fill=(186, 170, 152))
    draw.text((64 + draw.textlength("that speaks ", font=plain), 348), "PETSCII", font=mono, fill=(230, 132, 52))
    draw.text((64, 430), "Free and open source", font=plain, fill=(186, 170, 152))
    draw.text((64, 470), "Windows · macOS · Linux", font=plain, fill=(186, 170, 152))
    draw.text((64, 540), "limoniter.com", font=plain, fill=(230, 132, 52))
    page = Image.open(SOURCE / "teletext.png").crop((948, 200, 1492, 770))
    page = page.resize((int(page.width * 0.98), int(page.height * 0.98)))
    social.paste(page, (1200 - page.width - 48, (630 - page.height) // 2))
    social.save(OUT / "social.png", optimize=True)


main()
