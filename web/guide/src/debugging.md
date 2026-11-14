description: Debug Z80, 6502, 6809 and 68000 programs and Spectrum BASIC with breakpoints, stepping, registers, watches, memory and the profiler, on the built-in Spectrum or in an emulator.

# Debugging

When a program doesn't do what you meant, you can stop it at a line of your source, look at the registers and memory, and go on an instruction at a time. This page shows you how, on the built-in Spectrum first, and then in the emulators that work the same way.

## Where you can debug

| Source | Where it's debugged |
|---|---|
| Z80 | The built-in Spectrum, ZEsarUX (Spectrum and Next programs), or a Spectrum Next |
| 6502 | VICE |
| 6809 | XRoar |
| 68000 | Hatari |
| Spectrum BASIC | The built-in Spectrum |

Debugging works with Limoniter's own assemblers and with the ones you've installed (sjasmplus programs can be debugged too), because either way each address is mapped back to its line.

## Stopping at a line

1. Open your source, and click beside a line's number. A breakpoint mark appears there. Click it again to take it away.
2. Press [[F9]] to run the program, on the built-in Spectrum or the emulator you've chosen (see [Assembling and running](assembling-and-running.html)).
3. When the program gets to that line, it stops. The line is marked, and the registers and flags are shown in a panel beside the source.

Then:

- **Step** runs one instruction, into a CALL.
- **Step over** runs the instruction, or the whole of a subroutine it calls (a CALL, RST, DJNZ or LDIR on the Z80), and stops after it.
- **Carry on** runs on until the next breakpoint.
- **Pause** stops the program wherever it is, with the same view.

## Looking at the program while it's stopped

The panel beside the source has more, on the built-in Spectrum, in ZEsarUX and in VICE:

- **Call stack**: the calls the program is inside, read from the return addresses on the stack, each with its label and line.
- **Watches**: type an expression and click **Watch**, and it's worked out at every stop. Use registers, flags (ZF, CF…), labels, `[addr]` for a byte and `word[addr]` for a word, with arithmetic and comparisons. Click **×** to stop watching one.
- **Memory**: sixteen bytes a row at any address, label or expression typed in **At** (HL to begin with). What's changed since the last stop is picked out.

## Stopping only when something's true

To make a breakpoint stop only sometimes:

1. Open **Breakpoints** in the debugger's panel. Each breakpoint is listed with its line.
2. Beside **only when**, type a condition, such as `A=0 && [count]>3`.

The program now passes straight on whenever the condition is false. Conditions work on the built-in Spectrum and in ZEsarUX.

## Stopping when memory is written

To find what's writing over something it shouldn't:

1. Under **Stop when written to**, type an address or a label, or a range such as `buffer..buffer+31`.
2. Click **Add**.

The program stops after the instruction that wrote there, saying what was written where. XRoar and Hatari can stop on writes too.

## Finding where the time goes

On the built-in Spectrum, **Profile** counts the T-states spent at every address while the program runs, contention and interrupts included.

1. Start the program, and click **Profile**.
2. Let it run through the part you're interested in.
3. Click **Profile** again to stop counting.

The figures stay to read: the time spent in each routine (each label and what follows it), with its share and how often it was entered, and the busiest source lines. Click either to go there.

## Debugging Spectrum BASIC

You can debug a Spectrum BASIC program, 48K or 128K, on the built-in Spectrum in the same way:

1. Open the listing, and click beside a line's number for a breakpoint.
2. Press [[F9]]. The program stops as that line starts, with the line marked.
3. Use **Next statement** to run one statement at a time, or **Carry on** to run to the next breakpoint. **Stop at the next statement** pauses a running program where it is.

While it's stopped, its variables are listed: numbers, strings, arrays, and FOR loops with their limit and step. A program loaded from a tape stops before its first line runs, so a breakpoint on the first line works too.

## In emulators

- **ZEsarUX**: Z80 source runs and is debugged there through its remote protocol, with everything above. It runs Next programs, which the built-in Spectrum can't. A Spectrum program is loaded from its tape in Limoniter and handed over, so ZEsarUX can stop even at its first instruction.
- **VICE**: 6502 source is debugged through VICE's binary monitor, with breakpoints, A, X, Y, SP and the flags, Step, Step over (a JSR) and Carry on, and the call stack, watches and memory.
- **XRoar**: 6809 source runs as a Color Computer program, with breakpoints, the registers (A, B, D, X, Y, U, S, DP and CC), Step, Step over (a BSR, LBSR or JSR), Carry on, Pause, the memory window, and stopping when an address is written. XRoar needs the machine's BASIC ROMs, in its ROM folder or named in the device's options.
- **Hatari**: 68000 source runs as an Atari ST program, and stops at breakpoints wherever TOS loaded it, with the 68000's registers, Step, Step over and Carry on, and stopping when a watched byte changes.

## On a real Spectrum Next

A device of the kind **ZX Spectrum Next (debug cable)** runs Z80 source on a Next itself, through DeZog's dezogif, over a USB serial cable or Wi-Fi, with breakpoints, stepping, the registers, call stack, watches and memory window.

> This is new: it's been tested against a stand-in for the Next, but not yet on a real one. Reports are very welcome.
