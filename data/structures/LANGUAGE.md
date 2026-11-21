# Structure templates

A structure template describes a kind of file once, so that any file of that kind can be taken apart: its fields named, their values read and shown in the Structure panel beside the hex view, and their bytes coloured in the hex view. Click a field to select its bytes; click a byte to find its field; rest the pointer on a byte to see which field it belongs to.

Limoniter comes with templates for Amiga hunk executables, IFF files (ILBM pictures, 8SVX and AIFF sounds), Amiga disk images (ADF), ZX Spectrum tapes (TAP, TZX) and snapshots (SNA, Z80), Atari ST programs, Commodore programs (PRG) and 1541 disk images (D64), BMP, PNG, ZIP, ELF, MS-DOS and Windows executables (MZ, PE) and WAV.

Your own templates are `.structure` files in the `structures` folder in Limoniter's data folder (the Structure panel's menu opens it). They're tried before the built-in ones, so a changed copy of a built-in template takes its place. A template is read again whenever its file is saved, and a mistake in it is shown in the panel with its line number.

## A first template

```
# Lines starting with # (or //) are comments.
name "Sprite bank"
endings spr bank
magic str(0, 4) == "SPRB"
order little

struct Sprite {
    u8 width
    u8 height
    u8 pixels[width * height]
}

char signature[4]
u16 count "How many sprites follow"
Sprite sprites[count]
```

Each line is one statement. Fields are read one after another from the start of the file, each one starting where the one before ended.

## What the template is for

- `name "..."`: what the format is called, shown in the panel. Every template needs one.
- `endings a b "8svx"`: the file endings it's for, without dots (put an ending that starts with a digit in quotes). A template with endings is only chosen for files with one of them.
- `magic expression`: a test of the file's bytes, true for files of this kind. A template without endings is chosen by its magic alone. A file too short for the test simply doesn't fit.
- `order big` or `order little`: the byte order numbers are in. Big-endian (Motorola, as on the Amiga and Atari ST) is the default. `order` can also be used among the fields, to change it from there on (in the struct being read and those inside it).

The first template that fits a file is chosen, yours first. Another can be picked in the panel.

## Fields

`type name`, then optionally `[count]`, `@ offset`, `: format` and a description in quotes:

```
u32 size : hex "The length of what follows"
```

The types:

- `u8`, `u16`, `u24`, `u32`, `u64`: unsigned whole numbers. `s8` to `s64` (or `i8` to `i64`) are signed.
- `f32`, `f64`: IEEE floating-point numbers.
- Add `le` or `be` to give a field its own byte order whatever the template's: `u16le`, `u32be`.
- `char`: one character. `char name[16]` is text 16 bytes long, shown up to its first zero byte.
- `cstring`: text ending with a zero byte (which is part of the field).
- `pstring`: a length byte, then that many characters.
- The name of a `struct`, `enum` or `flags` type of the template's own.

## Lists

- `u16 table[count]`: `count` can be any expression, usually using fields read before. A list of numbers is one field; resting the pointer on one of its bytes says which entry it is.
- `Chunk chunks[]`: as many as fit, to the end of the file.
- `Chunk chunks[while u32(here) != 0]`: as long as the test holds; it's tried before each entry. In the test, `index` is how many entries have been read so far.

A list of structs shows each entry as `[0]`, `[1]` and so on. `index` inside an entry is its number.

## Structs

```
struct Chunk {
    char id[4]
    u32 size
    label id
    ...
}
```

A struct is a group of fields read one after another. `label expression` gives the struct a value to show beside its name in the panel, which makes long lists easy to read. Structs can be inside structs, and a struct can include itself (as a chain of directory sectors does), up to 64 deep.

Inside a struct, every field read so far can be used by its name: first the struct's own, then those of the structs it's inside, out to the file's own. `header.width` is a field of a struct read before, and `chunks[2].size` a field of a list's entry.

## Enums and flags

```
enum Compression : u8 {
    none = 0
    byte_run1 = 1 "Packed with ByteRun1"
}

flags ViewModes : u32 {
    LACE = 0x4
    HAM = 0x800
    HIRES = 1 << 15
}
```

