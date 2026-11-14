# OpticOS, in Limoniter

OpticOS is a clean-room replacement 16K ROM for the 48K ZX Spectrum, with its own BASIC (OpticBASIC), by
Optica Technology Design: https://github.com/brixham99/OpticOS

- `opticos.rom` is the ROM from the OpticOS v1.0.2 release (SHA-256
  8775e085c78c3ef5e409cae0237c7e31f6a4e67fa11ab6d04e41fdca2ed4ba29), unchanged. With the setting on, Limoniter
  downloads newer releases' ROMs by itself (src/opticos_rom.rs); this one is used until then. It's code, under the MIT
  licence in `LICENSE`. Limoniter's built-in Spectrum runs it, and hands it to Fuse and ZEsarUX, for running
  OpticBASIC programs.
- `examples/` holds the OpticOS example programs (`*.bas`, and `happybat.asm`, the machine code `happybat`
  loads) and the tape the ROM itself saved them to (`opticos_examples.tap`), from the same release (repository
  commit 246a54b). Limoniter's tests check that it
  tokenises and lists OpticBASIC exactly as the ROM does against them. The example programs are documentation,
  under Creative Commons Attribution 4.0 (CC BY 4.0), (c) 2026 Optica Technology Design.
