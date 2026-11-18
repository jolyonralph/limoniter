//! The Atari ST's operating system calls, for hover in 68000 source: GEMDOS (trap #1), the BIOS (trap #13) and
//! the XBIOS (trap #14), each function's number, name, arguments in the order they sit on the stack and what it
//! gives back; the Line-A routines ($A000-$A00F); and AES and VDI calls (trap #2, told apart by d0).
//!
//! Numbers, names and argument sizes are the system's (checked against EmuTOS's, Hatari's and FreeMiNT's tables);
//! the descriptions are Limoniter's own.

/// Which trap a call goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trap {
    Gemdos,
    Bios,
    Xbios,
}

impl Trap {
    pub fn number(self) -> u8 {
        match self {
            Trap::Gemdos => 1,
            Trap::Bios => 13,
            Trap::Xbios => 14,
        }
    }

    pub fn from_number(n: i64) -> Option<Trap> {
        match n {
            1 => Some(Trap::Gemdos),
            13 => Some(Trap::Bios),
            14 => Some(Trap::Xbios),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Trap::Gemdos => "GEMDOS",
            Trap::Bios => "BIOS",
            Trap::Xbios => "XBIOS",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Trap::Gemdos => "GEMDOS (trap #1): files, memory, the console and programs. The function's number is the last word pushed",
            Trap::Bios => "BIOS (trap #13): characters to and from devices, disk sectors and exception vectors. The function's number is the last word pushed",
            Trap::Xbios => "XBIOS (trap #14): the screen, palette, sound chip, timers, floppy and keyboard. The function's number is the last word pushed",
        }
    }
}

/// A function's argument: its name, whether it's a long (else a word), and what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arg {
    pub name: &'static str,
    pub long: bool,
    pub about: &'static str,
}

/// A system function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Function {
    pub trap: Trap,
    pub number: u16,
    pub name: &'static str,
    pub about: &'static str,
    /// In the order they're on the stack above the function number: the first is pushed just before it.
    pub args: &'static [Arg],
    pub returns: &'static str,
}

impl Function {
    /// The bytes the call leaves on the stack, which the caller takes off: the number and every argument.
    pub fn stack_bytes(&self) -> u32 {
        2 + self.args.iter().map(|a| if a.long { 4 } else { 2 }).sum::<u32>()
    }

    /// The function described: its name and number, what it does, each argument with where it is on the stack
    /// when the trap is taken, and what comes back.
    pub fn describe(&self) -> String {
        let mut lines = vec![format!("{} {} (${:02X}, {}), trap #{}", self.trap.name(), self.name, self.number, self.number, self.trap.number()), self.about.to_string()];
        lines.push(crate::i18n::tr("  0(sp)  word  the function's number").to_string());
        let mut at = 2;
        for a in self.args {
            lines.push(format!("{:>3}(sp)  {}  {}: {}", at, if a.long { "long" } else { "word" }, a.name, a.about));
            at += if a.long { 4 } else { 2 };
        }
        lines.push(format!("Returns: {}", self.returns));
        lines.push(crate::i18n::tf!("Afterwards take {} bytes off the stack{}", self.stack_bytes(), if self.stack_bytes() <= 8 { format!(" (addq.l #{},sp)", self.stack_bytes()) } else { format!(" (lea {}(sp),sp)", self.stack_bytes()) }));
        lines.join("\n")
    }

    /// The function as an EQU line: `Cconws equ 9`.
    pub fn equate(&self) -> String {
        format!("{:<16}equ     {:<6}; {} {}", self.name, self.number, self.trap.name(), self.about.split(['.', ':']).next().unwrap_or(""))
    }
}

const fn w(name: &'static str, about: &'static str) -> Arg {
    Arg { name, long: false, about }
}

const fn l(name: &'static str, about: &'static str) -> Arg {
    Arg { name, long: true, about }
}

const fn f(trap: Trap, number: u16, name: &'static str, about: &'static str, args: &'static [Arg], returns: &'static str) -> Function {
    Function { trap, number, name, about, args, returns }
}

use Trap::{Bios as B, Gemdos as G, Xbios as X};

const NOTHING: &str = "nothing";
const HANDLE: Arg = Arg { name: "handle", long: false, about: "a file handle from Fopen or Fcreate (0-5 are the standard ones)" };
const DEVICE: Arg = Arg { name: "device", long: false, about: "0 printer, 1 serial, 2 console, 3 MIDI, 4 keyboard, 5 the screen without escapes" };