An enum's field shows the name of its value (and what's said about it); a flags field shows the names of every bit set. Use them as types (`Compression compression`) or as a way of showing a number of another type (`u32 modes : ViewModes`). The entries' names can be used in expressions: `if compression == byte_run1 { ... }`.

## Ways of showing a number

After a colon: `hex`, `dec`, `bin`, `oct`, `char`, `dosdate` and `dostime` (MS-DOS and ZIP dates and times), `unix` (seconds since 1970), `amigadays` (days since 1978, as AmigaDOS keeps dates), or an enum or flags type. Without one, a number is shown in decimal with its hex beside it.

## Choosing and moving

```
if type == 1 {
    u16 width
} else if type == 2 {
    u32 width
} else {
    stop "A type this template doesn't know"
}
```

The opening brace goes on the same line as `if`; `else` may go on the next.

- `u32 table[16] @ offset`: the field is read at that place in the file instead of the next, and reading then carries on where it was. This is how pointers are followed: `Directory directory @ header.directory_offset`.
- `goto offset`: carry on reading from there.
- `skip n`: move on (or back, below 0) that many bytes without reading them.
- `stop "..."`: read no more, and say why (the message can be left out). It isn't treated as a mistake.
- `check expression "..."`: shows the message in the panel if the test doesn't hold, such as a checksum that doesn't match, and reading carries on.

## Values of your own

- `let name = expression`: a value to use later. It isn't shown.
- `calc name = expression`, or `calc name : hex = expression`: a value worked out and shown in the panel, with no bytes of its own.
- `def name(a, b) = expression`: a function of your own, written outside the structs, as `def sector(t, s) = (t - 1) * 21 + s`.

## Expressions

Numbers are written `42`, `0x2A`, `$2A` or `0b101010`; `'A'` is a character's code. Text is written in double quotes, with `\n`, `\t`, `\0`, `\\`, `\"` and `\xNN` for a byte.

From the loosest to the tightest: `a ? b : c`, `||`, `&&`, the comparisons (`==` `!=` `<` `<=` `>` `>=`), `|`, `^`, `&`, `<<` `>>`, `+` `-`, `*` `/` `%`, and the single `-`, `!` and `~`. This is Rust's order rather than C's, so `flags & 0x20 != 0` tests the bit, as it reads. `&&` and `||` look at their right side only when they need to, so `here + 4 <= file_size && u32(here) == 0` never reads past the end. Text can be compared with `==` and `!=`, and joined with `+`. Whole numbers are 128 bits wide, so nothing overflows by surprise.

Inside round or square brackets an expression can carry on to the next line.

Names known everywhere:

- `here`: the offset the next field would be read at.
- `file_size`: the file's length.
- `index`: the number of the list entry being read.
- `true` and `false`.

Functions:

- `u8(at)`, `u16(at)`, `s32(at)`, `u32le(at)`, `f32be(at)` and so on, one for each type of number: the number at that offset, without reading it as a field. `char(at)` is one character.
- `str(at, n)`: n bytes as text. `cstr(at)`: the text at that offset up to a zero byte (at most 256 bytes).
- `start_of(field)`, `end_of(field)`, `size_of(field)`: where a field's bytes are. `count(list)`: how many entries a list has.
- `exists(name)`: whether there's a field or value of that name, as for a field read only sometimes.
- `len(text)`, `min(a, b)`, `max(a, b)`, `abs(a)`, `align(a, n)` (a rounded up to a whole number of n).
- `xor(at, n)`, `sum(at, n)` and `crc32(at, n)`: the bytes there XORed together, added up, or their CRC-32 (as ZIP and PNG use), for checking checksums.

## Limits

To keep a template that's gone wrong (or a damaged file) from taking all the memory or going on for ever, a file is read into at most 200,000 fields, structs go at most 64 deep, and lists go round at most 20 million times in all. When reading goes wrong, what was read until then is still shown, with the mistake and its line.
