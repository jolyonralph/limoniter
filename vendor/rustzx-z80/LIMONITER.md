rustzx-z80 0.16.0 from crates.io (https://github.com/rustzx/rustzx), MIT licence (LICENSE.md), without its tests.

One change: `Regs::get_h_alt` and `Regs::get_l_alt` return H' and L'. In 0.16.0 they returned H and L, so every
.sna rustzx-core saved had the wrong H'L', and a program that returned to BASIC from such a snapshot (in Fuse or
ZEsarUX) found the ROM's calculator lost; the debugger showed the wrong HL' too.
