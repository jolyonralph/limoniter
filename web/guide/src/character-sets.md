description: Open and edit text from older computers byte for byte: choosing a character set, line endings, the kinds-of-computer presets, the character table, previews and machine themes.

# Character sets and old machines' text

Text from a Commodore 64, an Atari, a Spectrum or an IBM mainframe isn't ASCII, and a modern editor turns it into nonsense. Limoniter knows 178 single-byte character sets as well as Unicode, and opens and saves text in any of them byte for byte. This page shows you how to choose the right one and work with it.

## Seeing which character set a file is in

The status bar shows the file's character set (its encoding) and its line endings. Some files say for themselves what they are, and some are known by where they came from: text on an Atari disk, for example, opens as ATASCII. When Limoniter can't tell, it's easy to choose.

## Opening a file in the right character set

If a file looks wrong, it's usually been read in the wrong character set. To read it again in another:

1. Click the character set in the status bar, or choose **Format › Encoding › Reopen with…**.
2. Choose the character set it's really in, such as PETSCII, ATASCII, ZX Spectrum, CP437 or an EBCDIC code page.
3. The file is read again from its bytes. Nothing is changed or saved.

## Converting text to another character set

Reopening changes how the bytes are read. Converting changes the bytes:

- **Format › Encoding › Convert to…** changes the file's character set, so it's saved in the new one.
- **Format › Encoding › Save a copy in…** writes a copy in another character set, and leaves the file as it is.

> Bytes that have no Unicode equivalent are carried through exactly, and each line keeps its own line ending, so opening a file and saving it gives back exactly the same bytes.

## Line endings

Each line keeps its own ending: CRLF (Windows and DOS), LF (Unix, Linux and macOS), CR (classic Macintosh and Commodore), ATASCII's 0x9B, EBCDIC's NL (0x15), or fixed-width rows with none at all. To change them, click the line ending in the status bar. **View › Show › Line endings** marks the end of each line.

## Starting a file for a particular machine

**File › New as** starts a file of a kind, with its character set, line endings and colouring already chosen: Commodore text (PETSCII), ZX Spectrum text, Amiga text, DOS text and others, as well as BASIC and assembler.

To have every new file start out right for a machine, choose **Format › Encoding › Kind of computer for new files…** and pick a preset. Each fills in the character set, line endings and display together. There are 21, among them:

- Modern Windows, DOS US, Classic Macintosh, Linux / Unix
- Amiga plain text, Amiga ANSI, PC ANSI art
- Commodore PET / VIC-20 / C64, Atari 8-bit, Atari ST
- ZX Spectrum text, BBC Micro text, Teletext (40-column rows)
- IBM mainframe text (EBCDIC with NL), IBM mainframe records (fixed 80-byte EBCDIC)
- CP/M text, MSX text, Generic 7-bit ASCII

## Seeing every character

To see each byte of the character set the file is in, with its picture, its name and its Unicode value, open **View › Panels › Characters**. It follows the file in front, and also has a browser for all of Unicode.

**View › Panels › Encoding preview** shows the selected text, or the character at the cursor, as its code points and as the bytes it's stored as.

Control codes, such as PETSCII's colour and cursor codes, can be shown by name: tick **View › Show › Control characters as names**.

## Commodore control codes

In Commodore BASIC listings, control codes are written in braces, as the `petcat` tool writes them: `{clr}` clears the screen, `{red}` changes the colour, and so on. You can type them like that, and they're turned back into the right bytes when the program is saved.

## Previews of old screens

Many old text formats were meant to be seen on a screen, not read as text. **View › Preview** shows them as they'd look, beside the text or instead of it:

- PC ANSI art (`.ans`, `.nfo`, `.diz` and others), with its colours and SAUCE record.
- Amiga console text, in the Amiga's own pens, which you can set to Workbench 1.x's or 2.x's colours or your own.
- Commodore PETSCII streams (`.seq`), with colours, reverse and cursor movement.
- Teletext and BBC Mode 7 pages, with mosaics, double height and every national character set.
- ZX Spectrum text, with its colour and position controls.

![A teletext page file and its preview](teletext "A teletext page file beside the page it makes, in colour and double height.")

Nothing in a preview runs or fetches anything.

## Machine colours

Out of the box, Commodore, Atari, Spectrum, BBC, teletext and Amiga text open in their machine's own colours. Limoniter chooses each document's theme by rules about its file ending, language, character set, or whether it's in hex or table view. To change the rules, or to give one document a theme of its own, see **View › Theme** and [Settings](settings.html#themes).

## Sending text to the machine

When you send a file to a real machine with **File › Send to device**, the copy can be written in the machine's own character set and line endings, without changing the file you're editing. Set this under **Write text as** for the device in **Manage devices…**. See [Assembling and running](assembling-and-running.html#sending-to-a-device).
