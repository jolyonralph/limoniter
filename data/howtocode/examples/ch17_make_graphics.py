"""ch17_make_graphics.py - makes ch17_graphics.raw: the tiles, sprites, bob, font and sound effects of chapter 17's
examples (ch17_game.s and ch17_bobs.s). All of it is drawn here, by rule, so it's CC0 like the rest of v8.

How To Code v8, chapter 17. CC0: use it for anything.

The file is what the Amiga's chips want, so it can be put straight into chip RAM with INCBIN. In order:

    offset  bytes  what
         0     32  the playfield's 16 colours, as the COLORxx registers take them ($0RGB)
        32    512  4 tiles of 16 by 16 pixels in 4 bitplanes, interleaved: each line is plane 0's word, then plane
                   1's, 2's and 3's. Tile 0 is the floor, 1 a wall, 2 a gem on the floor, 3 cracked floor.
       544    192  3 sprite images of 16 lines, two words a line as sprite DMA reads them (no control words):
                   the player, and the enemy's two frames
       736     16  the sprites' colours: the player's 3 (COLOR17-19), the enemy's 3 (COLOR21-23), and 2 spare
       752    512  a bob of 32 by 32 pixels in 4 bitplanes, interleaved like the tiles (each line: plane 0's two
                   words, then 1's, 2's, 3's)
      1264    512  its mask, the same shape: each line's mask once for each bitplane, so one blit does all four
      1776    512  a font of 64 characters, 8 by 8 pixels, one byte a line: ASCII 32 (space) to 95 (_)
      2288   1600  the pick-up sound: 8-bit signed samples, played at period 300 (11.8 kHz on PAL)
      3888   3200  the hurt sound, the same way
      7088         (the end)

    python3 ch17_make_graphics.py        (writes ch17_graphics.raw beside this script)
"""
import math, os, struct

PALETTE = [
    0x124,                          # 0: deep blue: the border, round the playfield and the panel
    0x212, 0x323,                   # 1, 2: the floor and its specks
    0x211, 0x534, 0x756, 0x423,     # 3-6: the wall: mortar, rock, the light edge, the dark edge
    0x620, 0xa40, 0xe80,            # 7-9: the bob, dark to light
    0x0aa, 0x5ff, 0x066,            # 10-12: the gem: middle, light facet, dark facet
    0x310, 0xfc8, 0xfff,            # 13: the bob's edge, 14: its shine, 15: white
]
SPRITE_COLOURS = [0xfb8, 0x36f, 0xff4,     # the player: face, coat, helmet
                  0xa3c, 0xf6f, 0xfff,     # the enemy: body, wings, eyes
                  0x000, 0x000]


def speck(x, y, seed):
    """A repeatable scatter of specks: a small hash of the pixel's place."""
    h = (x * 73856093) ^ (y * 19349663) ^ (seed * 83492791)
    return (h >> 5) % 11 == 0


def floor(x, y):
    return 2 if speck(x, y, 1) else 1