pub const FUNCTIONS: &[Function] = &[
    // GEMDOS.
    f(G, 0x00, "Pterm0", "Ends the program, returning 0 to whatever ran it", &[], "doesn't return"),
    f(G, 0x01, "Cconin", "Waits for a key and echoes it to the screen", &[], "the key: ASCII in bits 0-7, scan code in bits 16-23"),
    f(G, 0x02, "Cconout", "Writes a character to the screen", &[w("char", "the character (bits 0-7)")], NOTHING),
    f(G, 0x03, "Cauxin", "Waits for a byte from the serial port", &[], "the byte"),
    f(G, 0x04, "Cauxout", "Sends a byte out of the serial port", &[w("char", "the byte")], NOTHING),
    f(G, 0x05, "Cprnout", "Sends a byte to the printer", &[w("char", "the byte")], "-1 if it went, 0 if the printer didn't take it"),
    f(G, 0x06, "Crawio", "Reads a key without waiting, or writes a character, with no special keys", &[w("data", "$FF to read a key; anything else is written")], "reading: the key as Cconin gives it, or 0 if none is waiting"),
    f(G, 0x07, "Crawcin", "Waits for a key, without echoing it or acting on Control-C", &[], "the key as Cconin gives it"),
    f(G, 0x08, "Cnecin", "Waits for a key without echoing it", &[], "the key as Cconin gives it"),
    f(G, 0x09, "Cconws", "Writes a string to the screen", &[l("string", "the string's address, ending with a 0 byte; VT52 escapes work")], "the characters written"),
    f(G, 0x0A, "Cconrs", "Reads a line typed at the keyboard", &[l("buffer", "where it goes: byte 0 the most to read, byte 1 is set to how many came, then the line")], NOTHING),
    f(G, 0x0B, "Cconis", "Whether a key is waiting", &[], "-1 if one is, 0 if not"),
    f(G, 0x0E, "Dsetdrv", "Chooses the current drive", &[w("drive", "0 for A:, 1 for B: and on")], "a bit for each drive there is"),
    f(G, 0x10, "Cconos", "Whether the screen can take a character", &[], "-1 (it always can)"),
    f(G, 0x11, "Cprnos", "Whether the printer can take a byte", &[], "-1 if it can, 0 if not"),
    f(G, 0x12, "Cauxis", "Whether a byte has come in at the serial port", &[], "-1 if one has, 0 if not"),
    f(G, 0x13, "Cauxos", "Whether the serial port can send a byte", &[], "-1 if it can, 0 if not"),
    f(G, 0x14, "Maddalt", "Adds a block of alternative (TT) RAM for GEMDOS to hand out", &[l("start", "where the block starts"), l("size", "its length in bytes")], "0, or a negative error"),
    f(G, 0x15, "Srealloc", "Asks for screen memory of a size (TOS 4)", &[l("size", "the bytes wanted")], "the block's address, or 0"),
    f(G, 0x19, "Dgetdrv", "Which drive is current", &[], "0 for A:, 1 for B: and on"),
    f(G, 0x1A, "Fsetdta", "Sets where Fsfirst and Fsnext put what they find", &[l("dta", "a 44-byte buffer")], NOTHING),
    f(G, 0x20, "Super", "Switches between user and supervisor mode (the hardware registers need supervisor mode)", &[l("stack", "0: go into supervisor mode on the user stack; 1: just ask which mode; otherwise the value an earlier Super returned, to go back to user mode")], "going in: the old supervisor stack, to give back to Super later; asking: -1 in supervisor mode, 0 in user mode"),
    f(G, 0x2A, "Tgetdate", "The date", &[], "the date packed: day in bits 0-4, month 5-8, years since 1980 9-15"),
    f(G, 0x2B, "Tsetdate", "Sets the date", &[w("date", "packed as Tgetdate gives it")], "0, or -1 if it isn't a date"),
    f(G, 0x2C, "Tgettime", "The time", &[], "the time packed: seconds/2 in bits 0-4, minutes 5-10, hours 11-15"),
    f(G, 0x2D, "Tsettime", "Sets the time", &[w("time", "packed as Tgettime gives it")], "0, or -1 if it isn't a time"),
    f(G, 0x2F, "Fgetdta", "Where Fsfirst and Fsnext put what they find", &[], "the buffer's address"),
    f(G, 0x30, "Sversion", "GEMDOS's version", &[], "the version: the major number in the low byte, the minor in the high byte"),
    f(G, 0x31, "Ptermres", "Ends the program but keeps some of its memory, for code that stays behind", &[l("keep", "bytes to keep from the start of the basepage"), w("code", "what to return to whatever ran it")], "doesn't return"),
    f(G, 0x36, "Dfree", "How much room is on a drive", &[l("buffer", "16 bytes for: free clusters, all clusters, bytes a sector, sectors a cluster"), w("drive", "0 the current drive, 1 A:, 2 B: and on")], "0, or a negative error"),
    f(G, 0x39, "Dcreate", "Makes a folder", &[l("path", "its name, ending with a 0 byte")], "0, or a negative error"),
    f(G, 0x3A, "Ddelete", "Removes an empty folder", &[l("path", "its name")], "0, or a negative error"),
    f(G, 0x3B, "Dsetpath", "Changes the current folder", &[l("path", "the folder")], "0, or a negative error"),
    f(G, 0x3C, "Fcreate", "Makes a file (emptying one there already), open to write", &[l("name", "its name, ending with a 0 byte"), w("attributes", "0 for an ordinary file; 1 read-only, 2 hidden, 4 system, 8 a volume label")], "a handle (a word, 6 or more), or a negative error"),
    f(G, 0x3D, "Fopen", "Opens a file that's there", &[l("name", "its name, ending with a 0 byte"), w("mode", "0 to read, 1 to write, 2 both")], "a handle (a word, 6 or more), or a negative error"),
    f(G, 0x3E, "Fclose", "Closes a file", &[HANDLE], "0, or a negative error"),
    f(G, 0x3F, "Fread", "Reads from a file", &[HANDLE, l("count", "the bytes to read"), l("buffer", "where they go")], "the bytes read (fewer at the end of the file), or a negative error"),
    f(G, 0x40, "Fwrite", "Writes to a file", &[HANDLE, l("count", "the bytes to write"), l("buffer", "where they come from")], "the bytes written, or a negative error"),
    f(G, 0x41, "Fdelete", "Deletes a file", &[l("name", "its name")], "0, or a negative error"),
    f(G, 0x42, "Fseek", "Moves where a file is read or written next", &[l("offset", "how far (signed)"), HANDLE, w("from", "0 the start, 1 where it is now, 2 the end")], "the new place from the start, or a negative error"),
    f(G, 0x43, "Fattrib", "Reads or sets a file's attributes", &[l("name", "its name"), w("set", "0 to read them, 1 to set them"), w("attributes", "when setting: 1 read-only, 2 hidden, 4 system, $20 archive")], "the attributes, or a negative error"),
    f(G, 0x44, "Mxalloc", "Asks for memory of a kind (TOS 2 and later)", &[l("size", "the bytes wanted, or -1 to ask how much there is"), w("kind", "0 ST RAM only, 1 TT RAM only, 2 either, ST first, 3 either, TT first")], "the block's address, or 0"),
    f(G, 0x45, "Fdup", "Makes a new handle for a standard one", &[w("handle", "0-5")], "the new handle, or a negative error"),
    f(G, 0x46, "Fforce", "Points a standard handle at another file, to redirect", &[w("standard", "0-5"), w("handle", "the file to point it at")], "0, or a negative error"),
    f(G, 0x47, "Dgetpath", "The current folder on a drive", &[l("buffer", "where its path goes (at least 128 bytes)"), w("drive", "0 the current drive, 1 A: and on")], "0, or a negative error"),
    f(G, 0x48, "Malloc", "Asks for a block of memory", &[l("size", "the bytes wanted, or -1 to ask for the biggest block there is")], "the block's address (0 if there isn't one), or the biggest block's size"),
    f(G, 0x49, "Mfree", "Gives back memory Malloc gave", &[l("block", "its address")], "0, or a negative error"),
    f(G, 0x4A, "Mshrink", "Makes a block of memory smaller (a program does this to its own at the start)", &[w("zero", "always 0"), l("block", "its address (a program's basepage)"), l("size", "the new size")], "0, or a negative error"),
    f(G, 0x4B, "Pexec", "Loads and runs a program, or only loads it", &[w("mode", "0 load and run, 3 load only, 4 run what's loaded, 5 just make a basepage"), l("name", "the program's file name (or the basepage, for 4)"), l("tail", "the command line: a length byte, then the text"), l("environment", "the environment strings, or 0 for the parent's")], "mode 0: the program's return code; 3: its basepage; or a negative error"),
    f(G, 0x4C, "Pterm", "Ends the program, returning a code", &[w("code", "what to return to whatever ran it")], "doesn't return"),
    f(G, 0x4E, "Fsfirst", "Finds the first file matching a name", &[l("pattern", "the name, with * and ? wildcards"), w("attributes", "which kinds to find besides ordinary files: 2 hidden, 4 system, 8 volume label, $10 folders")], "0 with the file described in the DTA, or a negative error"),
    f(G, 0x4F, "Fsnext", "Finds the next file matching Fsfirst's name", &[], "0 with the file in the DTA, or a negative error when there are no more"),
    f(G, 0x56, "Frename", "Renames or moves a file", &[w("zero", "always 0"), l("old", "its name now"), l("new", "its new name")], "0, or a negative error"),
    f(G, 0x57, "Fdatime", "Reads or sets a file's date and time", &[l("buffer", "two words: time, then date, packed as Tgettime and Tgetdate"), HANDLE, w("set", "0 to read, 1 to set")], "0, or a negative error"),
    f(G, 0x5C, "Flock", "Locks part of a file against others", &[HANDLE, w("mode", "0 to lock, 1 to unlock"), l("start", "where the part starts"), l("length", "its length")], "0, or a negative error"),
    // BIOS.
    f(B, 0x00, "Getmpb", "Fills in the memory parameter block (only GEMDOS uses this)", &[l("block", "where it goes")], NOTHING),
    f(B, 0x01, "Bconstat", "Whether a device has a character waiting", &[DEVICE], "-1 if it has, 0 if not"),
    f(B, 0x02, "Bconin", "Waits for a character from a device", &[DEVICE], "the character; from the keyboard, the scan code in bits 16-23 too"),
    f(B, 0x03, "Bconout", "Sends a character to a device", &[DEVICE, w("char", "the character")], "nothing useful"),
    f(B, 0x04, "Rwabs", "Reads or writes disk sectors, not going through files", &[w("mode", "0 read, 1 write (add 2 to ignore a disk change)"), l("buffer", "where they go or come from"), w("count", "how many sectors"), w("sector", "the first one"), w("drive", "0 A:, 1 B: and on")], "0, or a negative error"),
    f(B, 0x05, "Setexc", "Reads or sets an exception vector", &[w("vector", "its number: the address divided by 4 ($46 for $118)"), l("handler", "the new handler's address, or -1 just to read it")], "the old handler's address"),
    f(B, 0x06, "Tickcal", "How long a system timer tick is", &[], "milliseconds a tick (20)"),
    f(B, 0x07, "Getbpb", "The layout of the disk in a drive", &[w("drive", "0 A:, 1 B: and on")], "the address of its BIOS parameter block, or 0"),
    f(B, 0x08, "Bcostat", "Whether a device can take a character", &[DEVICE], "-1 if it can, 0 if not"),
    f(B, 0x09, "Mediach", "Whether the disk in a drive may have been changed", &[w("drive", "0 A:, 1 B: and on")], "0 not changed, 1 may have been, 2 has been"),
    f(B, 0x0A, "Drvmap", "Which drives there are", &[], "a bit for each, bit 0 for A:"),
    f(B, 0x0B, "Kbshift", "Reads or sets the Shift, Control, Alternate and Caps Lock state", &[w("state", "-1 to read; otherwise the new state")], "the state: bit 0 right Shift, 1 left Shift, 2 Control, 3 Alternate, 4 Caps Lock"),
    // XBIOS.
    f(X, 0x00, "Initmous", "Sets how the mouse is reported", &[w("mode", "0 off, 1 relative, 2 absolute, 4 by keys"), l("parameters", "the settings block for the mode"), l("handler", "a routine to call with each mouse packet")], NOTHING),
    f(X, 0x01, "Ssbrk", "Takes memory off the top before the system starts (only for the earliest code)", &[w("size", "the bytes")], "where it starts"),
    f(X, 0x02, "Physbase", "Where the screen being shown is", &[], "its address"),
    f(X, 0x03, "Logbase", "Where the system draws (the screen VDI writes to)", &[], "its address"),
    f(X, 0x04, "Getrez", "The resolution", &[], "0 low, 1 medium, 2 high (TT: 4 TT medium, 6 TT high, 7 TT low)"),
    f(X, 0x05, "Setscreen", "Sets the screens and the resolution; -1 leaves one as it is. The change happens at the next frame", &[l("logical", "where the system draws, or -1"), l("physical", "the screen shown, or -1"), w("resolution", "0 low, 1 medium, 2 high, or -1")], NOTHING),
    f(X, 0x06, "Setpalette", "Sets all 16 colours at the next frame", &[l("palette", "16 words, colour 0 first")], NOTHING),
    f(X, 0x07, "Setcolor", "Reads or sets one colour", &[w("number", "the colour, 0 to 15"), w("colour", "the new value, or -1 just to read it")], "the colour's old value"),
    f(X, 0x08, "Floprd", "Reads sectors from a floppy", &[l("buffer", "where they go"), l("unused", "0"), w("drive", "0 A:, 1 B:"), w("sector", "the first, from 1"), w("track", "the track"), w("side", "0 or 1"), w("count", "how many")], "0, or a negative error"),
    f(X, 0x09, "Flopwr", "Writes sectors to a floppy", &[l("buffer", "where they come from"), l("unused", "0"), w("drive", "0 A:, 1 B:"), w("sector", "the first, from 1"), w("track", "the track"), w("side", "0 or 1"), w("count", "how many")], "0, or a negative error"),
    f(X, 0x0A, "Flopfmt", "Formats a track of a floppy", &[l("buffer", "at least 8K of work space"), l("skew", "0, or a table of sector numbers"), w("drive", "0 A:, 1 B:"), w("sectors", "sectors on the track (9 usually)"), w("track", "the track"), w("side", "0 or 1"), w("interleave", "1 usually"), l("magic", "$87654321"), w("fill", "the value to fill sectors with ($E5E5)")], "0, or a negative error"),
    f(X, 0x0C, "Midiws", "Sends bytes out of the MIDI port", &[w("count", "how many, less one"), l("bytes", "where they are")], NOTHING),
    f(X, 0x0D, "Mfpint", "Sets the handler of one of the MFP's interrupts", &[w("channel", "0 to 15"), l("handler", "its address")], NOTHING),
    f(X, 0x0E, "Iorec", "Where a device's input buffer is", &[w("device", "0 serial, 1 keyboard, 2 MIDI")], "the address of the buffer's record"),
    f(X, 0x0F, "Rsconf", "Sets up the serial port; -1 leaves a setting as it is", &[w("speed", "0 19200 baud, 1 9600, 4 2400 ... 15 50"), w("flow", "0 none, 1 XON/XOFF, 2 RTS/CTS"), w("ucr", "the MFP's UCR"), w("rsr", "the MFP's RSR"), w("tsr", "the MFP's TSR"), w("scr", "the MFP's SCR")], "the old UCR, RSR, TSR and SCR in one long"),
    f(X, 0x10, "Keytbl", "Sets the keyboard's translation tables; -1 leaves one as it is", &[l("normal", "the table without Shift"), l("shifted", "the table with Shift"), l("caps", "the table with Caps Lock")], "the address of the tables' record"),
    f(X, 0x11, "Random", "A random number", &[], "24 random bits"),
    f(X, 0x12, "Protobt", "Makes a boot sector in memory", &[l("buffer", "512 bytes"), l("serial", "a serial number, -1 to keep, or $1000000 or more for a random one"), w("type", "the disk: 0 single-sided 360K, 1 double-sided 720K ... or -1 to keep"), w("executable", "1 to make it run, 0 not, -1 to keep")], NOTHING),
    f(X, 0x13, "Flopver", "Checks sectors on a floppy can be read", &[l("buffer", "at least 1K of work space"), l("unused", "0"), w("drive", "0 A:, 1 B:"), w("sector", "the first"), w("track", "the track"), w("side", "0 or 1"), w("count", "how many")], "0, or a negative error with the bad sectors listed in the buffer"),
    f(X, 0x14, "Scrdmp", "Prints the screen", &[], NOTHING),
    f(X, 0x15, "Cursconf", "Sets how the text cursor looks", &[w("function", "0 hide, 1 show, 2 blink, 3 steady, 4 set the blink rate, 5 read it"), w("rate", "the rate, in frames, for 4")], "the blink rate, for 5"),
    f(X, 0x16, "Settime", "Sets the keyboard processor's clock", &[l("time", "the date and time packed: Tgettime's word in the low half, Tgetdate's in the high")], NOTHING),
    f(X, 0x17, "Gettime", "Reads the keyboard processor's clock", &[], "the date and time, packed as Settime takes them"),
    f(X, 0x18, "Bioskeys", "Puts back the keyboard tables TOS started with", &[], NOTHING),
    f(X, 0x19, "Ikbdws", "Sends bytes to the keyboard processor", &[w("count", "how many, less one"), l("bytes", "where they are")], NOTHING),
    f(X, 0x1A, "Jdisint", "Turns one of the MFP's interrupts off", &[w("channel", "0 to 15")], NOTHING),
    f(X, 0x1B, "Jenabint", "Turns one of the MFP's interrupts on", &[w("channel", "0 to 15")], NOTHING),
    f(X, 0x1C, "Giaccess", "Reads or writes a YM2149 register", &[w("data", "the value to write"), w("register", "the register; add $80 to write")], "the register's value"),
    f(X, 0x1D, "Offgibit", "Clears bits of the YM's port A", &[w("mask", "AND mask: 0 bits are cleared")], NOTHING),
    f(X, 0x1E, "Ongibit", "Sets bits of the YM's port A", &[w("mask", "OR mask: 1 bits are set")], NOTHING),
    f(X, 0x1F, "Xbtimer", "Starts one of the MFP's timers with a handler", &[w("timer", "0 A, 1 B, 2 C, 3 D"), w("control", "the timer's control value: 1-7 a divider, 8 event count"), w("data", "the count"), l("handler", "the interrupt routine (it ends with RTE, clearing its in-service bit)")], NOTHING),
    f(X, 0x20, "Dosound", "Plays a sound list on the YM, a step every 50th of a second", &[l("list", "the list's address, or -1 to ask where it's up to")], "where the list is up to, for -1"),
    f(X, 0x21, "Setprt", "Reads or sets the printer settings", &[w("settings", "-1 to read; otherwise the new settings")], "the old settings"),
    f(X, 0x22, "Kbdvbase", "Where the keyboard processor's handler vectors are", &[], "the address of the vectors: MIDI, keyboard errors, status, mouse, clock, joystick"),
    f(X, 0x23, "Kbrate", "Reads or sets the key repeat; -1 leaves one as it is", &[w("delay", "frames before repeating starts"), w("rate", "frames between repeats")], "the old delay in the high byte, the old rate in the low"),
    f(X, 0x24, "Prtblk", "Prints a block of the screen", &[l("block", "the block's settings")], NOTHING),
    f(X, 0x25, "Vsync", "Waits for the next vertical blank", &[], NOTHING),
    f(X, 0x26, "Supexec", "Runs a routine in supervisor mode", &[l("routine", "its address; it ends with RTS")], "what the routine left in d0"),
    f(X, 0x27, "Puntaes", "Throws the AES out of memory (when it's in RAM) and restarts", &[], "doesn't return if it worked"),
    f(X, 0x29, "Floprate", "Reads or sets a floppy drive's step rate", &[w("drive", "0 A:, 1 B:"), w("rate", "0 6 ms, 1 12 ms, 2 2 ms, 3 3 ms, or -1 to read")], "the old rate"),
    f(X, 0x2A, "DMAread", "Reads sectors from a hard disk (TOS 2 and later)", &[l("sector", "the first"), w("count", "how many"), l("buffer", "where they go"), w("device", "0-7 ACSI, 8-15 SCSI, 16-17 IDE")], "0, or a negative error"),
    f(X, 0x2B, "DMAwrite", "Writes sectors to a hard disk (TOS 2 and later)", &[l("sector", "the first"), w("count", "how many"), l("buffer", "where they come from"), w("device", "0-7 ACSI, 8-15 SCSI, 16-17 IDE")], "0, or a negative error"),
    f(X, 0x2C, "Bconmap", "Chooses which serial port the BIOS's device 1 is (Mega STE, TT, Falcon)", &[w("device", "6 and up for a port; -1 to ask which; -2 for the table")], "the old device"),
    f(X, 0x2E, "NVMaccess", "Reads or writes the battery-backed memory (TT, Falcon)", &[w("operation", "0 read, 1 write, 2 clear"), w("start", "the first byte"), w("count", "how many"), l("buffer", "where they go or come from")], "0, or a negative error"),
    f(X, 0x40, "Blitmode", "Reads or sets whether the system uses the blitter", &[w("mode", "-1 to read; bit 0 set to use the blitter")], "the old mode: bit 0 in use, bit 1 there is one"),
    f(X, 0x50, "EsetShift", "Sets the TT's video mode", &[w("mode", "the TT shifter's value")], "the old mode"),
    f(X, 0x51, "EgetShift", "The TT's video mode", &[], "the TT shifter's value"),
    f(X, 0x52, "EsetBank", "Chooses which 16 of the TT's 256 colours the ST modes use", &[w("bank", "0 to 15, or -1 to read")], "the old bank"),
    f(X, 0x53, "EsetColor", "Reads or sets one of the TT's colours", &[w("number", "0 to 255"), w("colour", "the value, or -1 to read")], "the old value"),
    f(X, 0x54, "EsetPalette", "Sets a run of the TT's colours", &[w("first", "the first colour"), w("count", "how many"), l("colours", "the words")], NOTHING),
    f(X, 0x55, "EgetPalette", "Reads a run of the TT's colours", &[w("first", "the first colour"), w("count", "how many"), l("colours", "where the words go")], NOTHING),
    f(X, 0x56, "EsetGray", "Turns the TT's grey mode on or off", &[w("mode", "0 colour, 1 grey, -1 to read")], "the old mode"),
    f(X, 0x57, "EsetSmear", "Turns the TT's sample-and-hold mode on or off", &[w("mode", "0 off, 1 on, -1 to read")], "the old mode"),
    f(X, 0x58, "VsetMode", "Sets the Falcon's video mode", &[w("mode", "the mode bits: colours, columns, VGA, PAL, overscan, interlace; -1 to read")], "the old mode"),
    f(X, 0x59, "VgetMonitor", "Which monitor the Falcon has", &[], "0 mono, 1 RGB, 2 VGA, 3 TV"),
    f(X, 0x5A, "VsetSync", "Sets the Falcon's external sync", &[w("flags", "bit 0 external clock, 1 vertical sync, 2 horizontal sync")], NOTHING),
    f(X, 0x5B, "VgetSize", "How much memory a Falcon video mode needs", &[w("mode", "the mode bits")], "the bytes"),
    f(X, 0x5D, "VsetRGB", "Sets a run of the Falcon's colours", &[w("first", "the first colour"), w("count", "how many"), l("colours", "longs of $00RRGGBB")], NOTHING),
    f(X, 0x5E, "VgetRGB", "Reads a run of the Falcon's colours", &[w("first", "the first colour"), w("count", "how many"), l("colours", "where the longs go")], NOTHING),
    f(X, 0x80, "Locksnd", "Claims the Falcon's sound system", &[], "1, or a negative error if it's taken"),
    f(X, 0x81, "Unlocksnd", "Gives the Falcon's sound system back", &[], "0, or a negative error"),
    f(X, 0x82, "Soundcmd", "Sets the Falcon's sound mixer and converters", &[w("command", "what to set: 0-1 attenuation, 2-3 gain, 4 adder, 5 input, 6 sample format, 7 prescaler"), w("value", "the setting, or -1 to read")], "the setting"),
    f(X, 0x83, "Setbuffer", "Sets the Falcon's play or record buffer", &[w("which", "0 play, 1 record"), l("start", "where it starts"), l("end", "where it ends")], "0, or a negative error"),
    f(X, 0x84, "Setmode", "Sets the Falcon's sample format", &[w("mode", "0 8-bit stereo, 1 16-bit stereo, 2 8-bit mono")], "0, or a negative error"),
    f(X, 0x88, "Buffoper", "Starts or stops the Falcon's playing and recording", &[w("mode", "bit 0 play, 1 play looping, 2 record, 3 record looping; -1 to read")], "the old mode"),
    f(X, 0x8B, "Devconnect", "Connects a source to destinations in the Falcon's sound matrix", &[w("source", "0 DMA play, 1 DSP, 2 external, 3 ADC"), w("destinations", "bits: 1 DMA record, 2 DSP, 4 external, 8 DAC"), w("clock", "0 25 MHz, 1 external, 2 32 MHz"), w("prescale", "the divider: 1-11"), w("protocol", "0 handshake, 1 none")], "0, or a negative error"),
    f(X, 0x8C, "Sndstatus", "The Falcon's sound status, or resets it", &[w("reset", "1 to reset, 0 to read")], "the status"),
    f(X, 0x8D, "Buffptr", "Where the Falcon's playing and recording are up to", &[l("buffer", "16 bytes for the play, record and two reserved pointers")], "0"),
];

