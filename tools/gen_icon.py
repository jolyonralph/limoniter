"""Makes Limoniter's icons from its logo, data/logo-source.png: data/limoniter.ico (built into the program file and
the installer), data/icon.png (the window and taskbar) and data/logo.png (the About box).

The logo is a block of iron ore in rust oranges and deep browns. The specks of near-clear colour the drawing left
around it are taken away, and it is trimmed to a square. Needs Pillow. Run it again only when the logo changes:
    python -I tools/gen_icon.py data
"""
import io
import os
import struct
import sys

from PIL import Image, ImageFilter


def cleaned(source):
    image = source.convert('RGBA')
    alpha = image.getchannel('A')
    # Faint specks go; so does anything not close to the solid block itself.
    solid = alpha.point(lambda value: 255 if value >= 250 else 0).filter(ImageFilter.MaxFilter(9))
    alpha = Image.composite(alpha.point(lambda value: 0 if value < 64 else value), Image.new('L', alpha.size, 0), solid)
    image.putalpha(alpha)
    left, top, right, bottom = alpha.getbbox()
    side = round(max(right - left, bottom - top) * 1.04)
    square = Image.new('RGBA', (side, side), (0, 0, 0, 0))
    square.paste(image.crop((left, top, right, bottom)), ((side - (right - left)) // 2, (side - (bottom - top)) // 2))
    return square


def sized(logo, size):
    return logo.resize((size, size), Image.LANCZOS)


def png_bytes(image):
    out = io.BytesIO()
    image.save(out, 'PNG', optimize=True)
    return out.getvalue()


def dib_bytes(image):
    """The older form Windows' tools expect for all but the largest size: bottom row first, with a mask after."""
    size = image.width
    header = struct.pack('<IiiHHIIiiII', 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    rows = [[image.getpixel((x, y)) for x in range(size)] for y in range(size)]
    pixels = b''.join(bytes((b, g, r, a)) for row in reversed(rows) for (r, g, b, a) in row)
    stride = ((size + 31) // 32) * 4
    mask = bytearray()
    for row in reversed(rows):
        line = bytearray(stride)
        for x, pixel in enumerate(row):
            if pixel[3] == 0:
                line[x // 8] |= 0x80 >> (x % 8)
        mask += line
    return header + pixels + bytes(mask)


def ico_bytes(images):
    out = struct.pack('<HHH', 0, 1, len(images))
    offset = 6 + 16 * len(images)
    body = b''
    for size, data in images:
        shown = 0 if size >= 256 else size
        out += struct.pack('<BBBBHHII', shown, shown, 0, 0, 1, 32, len(data), offset + len(body))
        body += data
    return out + body


def main():
    folder = sys.argv[1]
    logo = cleaned(Image.open(os.path.join(folder, 'logo-source.png')))
    images = [(size, png_bytes(sized(logo, size)) if size >= 256 else dib_bytes(sized(logo, size))) for size in (16, 24, 32, 48, 64, 256)]
    with open(os.path.join(folder, 'limoniter.ico'), 'wb') as out:
        out.write(ico_bytes(images))
    with open(os.path.join(folder, 'icon.png'), 'wb') as out:
        out.write(png_bytes(sized(logo, 64)))
    with open(os.path.join(folder, 'logo.png'), 'wb') as out:
        out.write(png_bytes(sized(logo, 256)))
    print('ok')


main()
