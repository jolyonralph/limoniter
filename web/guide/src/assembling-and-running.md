description: Run programs with F9 on the built-in ZX Spectrum, an emulator or a real machine; assemble Z80, 6502, 6809 and 68000 source; and send files to the machines they're for.

# Assembling and running

Press [[F9]], and what you're editing runs: a BASIC listing, a tape, a snapshot, a disk image, or assembler source. It can run on the ZX Spectrum built into Limoniter, on an emulator you've installed, or on a real machine. This page shows you how to run things, how to assemble source, and how to send files to a machine.

## Running a program

1. Open the program, or the listing or source, or select a program in a disk image.
2. Press [[F9]], or choose **File › Run on machine**.
3. The first time you run a kind of program, Limoniter asks where it should run. Choose a machine. It remembers, so next time F9 runs it there straight away.

To run it somewhere else, choose from **File › Run on**, which lists every place that could run it, and says why any that can't won't.

Limoniter works out the machine from the file: a Commodore program by where it loads, a Z80 snapshot by its header, a +3DOS file by which BASIC it holds. Plain text asks which BASIC it's for.

A run shows in the status bar, with a **Stop** button. **File › Run on › Stop running** stops it too.

> The file that runs is written to a temporary folder of its own. Your file, and any disk image it came from, are never written to.

## The built-in Spectrum

Limoniter has a ZX Spectrum 48K and 128K built in, with nothing to install. It runs tapes and snapshots in a panel beside the file; a BASIC program is loaded and running at once.

- Click its screen to type on it. Ctrl is Symbol Shift, and Esc is Break.
- Its sound, the beeper and the 128K's AY chip, comes out of your speakers. **Mute** turns it off.
- **Pause**, **Restart** and **Switch it off** are under the screen.
- Under the screen is the Spectrum's keyboard, with everything printed on and around each key. Click Caps Shift or Symbol Shift and then a key to press them together, or hold a key down with the mouse for games. **Keyboard** hides it.

## Emulators

Emulators are started as programs of their own: ZEsarUX, Fuse and CSpect for the Spectrum and Next, VICE for Commodores, b2 for the BBC Micro, openMSX, XRoar for the Dragon and Color Computer, and Hatari for the Atari ST.

To set them up:

1. Choose **File › Run on › Devices and emulators…**.
2. Click **Find emulators**. Limoniter adds the ones installed on your computer.
3. To add one it didn't find, click **Add**, choose the kind **Emulator**, and point it to the emulator's program. You can give it extra options of your own.

Only programs listed as emulators, by you or by Find emulators, are ever started.

## Real machines

The devices you send files to can run programs too. An Ultimate 64 or 1541 Ultimate runs a program as it arrives. A Spectrum Next, a card or a folder receives it, and you start it there.

## Assembling source

Limoniter has its own assemblers for the Z80, 6502, 6809 and 68000, with nothing to install. To try one, choose **File › New as** and **Z80 assembler**, **6502 assembler**, **6809 assembler** or **68000 assembler**: the file starts with a few lines to begin with (on the Spectrum and the C64, they turn the border red).

Press [[F9]], and the source is assembled and run:

- **Z80** for the 48K or 128K goes on a tape with a BASIC loader that loads and calls it, and runs where Spectrum programs run. Source that says `DEVICE ZXSPECTRUMNEXT` becomes a `.nex` for the Next. sjasmplus projects build as sjasmplus builds them, including the files they save.
- **6502** becomes a C64 program, with a `10 SYS` line in front if it doesn't start at $0801, and runs where C64 programs run.
- **6809** becomes a Color Computer LOADM binary and runs in XRoar.
- **68000** without an ORG becomes an Atari ST program and runs in Hatari; with an ORG, the binary is saved beside the source.

Mistakes are listed in the Output panel, where clicking one goes to its line, and are marked beside their lines.

### Saving instead of running

- **File › Run on › Assemble and save** saves the program beside its source, without running it: a `.tap`, `.nex`, `.prg` or `.bin`.
- **File › Run on › Assemble and save as binary…** saves just the assembled bytes, from the lowest address to the highest, with no loader or header: for `LOAD "" CODE`, `INCBIN` or a ROM. The Output panel says the address to load them at.

### Using your own assembler

To use an assembler you've installed instead (sjasmplus or Pasmo for the Z80, ACME for the 6502, lwasm for the 6809, vasm for the 68000), choose **File › Run on › Assemblers…** and pick it for that processor. The choice is kept, and used by F9 and both kinds of Assemble and save.

## Help as you write

Z80, 6502, 6809 and 68000 source is assembled quietly a moment after you stop typing, so Limoniter knows your code:

- **Completion**: type two letters of a word, or press [[Ctrl+Space]], for instructions, directives, macros and your labels. [[↑]] and [[↓]] choose, [[Enter]] or [[Tab]] puts it in, and [[Esc]] closes the list.
- **Hover** over a label for its value and where it's defined, or over an instruction for its address, bytes and timing.
- **Search › Go to definition** ([[F12]]), **Find all uses** ([[Shift+F12]]) and **Rename…** ([[Ctrl+F12]]), which renames a label in every file it's used in.
- **The Functions panel** (**View › Panels › Functions**) shows the program's outline: each ORG, every label with its value, and macros.
- The status bar shows the cursor line's address, bytes and T-states.

## Other tools for machine code

- **Tools › Disassemble…** turns machine code back into source for the Z80, 6502 or 6809: from a C64 program, a snapshot, a tape's CODE block, a Color Computer binary or an Atari binary file. The source always assembles back to exactly the bytes it came from.
- **Tools › Character and sprite editor** draws the Spectrum's UDGs and fonts and the Next's 16×16 sprites, and puts them in your source as DB lines.
- **Tools › Spectrum screen designer** draws a Spectrum screen, cell by cell, and opens and saves `.scr` files.

## Sending to a device

To send a copy of a file to the machine it's for, such as a card for a Gotek or SD2IEC, a Spectrum Next over Wi-Fi, or an Ultimate 64:

1. Choose **File › Send to device › Manage devices…**.
2. Click **Add**, give the device a name, and choose its **Kind**: **Card or stick** (found by its volume label, whatever drive it's on today), **Folder**, **SFTP server**, **FTP server**, **ZX Spectrum Next (NextSync)**, **Ultimate 64 / 1541 Ultimate**, or **Command line** for a program of your own.
3. Fill in the details it asks for, and choose under **Write text as** whether text goes as it's saved or in the machine's own character set.
4. Click **Done**.

![The Devices box, setting up a Spectrum Next](devices "Devices are set up once, by name, and then a file is sent with one command.")

Then choose the device under **File › Send to device**. The file you're editing goes on saving where it always does. After the first time, [[Ctrl+Alt+S]] sends the file in front again, to the same place under the same name.

On a card, the copy is written under a temporary name and renamed once it's all there, so you can pull the card out straight away. If the copy on the device has changed since you last sent it, you're asked before it's replaced.

> For a Spectrum Next, Limoniter makes the file ready and waits. On the Next, type `.sync` and this computer's address the first time, and just `.sync` after that. The Next needs NextSync's `.sync` command.