/// A function by trap and number.
pub fn function(trap: Trap, number: i64) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.trap == trap && f.number as i64 == number)
}

/// A function by name, in any case.
pub fn named(name: &str) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// The Line-A routines, $A000 to $A00F.
pub const LINE_A: [(&str, &str); 16] = [
    ("Initialise", "Gives the Line-A variables' address in a0 and d0, the system fonts' in a1 and the routines' in a2; call it first"),
    ("Put pixel", "Plots a pixel: INTIN[0] the colour, PTSIN[0] and [1] x and y"),
    ("Get pixel", "Reads a pixel's colour at PTSIN[0] and [1], into d0"),
    ("Arbitrary line", "Draws a line from (X1,Y1) to (X2,Y2) with the line mask and colour in the variables"),
    ("Horizontal line", "Draws a horizontal line from X1 to X2 at Y1, with the fill pattern"),
    ("Filled rectangle", "Fills the rectangle (X1,Y1)-(X2,Y2) with the fill pattern, clipped if CLIP is set"),
    ("Filled polygon", "Fills one line (Y1) of a polygon whose points are in PTSIN"),
    ("Bit block transfer", "Copies a rectangle between screens or memory, as the blit block in a6 says"),
    ("Text block transfer", "Draws a character of a font, as the variables say"),
    ("Show mouse", "Shows the mouse pointer (INTIN[0] 0 to show it whatever the hide count)"),
    ("Hide mouse", "Hides the mouse pointer"),
    ("Transform mouse", "Changes the mouse pointer's shape, from the block INTIN points at"),
    ("Undraw sprite", "Takes a sprite off the screen, putting back what was under it (a2 the save buffer)"),
    ("Draw sprite", "Draws a 16x16 sprite at d0,d1 (a0 its definition, a2 a save buffer)"),
    ("Copy raster form", "Copies a raster area between forms, as the VDI's vro_cpyfm"),
    ("Seed fill", "Flood-fills from a point, as the VDI's v_contourfill"),
];

