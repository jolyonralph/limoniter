# 14. Further reading

[‹ Releasing](13-releasing.md) · [Contents](README.md)

Everything this guide says rests on the documents below; they say much more. Every link was fetched when the guide was written.

## The Spectrum

- **World of Spectrum's technical FAQ**, the reference for timing and ports: the 48K (<https://worldofspectrum.org/faq/reference/48kreference.htm>), the 128K, +2, +2A and +3 (<https://worldofspectrum.org/faq/reference/128kreference.htm>), the Z80 as the Spectrum uses it, interrupts included (<https://worldofspectrum.org/faq/reference/z80reference.htm>), ports (<https://worldofspectrum.org/faq/reference/ports.htm>), and file formats (<https://worldofspectrum.org/faq/reference/formats.htm>, <https://worldofspectrum.org/faq/reference/z80format.htm>).
- **The ROM**, disassembled: Ian Logan and Frank O'Hara's *The Complete Spectrum ROM Disassembly*, annotated and linked by SkoolKit (<https://skoolkid.github.io/rom/>). Every routine and system variable, with what calls what.
- **Sinclair's 48K BASIC manual** (<https://worldofspectrum.org/ZXBasicManual/zxmanchap24.html> is its chapter on memory) and **Amstrad's +3 manual**, whose chapter 8 is a technical reference for the 128K family (<https://worldofspectrum.org/ZXSpectrum128+3Manual/chapter8pt24.html> for paging, <https://worldofspectrum.org/ZXSpectrum128+3Manual/chapter8pt30.html> for the AY).
- **Fuse's timings**, machine by machine, in libspectrum's `timings.c` (<https://sourceforge.net/p/fuse-emulator/libspectrum/ci/master/tree/timings.c>).
- **The floating bus**: Ramsoft's *Floating bus technical guide* (<https://k1.spdns.de/Develop/Projects/zxsp/Info/Floating%20bus%20technical%20guide.html>), and the +2A and +3's, found later (<https://github.com/redcode/ZXSpectrum/wiki/2A-3-Floating-Bus-Test>).
- **The TZX format** (<https://worldofspectrum.net/TZXformat.html>): every block type, and the ROM loader's timings.

## The Z80

- Zilog's ***Z80 CPU User Manual***: every instruction, its flags and its T-states (Zilog's document UM0080, from Zilog's site).
- Rodnay Zaks, ***Programming the Z80*** (Sybex): the classic book, still the best introduction to the processor.

## The Next

- The Next's **register and port lists**, from its FPGA core: <https://gitlab.com/SpectrumNext/ZX_Spectrum_Next_FPGA/-/raw/master/cores/zxnext/nextreg.txt> and <https://gitlab.com/SpectrumNext/ZX_Spectrum_Next_FPGA/-/raw/master/cores/zxnext/ports.txt>.
- **The Next's wiki** (<https://wiki.specnext.dev>): the Z80N's instructions (<https://wiki.specnext.dev/Extended_Z80_instruction_set>), Layer 2 (<https://wiki.specnext.dev/Layer_2>), sprites (<https://wiki.specnext.dev/Sprites>), the copper (<https://wiki.specnext.dev/Copper>), the tilemap (<https://wiki.specnext.dev/Tilemap>), DMA (<https://wiki.specnext.dev/DMA>) and the `.nex` format (<https://wiki.specnext.dev/NEX_file_format>).

## Tools

- **Fuse**, the Free Unix Spectrum Emulator: <https://fuse-emulator.sourceforge.net/>.
- **ZEsarUX**, which emulates the Next too: <https://github.com/chernandezba/zesarux>.
- **CSpect**, Mike Dailly's Next emulator: <https://mdf200.itch.io/cspect>.
- **sjasmplus**, the assembler whose syntax the examples use: <https://github.com/z00m128/sjasmplus>.

## The scene

- **Spectrum Computing** (<https://spectrumcomputing.co.uk/>): the archive and database of Spectrum software, and its forums, where people who know these machines best answer questions.
- **Pouët** (<https://www.pouet.net/>) and **Demozoo** (<https://demozoo.org/>): the demos, to watch and learn from.

[‹ Releasing](13-releasing.md) · [Contents](README.md)
