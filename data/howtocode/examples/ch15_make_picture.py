"""ch15_make_picture.py - makes ch15_picture.raw, the picture ch15_load_picture.s loads from disk.

How To Code v8, chapter 15. CC0: use it for anything.

The file is what the Amiga's chips want, so it can be read straight into chip RAM and shown: 16 colours as the
COLORxx registers take them (32 bytes, $0RGB), then a 320 by 240 picture in 4 bitplanes, interleaved: each line
is plane 0's 40 bytes, then plane 1's, plane 2's and plane 3's (38400 bytes).

    python3 ch15_make_picture.py        (writes ch15_picture.raw beside this script)
"""
import math, os, struct

WIDTH, HEIGHT, PLANES = 320, 240, 4

# Colour 0 is the background (the copper bar shows through it); 1-15 a ramp from deep blue through red to yellow.
palette = [0x102]
for i in range(15):
    t = i / 14
    r = min(15, round(15 * min(1, t * 1.6)))
    g = min(15, round(15 * max(0, t * 1.4 - 0.4)))
    b = min(15, round(15 * max(0, 0.8 - t) + 3 * (1 - t)))
    palette.append((r << 8) | (g << 4) | b)

def colour(x, y):
    """A flower of rings in the middle, on the background."""
    dx, dy = x - WIDTH / 2 + 0.5, y - HEIGHT / 2 + 0.5
    r = math.hypot(dx, dy)
    a = math.atan2(dy, dx)
    edge = 100 + 10 * math.sin(a * 6)
    if r > edge:
        return 0
    ring = r / 7 + 1.5 * math.sin(a * 6 + r / 12)
    return 1 + max(0, min(14, int(ring)))

data = bytearray(struct.pack('>16H', *palette))
for y in range(HEIGHT):
    row = [colour(x, y) for x in range(WIDTH)]
    for plane in range(PLANES):
        for byte in range(WIDTH // 8):
            value = 0
            for bit in range(8):
                value = (value << 1) | ((row[byte * 8 + bit] >> plane) & 1)
            data.append(value)

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'ch15_picture.raw')
with open(out, 'wb') as f:
    f.write(data)
print(out, len(data), 'bytes')
