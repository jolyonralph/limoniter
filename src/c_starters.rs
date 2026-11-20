//! C programs to start from, one for each machine and compiler File, New offers: each says which machine it's
//! for on its first line, so F9 knows what to compile it with and where to run it.

/// The Commodore 64, with cc65 or llvm-mos. Both turn the text's small letters into the 64's capitals.
pub const C64: &str = "// machine: c64
// A C program for the Commodore 64: F9 compiles it with cc65 or llvm-mos and runs it.
#include <stdio.h>

#define BORDER (*(volatile unsigned char *)0xD020)
#define BACKGROUND (*(volatile unsigned char *)0xD021)

int main(void)
{
    BORDER = 2; // a red border
    BACKGROUND = 0;
    printf(\"hello from limoniter\\n\");
    return 0;
}
";

/// The Spectrum, with z88dk's classic library: printf prints on the screen.
pub const SPECTRUM: &str = "// machine: spectrum
// A C program for the ZX Spectrum: F9 compiles it with z88dk and runs it.
#include <stdio.h>
#include <spectrum.h>

int main(void)
{
    zx_border(2); // a red border
    printf(\"Hello from Limoniter\\n\");
    return 0;
}
";

/// The Spectrum, with SDCC alone: no library to print with, so it colours the screen itself.
pub const SPECTRUM_SDCC: &str = "// machine: spectrum
// compiler: sdcc
// A C program for the ZX Spectrum, built with SDCC alone: F9 compiles it and runs it.
// It starts at main, which comes first, at 32768. Without SDCC's own start-up code, globals don't get their
// first values, so main sets them.
__sfr __at 0xFE ula;

void main(void)
{
    unsigned char *attributes = (unsigned char *)0x5800;
    unsigned int i;
    ula = 2; // a red border
    for (i = 0; i < 768; i++)
        attributes[i] = (unsigned char)((i & 7) << 3); // stripes of colour
    for (;;)
        ;
}
";

/// The Master System, with z88dk.
pub const MASTER_SYSTEM: &str = "// machine: sms
// A C program for the Sega Master System: F9 compiles it with z88dk and runs it.
#include <stdio.h>

int main(void)
{
    printf(\"Hello from Limoniter\\n\");
    for (;;)
        ;
}
";

/// The Mega Drive, with SGDK.
pub const MEGA_DRIVE: &str = "// machine: megadrive
// A Mega Drive program in C: F9 builds the cartridge with SGDK and runs it.
#include <genesis.h>

int main(bool hardReset)
{
    VDP_drawText(\"Hello from Limoniter\", 10, 13);
    while (TRUE)
    {
        // Wait for the next frame, letting SGDK do its work.
        SYS_doVBlankProcess();
    }
    return 0;
}
";

/// The Amiga, with amiga-gcc or vbcc: both open dos.library as the program starts.
pub const AMIGA: &str = "// machine: amiga
// An Amiga program in C: F9 compiles it with amiga-gcc or vbcc and runs it.
#include <proto/exec.h>
#include <proto/dos.h>

int main(void)
{
    Printf(\"Hello from Limoniter\\n\");
    Delay(150); // three seconds, at 50 a second
    return 0;
}
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::running::Machine;

    #[test]
    fn each_starter_says_its_machine() {
        for (text, machine) in [(C64, Machine::C64), (SPECTRUM, Machine::Spectrum48), (SPECTRUM_SDCC, Machine::Spectrum48), (MASTER_SYSTEM, Machine::MasterSystem), (MEGA_DRIVE, Machine::MegaDrive), (AMIGA, Machine::Amiga)] {
            assert_eq!(crate::compilers::c_machine(text.lines()), Some(machine), "{text}");
        }
        assert_eq!(crate::compilers::compiler_said(SPECTRUM_SDCC.lines()).map(|c| c.id), Some("sdcc"));
        assert_eq!(crate::compilers::compiler_said(SPECTRUM.lines()), None, "the first one found");
    }
}
