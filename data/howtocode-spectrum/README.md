# How To Code: the ZX Spectrum

*How to write Spectrum demos and games that work: the 48K, the 128K and its family, and the Next.*

By Comrade J and Mnemonic of Share and Enjoy, made as How To Code v8 was for the Amiga, and released under **CC0**: use any of it, for anything.

> Fourteen chapters, and nineteen example programs with the startup they're built on. Every example assembles with Limoniter's own assembler, and the Spectrum ones run on Limoniter's built-in 48K and 128K, in Fuse and in ZEsarUX before they go in; the Next ones run in ZEsarUX's Next. Chapter 1 says what's been tested where, and what hasn't. Click a chapter to read it.

## Targets

The examples are written for the **48K Spectrum** first: a Z80 at 3.5 MHz, 16K of ROM, 48K of RAM, and the ULA. Every 48K example runs on the 128K, +2, +2A and +3 too, and finds out which it's on. Chapters about the 128K's extra memory, its shadow screen and its AY sound chip say so, as do the Next's.

## Chapters

| | Chapter | What's in it |
|---|---|---|
| 1 | [**Introduction**](01-introduction.md): what this guide is, who it's for, how it was checked, CC0 | |
| 2 | [**Working with Limoniter**](02-working-with-limoniter.md): F9 to the built-in Spectrum or Fuse and ZEsarUX, the debugger and the T-state profiler, tapes, snapshots and .nex files | |
| 3 | [**The machine**](03-the-machine.md): the memory map, the ULA, T-states, contended memory, the floating bus | 2 examples |
| 4 | [**Startup and interrupts**](04-startup-and-interrupts.md): the startup every example uses, IM 1 and IM 2 with a vector table, giving the machine back to BASIC | startup |
| 5 | [**The screen**](05-the-screen.md): its odd layout, attributes, plotting with tables, scrolling, the 128K's shadow screen, border bars and multicolour to the T-state | 5 examples |
| 6 | [**Sprites**](06-sprites.md): masked and pre-shifted, XOR, saving the background, drawing with the stack | 1 example |
| 7 | [**Sound**](07-sound.md): the beeper and a two-voice 1-bit engine, the 128K's AY and a small player | 2 examples |
| 8 | [**The 128K and later models**](08-the-128k-and-later-models.md): paging, banks, contention by bank, the +2A and +3, the Pentagon | 1 example |
| 9 | [**Input**](09-input.md): the keyboard's half-rows, Kempston and Sinclair joysticks | 1 example |
| 10 | [**Speed**](10-speed.md): counting T-states, unrolling, self-modifying code, the stack | 1 example |
| 11 | [**The Next**](11-the-next.md): nextreg, the Z80N's new instructions, Layer 2, sprites, the copper, the tilemap, DMA, memory and .nex files | 3 examples |
| 12 | [**Games and demos**](12-games-and-demos.md): the frame loop, states, controls, random numbers, and a small whole game | 1 example |
| 13 | [**Releasing**](13-releasing.md): TAP, TZX, loaders and loading screens, turbo loaders, snapshots, the Next | 1 example |
| 14 | [**Further reading**](14-further-reading.md) | |

## Examples

In `examples/`. Each is a whole program in sjasmplus syntax that assembles with Limoniter's own assembler and runs with F9. All but the Next's are built on one small startup, [`startup.asm`](examples/startup.asm), which takes the machine from BASIC and gives it back; [`startup_demo.asm`](examples/startup_demo.asm) is the smallest program on it. **Tools › ZX Spectrum and Next › Copy How To Code's Spectrum examples to a folder…** copies them all to a folder of your own, and **File › New as › ZX Spectrum demo (Z80, How To Code)** starts a new file with the startup already in it.

[WRITING.md](WRITING.md) says how the guide is written and its examples tested.
