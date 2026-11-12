# Running programs

How Limoniter runs what you're editing on a machine: the Spectrum built in, an emulator of your own, or the real
thing. **Run on machine** (F9) runs on the machine chosen for that kind of program; **Run on** in the File menu
lists every machine that could run it.

## What's run

When you run, the document becomes a *runnable*: a machine and a file it loads.

| You're editing | It runs |
|---|---|
| A BASIC listing opened from a program | The program, tokenised as it was: NextBASIC as a `.bas`, a 48K tape listing as a `.tap`, Commodore as a `.prg` |
| A program, tape, snapshot or disk image (`.tap`, `.tzx`, `.z80`, `.sna`, `.bas`, `.nex`, `.prg`, `.t64`, `.d64`, `.crt`, `.ssd`, `.dsk`, `.trd`, `.rom`, `.cas` and others) | That file, as it is |
| A file selected in a disk image or archive | That file, taken out of it |
| Anything else | Nothing yet: Run asks which machine it's BASIC for, and runs it as a listing |

The machine is worked out from the file: a Commodore program by where it loads, a Z80 snapshot by its header, a
+3DOS file by which BASIC it holds.

## Where it runs

Every place a program can run is in one list, **File, Send to device, Devices**:

- **Built in:** a ZX Spectrum 48K and 128K, running in the preview pane, with nothing to install.
- **Emulators:** Fuse, ZEsarUX, VICE, CSpect, b2, openMSX and others, started as programs of their own. Limoniter
  finds the ones installed, and any other can be added by its path.
- **Real machines:** the devices programs are sent to. An Ultimate 64 runs a program as it arrives; a Spectrum
  Next, a card or a folder only receives it, and the program is started there.

The first time a kind of program is run, Limoniter asks where, and remembers.

## While it runs

A run is a session, shown in the status bar with a Stop button. Running again reloads into the session if the
machine can take it, resets and loads if not, and starts it afresh otherwise.

Machines that can be reached while they run (the built-in Spectrum, ZEsarUX through its remote protocol, VICE
through its binary monitor) can also:

- show their memory in hex view, as it changes, and take edits to it;
- show their screen as text in the machine's own character set;
- give back the BASIC program in memory as a listing, and take an edited one back without a reset.

## Ground rules

- Only programs listed as emulators, by you or by Limoniter's search, are ever started.
- The file run is written to a temporary folder of its own; disk images are never written.
- Add-ons can act on a file before it runs, with `on_run`, as `on_send` does before a send.
