description: List BASIC programs from Commodore, Spectrum, BBC, Atari, MSX, Apple II, TRS-80 and other machines, edit them as text, and turn them back into programs to save, run or send.

# BASIC listings

Old BASIC programs aren't saved as text: each keyword is kept as a single byte, a token. Limoniter lists them as the machine's own LIST would, lets you edit the listing as text, and turns it back into a program, byte for byte. This page shows you how.

## Listing a program

Open the program, from a file or from inside a disk image or tape. Choose **View › Preview** to see it listed beside the bytes, or instead of them.

It lists Commodore BASIC V2 (C64, VIC-20 and PET), ZX Spectrum 48K and 128K, Spectrum Next NextBASIC, BBC BASIC, MSX-BASIC, Atari BASIC, MBASIC, Applesoft, Color BASIC and Dragon BASIC, TRS-80 BASIC, ZX81 BASIC, SAM BASIC and Oric BASIC.

![A Commodore 64 BASIC listing with the Send to Ultimate 64 box](send "A C64 program listed as LIST shows it, about to be sent to an Ultimate 64.")

A damaged program is listed as far as its lines link up, and says where it stops.

## Editing a listing

1. With the program in front, choose **Format › Tokenised BASIC › Open the listing as text**.
2. The listing opens in a new tab, as ordinary text. Edit it as you would any text. Typing a couple of letters of a keyword offers the keywords that finish it (Spectrum, Next and Commodore).
3. When you're done, save it as a program, run it, or send it, as below.

A listing remembers the program it came from, so you don't need to save it as a program before running or sending it: F9 and **Send to device** tokenise it on the way.

## Saving a listing as a program

Choose one of these under **Format › Tokenised BASIC**:

- **Save as a Spectrum tape (.tap)…**
- **Save as a Spectrum Next program (.bas)…**
- **Save as a Commodore program (.prg)…**
- **Save as a BBC BASIC program…**

Listing and tokenising undo each other: tokenise a listing you haven't changed, and you get back the original bytes. The other BASICs (MSX, Atari, TRS-80, ZX81, SAM and Oric among them) are listed only.

## Writing a new program

To start a new program, choose **File › New as** and one of **Spectrum BASIC**, **Spectrum Next BASIC**, **Commodore 64 BASIC**, **BBC BASIC** or **OpticBASIC (OpticOS)**. It starts with a line to try, and it's ready to run with [[F9]] and to send as a program.

Listings written for the zmakebas or bas2tap tools tokenise as those tools do, escapes and all.

## Spectrum graphics and UDGs

In a Spectrum listing, block graphics are shown as the matching Unicode characters (▝ ▘ ▀ …), user-defined graphics as `{UDG-A}`, and colour codes as `{INK 2}`. To have listings written zmakebas's or bas2tap's way instead, change **Spectrum BASIC listings write graphics as** in **Tools › Settings…**, under Editing.

To type graphics as the Spectrum's GRAPHICS key does:

1. Press [[Ctrl+Shift+9]], or choose **Format › Tokenised BASIC › Spectrum GRAPH mode**. The status bar says GRAPH.
2. Press 1 to 8 for the block graphics, with Shift for the inverted ones, and A to U for the UDGs.
3. Press 9 or [[Esc]] to leave GRAPH mode.

UDGs are drawn as their pictures when Limoniter can find them: on the tape the program came from, in its snapshot, or in its own DATA lines. To take them from another file, choose **Format › Tokenised BASIC › Spectrum UDG pictures from…**. Put the cursor in one to see it as text again and edit it. **Tools › Character and sprite editor** draws new ones.

## Commodore control codes

In a Commodore listing, control codes inside strings are written in braces, as `petcat` writes them: `{clr}`, `{red}` and so on. Type them the same way, and they become the right bytes when the listing is tokenised.

## Running and sending a program

- [[F9]] runs the program on the machine chosen for it: the built-in Spectrum, an emulator, or a real machine. See [Assembling and running](assembling-and-running.html).
- **File › Send to device** sends it, tokenised, to a real machine or its card: a NextBASIC listing goes to a Next as a `.bas`, a C64 listing as a `.prg`.
- While a program runs on the built-in Spectrum, in ZEsarUX or in VICE, **File › Run on › Take its BASIC program** gives you the program in the machine's memory as a listing, and **Put this listing into it** puts your edited listing back without a reset.

To step through a Spectrum BASIC program line by line, with its variables, see [Debugging](debugging.html#debugging-spectrum-basic).
