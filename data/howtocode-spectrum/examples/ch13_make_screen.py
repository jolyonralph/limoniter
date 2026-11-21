"""Makes ch13_screen.scr, the loading screen for ch13_tape.asm: 6,912 bytes, the Spectrum's screen as it is in
memory (6,144 bytes of pixels in the screen's own line order, then 768 attributes). A pattern of diagonal
stripes and boxes, drawn here so that it's CC0 like the rest of How To Code.

    python ch13_make_screen.py
"""
from pathlib import Path


def address(x, y):
    """The byte holding pixel (x, y): y's bits in the order 7 6 2 1 0 5 4 3 (chapter 5)."""
    return ((y & 0xC0) << 5) | ((y & 7) << 8) | ((y & 0x38) << 2) | (x >> 3)


def main():
    pixels = bytearray(6144)
    for y in range(192):
        for x in range(256):
            cx, cy = abs(x - 128), abs(y - 96)
            box = max(cx * 3 // 4, cy) // 6 % 2 == 0      # nested boxes
            stripe = (x + y) // 4 % 2 == 0                 # diagonal stripes
            if box != stripe:
                pixels[address(x, y)] |= 0x80 >> (x & 7)
    attributes = bytearray(768)
    colours = [1, 3, 2, 6, 4, 5, 7, 5, 4, 6, 2, 3]
    for row in range(24):
        for column in range(32):
            ink = colours[(row + column // 4) % len(colours)]
            attributes[row * 32 + column] = 0x40 | ink    # bright ink on black paper
    Path(__file__).with_name("ch13_screen.scr").write_bytes(bytes(pixels + attributes))


main()
