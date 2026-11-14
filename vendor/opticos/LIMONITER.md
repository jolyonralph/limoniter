# OpticOS, in Limoniter

OpticOS is a clean-room replacement 16K ROM for the 48K ZX Spectrum, with its own BASIC (OpticBASIC), by
Optica Technology Design: https://github.com/brixham99/OpticOS

- `opticos.rom` is the ROM from the OpticOS v1.0.1 release (SHA-256
  06961e1fdc4b0146b006d847c95da0d3b39fb491b5659ee09c0c85affb47a0c6), unchanged. It's code, under the MIT
  licence in `LICENSE`. Limoniter's built-in Spectrum runs it, and hands it to Fuse and ZEsarUX, for running
  OpticBASIC programs.
- `examples/` holds the OpticOS example programs (`*.bas`) and the tape the ROM itself saved them to
  (`opticos_examples.tap`), from the same release (repository commit 9a59e5f). Limoniter's tests check that it
  tokenises and lists OpticBASIC exactly as the ROM does against them. The example programs are documentation,
  under Creative Commons Attribution 4.0 (CC BY 4.0), (c) 2026 Optica Technology Design.