def wall(x, y):
    """Bricks 16 wide and 8 high, every other row moved along by 8."""
    if y % 8 == 0:
        return 3
    bx = (x + (8 if (y // 8) % 2 else 0)) % 16
    if bx == 0:
        return 3
    if y % 8 == 1 or bx == 1:
        return 5
    if y % 8 == 7 or bx == 15:
        return 6
    return 6 if speck(x, y, 2) else 4


def gem(x, y):
    dx, dy = x - 7.5, y - 7.5
    if abs(dx) + abs(dy) <= 6.5:
        if (x, y) in ((6, 4), (5, 5), (6, 5)):
            return 15
        if dx + dy < -2:
            return 11
        if dx + dy > 2:
            return 12
        return 10
    return floor(x, y)


def cracked(x, y):
    crack = {(3, 4), (4, 5), (5, 5), (6, 6), (6, 7), (7, 8), (11, 10), (12, 11), (12, 12), (10, 10)}
    return 3 if (x, y) in crack else floor(x, y)


def planar(pixels, width, planes):
    """Rows of colour numbers made into interleaved bitplanes: for each row, each plane's words in turn."""
    out = bytearray()
    for row in pixels:
        for plane in range(planes):
            for word in range(width // 16):
                value = 0
                for bit in range(16):
                    value = (value << 1) | ((row[word * 16 + bit] >> plane) & 1)
                out += struct.pack('>H', value)
    return out


def sprite(rows):
    """A sprite from 16 strings of 16 characters: '.' is see-through, '1' to '3' its colours."""
    out = bytearray()
    for row in rows:
        a = b = 0
        for ch in row:
            c = 0 if ch == '.' else int(ch)
            a = (a << 1) | (c & 1)
            b = (b << 1) | (c >> 1)
        out += struct.pack('>HH', a, b)
    return out


PLAYER = [
    "................",
    "......3333......",
    ".....333333.....",
    "....33333333....",
    "....33111133....",
    "....31111111....",
    "....11311311....",
    ".....111111.....",
    "......1111......",
    "....22222222....",
    "...2222222222...",
    "..112222222211..",
    "..11.222222.11..",
    ".....222222.....",
    ".....22..22.....",
    "....222..222....",
]
ENEMY_A = [
    "................",
    "................",
    "2..............2",
    "22............22",
    "222...1111...222",
    "2222.111111.2222",
    "2222213313312222",
    ".222213313312222",
    "..22211111112222",
    "...2111111111...",
    "....11111111....",
    ".....111111.....",
    "......1..1......",
    ".....1....1.....",
    "................",
    "................",
]
ENEMY_B = [
    "................",
    "................",
    "................",
    "................",
    "......1111......",
    ".....111111.....",
    "....11331331....",
    "...2113313312...",
    "..222111111222..",
    ".22221111112222.",
    "2222.111111.2222",
    "222...1111...222",
    "22....1..1....22",
    "2....1....1....2",
    "................",
    "................",
]


def ball(x, y):
    """The bob: a shaded ball, lit from up and to the left, with a dark edge."""
    dx, dy = x - 15.5, y - 15.5
    r = math.hypot(dx, dy)
    if r > 15.5:
        return 0
    if r > 14.2:
        return 13
    light = math.hypot(dx + 6, dy + 6)
    if light < 3:
        return 15
    if light < 6:
        return 14
    if light < 11:
        return 9
    if light < 17:
        return 8
    return 7


# A font of 5 by 7 letters, drawn here: each character is 7 rows of 5 pixels ('#' set).
FONT = {
    ' ': ["     "] * 7,
    '0': [" ### ", "#   #", "#  ##", "# # #", "##  #", "#   #", " ### "],
    '1': ["  #  ", " ##  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "],
    '2': [" ### ", "#   #", "    #", "  ## ", " #   ", "#    ", "#####"],
    '3': [" ### ", "#   #", "    #", "  ## ", "    #", "#   #", " ### "],
    '4': ["   # ", "  ## ", " # # ", "#  # ", "#####", "   # ", "   # "],
    '5': ["#####", "#    ", "#### ", "    #", "    #", "#   #", " ### "],
    '6': ["  ## ", " #   ", "#    ", "#### ", "#   #", "#   #", " ### "],
    '7': ["#####", "    #", "   # ", "  #  ", " #   ", " #   ", " #   "],
    '8': [" ### ", "#   #", "#   #", " ### ", "#   #", "#   #", " ### "],
    '9': [" ### ", "#   #", "#   #", " ####", "    #", "   # ", " ##  "],
    'A': [" ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"],
    'B': ["#### ", "#   #", "#   #", "#### ", "#   #", "#   #", "#### "],
    'C': [" ### ", "#   #", "#    ", "#    ", "#    ", "#   #", " ### "],
    'D': ["#### ", "#   #", "#   #", "#   #", "#   #", "#   #", "#### "],
    'E': ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####"],
    'F': ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#    "],
    'G': [" ### ", "#   #", "#    ", "# ###", "#   #", "#   #", " ####"],
    'H': ["#   #", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"],
    'I': [" ### ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "],
    'J': ["  ###", "   # ", "   # ", "   # ", "   # ", "#  # ", " ##  "],
    'K': ["#   #", "#  # ", "# #  ", "##   ", "# #  ", "#  # ", "#   #"],
    'L': ["#    ", "#    ", "#    ", "#    ", "#    ", "#    ", "#####"],
    'M': ["#   #", "## ##", "# # #", "# # #", "#   #", "#   #", "#   #"],
    'N': ["#   #", "##  #", "# # #", "#  ##", "#   #", "#   #", "#   #"],
    'O': [" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "],
    'P': ["#### ", "#   #", "#   #", "#### ", "#    ", "#    ", "#    "],
    'Q': [" ### ", "#   #", "#   #", "#   #", "# # #", "#  # ", " ## #"],
    'R': ["#### ", "#   #", "#   #", "#### ", "# #  ", "#  # ", "#   #"],
    'S': [" ####", "#    ", "#    ", " ### ", "    #", "    #", "#### "],
    'T': ["#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "],
    'U': ["#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "],
    'V': ["#   #", "#   #", "#   #", "#   #", "#   #", " # # ", "  #  "],
    'W': ["#   #", "#   #", "#   #", "# # #", "# # #", "## ##", "#   #"],
    'X': ["#   #", "#   #", " # # ", "  #  ", " # # ", "#   #", "#   #"],
    'Y': ["#   #", "#   #", " # # ", "  #  ", "  #  ", "  #  ", "  #  "],
    'Z': ["#####", "    #", "   # ", "  #  ", " #   ", "#    ", "#####"],
    '!': ["  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "     ", "  #  "],
    ':': ["     ", "  #  ", "  #  ", "     ", "  #  ", "  #  ", "     "],
    '-': ["     ", "     ", "     ", " ### ", "     ", "     ", "     "],
    '.': ["     ", "     ", "     ", "     ", "     ", " ##  ", " ##  "],
}


def font():
    out = bytearray()
    for code in range(32, 96):
        rows = FONT.get(chr(code), ["     "] * 7)
        for row in rows + ["     "]:
            value = 0
            for ch in row:
                value = (value << 1) | (ch == '#')
            out.append(value << 2)          # 5 pixels in the middle of the 8: one blank on the left, two right
    return out


def sound(length, pitch, noise=False):
    """A sound effect: a square wave (or noise) sliding in pitch, fading out. pitch(t) is in Hz, t from 0 to 1.
    The first word is 0, as ptplayer asks of a sound effect and as trackers keep the first word of a sample."""
    rate = 3546895 / 300                     # period 300 on a PAL Amiga
    out = bytearray(2)
    phase, seed = 0.0, 1
    for n in range(length - 2):
        t = n / (length - 2)
        phase += pitch(t) / rate
        if noise:
            if int(phase) != int(phase - pitch(t) / rate):
                seed = (seed * 1103515245 + 12345) & 0x7fffffff
            level = 1 if seed & 0x10000 else -1
        else:
            level = 1 if (phase % 1) < 0.5 else -1
        value = int(level * 100 * (1 - t) ** 1.5)
        out.append(value & 0xff)
    return out


data = bytearray(struct.pack('>16H', *PALETTE))
for tile in (floor, wall, gem, cracked):
    data += planar([[tile(x, y) for x in range(16)] for y in range(16)], 16, 4)
for image in (PLAYER, ENEMY_A, ENEMY_B):
    assert len(image) == 16 and all(len(row) == 16 for row in image)
    data += sprite(image)
data += struct.pack('>8H', *SPRITE_COLOURS)
pixels = [[ball(x, y) for x in range(32)] for y in range(32)]
data += planar(pixels, 32, 4)
data += planar([[15 if c else 0 for c in row] for row in pixels], 32, 4)   # the mask: all four planes set
data += font()
data += sound(1600, lambda t: 700 + 1400 * t if t < 0.5 else 1100 + 1800 * (t - 0.5))
data += sound(3200, lambda t: 2400 - 2000 * t, noise=True)
assert len(data) == 7088, len(data)

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'ch17_graphics.raw')
with open(out, 'wb') as f:
    f.write(data)
print(out, len(data), 'bytes')