/// What a Line-A opcode does, said.
pub fn line_a(opcode: u16) -> Option<String> {
    if opcode & 0xFFF0 != 0xA000 {
        return None;
    }
    let (name, about) = LINE_A[(opcode & 15) as usize];
    Some(crate::i18n::tf!("Line-A ${opcode:04X}: {name}\n{about}\nIt's an instruction the 68000 doesn't have, so it goes to TOS's handler at $28. a0-a2 and d0-d2 aren't kept"))
}

/// What trap #2 is, by the value in d0.
pub fn trap2(d0: Option<i64>) -> String {
    match d0 {
        Some(200) => crate::i18n::tr("AES call (trap #2 with d0 = 200, $C8): d1 holds the address of the AES parameter block (control, global, int_in, int_out, addr_in, addr_out); the function's number is control[0]").into(),
        Some(115) => crate::i18n::tr("VDI call (trap #2 with d0 = 115, $73): d1 holds the address of the VDI parameter block (contrl, intin, ptsin, intout, ptsout); the function's number is contrl[0]").into(),
        Some(-2) => crate::i18n::tr("GDOS check (trap #2 with d0 = -2): d0 comes back -2 if GDOS isn't loaded").into(),
        Some(0) => crate::i18n::tr("Pterm0 (trap #2 with d0 = 0): ends the program, as GEMDOS's Pterm0 does").into(),
        _ => crate::i18n::tr("trap #2: the AES (d0 = 200) or the VDI (d0 = 115), with d1 the address of the parameter block").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Names by number, as Hatari's tables (gemdos.c, bios.c, xbios.c) list them.
    #[test]
    fn numbers_and_names_are_the_system_s() {
        let gemdos = [(0, "Pterm0"), (1, "Cconin"), (9, "Cconws"), (0x0B, "Cconis"), (0x19, "Dgetdrv"), (0x20, "Super"), (0x30, "Sversion"), (0x31, "Ptermres"), (0x3C, "Fcreate"), (0x3D, "Fopen"), (0x3E, "Fclose"), (0x3F, "Fread"), (0x40, "Fwrite"), (0x42, "Fseek"), (0x44, "Mxalloc"), (0x48, "Malloc"), (0x49, "Mfree"), (0x4A, "Mshrink"), (0x4B, "Pexec"), (0x4C, "Pterm"), (0x4E, "Fsfirst"), (0x4F, "Fsnext"), (0x56, "Frename"), (0x57, "Fdatime"), (0x5C, "Flock")];
        for (n, name) in gemdos {
            assert_eq!(function(Trap::Gemdos, n).map(|f| f.name), Some(name), "GEMDOS ${n:02X}");
        }
        let bios = ["Getmpb", "Bconstat", "Bconin", "Bconout", "Rwabs", "Setexc", "Tickcal", "Getbpb", "Bcostat", "Mediach", "Drvmap", "Kbshift"];
        for (n, name) in bios.iter().enumerate() {
            assert_eq!(function(Trap::Bios, n as i64).map(|f| f.name), Some(*name));
        }
        let xbios = ["Initmous", "Ssbrk", "Physbase", "Logbase", "Getrez", "Setscreen", "Setpalette", "Setcolor", "Floprd", "Flopwr", "Flopfmt", "", "Midiws", "Mfpint", "Iorec", "Rsconf", "Keytbl", "Random", "Protobt", "Flopver", "Scrdmp", "Cursconf", "Settime", "Gettime", "Bioskeys", "Ikbdws", "Jdisint", "Jenabint", "Giaccess", "Offgibit", "Ongibit", "Xbtimer", "Dosound", "Setprt", "Kbdvbase", "Kbrate", "Prtblk", "Vsync", "Supexec", "Puntaes"];
        for (n, name) in xbios.iter().enumerate().filter(|(_, n)| !n.is_empty()) {
            assert_eq!(function(Trap::Xbios, n as i64).map(|f| f.name), Some(*name), "XBIOS {n}");
        }
        for (n, name) in [(0x40, "Blitmode"), (80, "EsetShift"), (88, "VsetMode"), (89, "VgetMonitor"), (93, "VsetRGB"), (128, "Locksnd"), (139, "Devconnect")] {
            assert_eq!(function(Trap::Xbios, n).map(|f| f.name), Some(name), "XBIOS {n}");
        }
        // No number twice.
        for (i, a) in FUNCTIONS.iter().enumerate() {
            assert!(!FUNCTIONS[i + 1..].iter().any(|b| b.trap == a.trap && b.number == a.number), "{}", a.name);
        }
    }

    /// Argument sizes, as FreeMiNT's osbind.h binds them (w for word, l for long, after the function's number).
    #[test]
    fn arguments_are_the_sizes_the_bindings_push() {
        let bindings = [
            ("Cconws", "l"), ("Super", "l"), ("Fopen", "lw"), ("Fclose", "w"), ("Fread", "wll"), ("Fwrite", "wll"), ("Fseek", "lww"), ("Malloc", "l"), ("Mshrink", "wll"), ("Pexec", "wlll"), ("Pterm", "w"),
            ("Fsfirst", "lw"), ("Frename", "wll"), ("Fdatime", "lww"), ("Dfree", "lw"), ("Mxalloc", "lw"), ("Bconout", "ww"), ("Rwabs", "wlwww"), ("Setexc", "wl"), ("Kbshift", "w"), ("Initmous", "wll"),
            ("Setscreen", "llw"), ("Setpalette", "l"), ("Setcolor", "ww"), ("Floprd", "llwwwww"), ("Flopfmt", "llwwwwwlw"), ("Midiws", "wl"), ("Mfpint", "wl"), ("Rsconf", "wwwwww"), ("Keytbl", "lll"),
            ("Protobt", "llww"), ("Cursconf", "ww"), ("Ikbdws", "wl"), ("Giaccess", "ww"), ("Xbtimer", "wwwl"), ("Dosound", "l"), ("Kbrate", "ww"), ("Supexec", "l"), ("Floprate", "ww"), ("DMAread", "lwlw"),
            ("NVMaccess", "wwwl"), ("EsetPalette", "wwl"), ("Flock", "wwll"),
        ];
        for (name, sizes) in bindings {
            let found = named(name).unwrap_or_else(|| panic!("{name}"));
            let ours: String = found.args.iter().map(|a| if a.long { 'l' } else { 'w' }).collect();
            assert_eq!(ours, sizes, "{name}");
        }
        assert_eq!(named("fread").unwrap().stack_bytes(), 12);
        let said = named("Fopen").unwrap().describe();
        assert!(said.contains("  2(sp)  long  name") && said.contains("  6(sp)  word  mode") && said.contains("addq.l #8,sp"), "{said}");
    }

    #[test]
    fn line_a_and_trap_2_are_said() {
        assert!(line_a(0xA00A).unwrap().contains("Hide mouse"));
        assert!(line_a(0xA010).is_none() && line_a(0x4E75).is_none());
        assert!(trap2(Some(200)).starts_with("AES"));
        assert!(trap2(Some(0x73)).starts_with("VDI"));
    }
}
