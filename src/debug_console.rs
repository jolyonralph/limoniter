//! The debug console port: printf for assembler, on machines with no console. Each built-in machine has one
//! place (a port, or an address where the processor has no ports) that a real machine ignores writes to; the
//! bytes a program writes there are gathered into lines, and the lines appear in the Output panel, named for
//! the machine ("Spectrum: score 120").

/// The Z80 machines' port (the Spectrum, ZX80, ZX81 and Jupiter Ace). It's odd, and has bits 1, 5 and 7 set, so
/// the Spectrum's ULA, the 128K's paging and sound chip, the Kempston joystick, the ZX81's NMI switch and the
/// Ace's own port all leave it alone: `OUT ($BB),A` does nothing on a real machine.
pub const Z80_PORT: u8 = 0xBB;
/// The Atari 2600's address: the TIA only has registers up to $2C, and its decoding ignores a write to $2D,
/// so `STA $2D` does nothing on a real console.
pub const ATARI_2600_ADDRESS: u16 = 0x2D;
/// The Vectrex's address: $C000 to $C7FF has nothing on its bus, so a write there does nothing.
pub const VECTREX_ADDRESS: u16 = 0xC000;

/// The longest line kept; a program that never ends its line has it cut here.
const LONGEST_LINE: usize = 200;
/// The most lines held before the Output panel next collects them, so a program printing in a loop can't use up memory.
const MOST_HELD: usize = 500;

/// What a machine has been written to its console port, a line at a time.
#[derive(Clone, Debug, Default)]
pub struct Console {
    line: Vec<u8>,
    lines: Vec<String>,
}

impl Console {
    /// A byte written to the port. A new line (10), a carriage return (13) or a zero ends the line, and an empty
    /// line is let go so the two of a CR LF pair don't print a blank one.
    pub fn put(&mut self, byte: u8) {
        match byte {
            0 | 10 | 13 => self.end_line(),
            _ => {
                self.line.push(byte);
                if self.line.len() >= LONGEST_LINE {
                    self.end_line();
                }
            }
        }
    }

    fn end_line(&mut self) {
        if self.line.is_empty() {
            return;
        }
        // The machines' own characters above 127 aren't ASCII, so they're shown as what Latin-1 would make of them.
        let text: String = self.line.drain(..).map(|b| if (32..127).contains(&b) || b >= 160 { char::from(b) } else if b == 9 { ' ' } else { '\u{FFFD}' }).collect();
        if self.lines.len() < MOST_HELD {
            self.lines.push(text);
        }
    }

    /// The lines finished since last asked, which are no longer held.
    pub fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }
}

/// A line as the Output panel shows it, named for the machine that printed it.
pub fn output_line(machine: &str, line: &str) -> String {
    format!("{machine}: {line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(console: &mut Console, text: &str) {
        for b in text.bytes() {
            console.put(b);
        }
    }

    #[test]
    fn bytes_become_lines_at_new_lines() {
        let mut console = Console::default();
        print(&mut console, "score 120\nlives 3\r\npart");
        assert_eq!(console.take(), vec!["score 120", "lives 3"]);
        // The unfinished line waits for its end.
        print(&mut console, "ly\0");
        assert_eq!(console.take(), vec!["partly"]);
        assert!(console.take().is_empty());
    }

    #[test]
    fn odd_bytes_and_long_lines_are_kept_in_check() {
        let mut console = Console::default();
        print(&mut console, &"x".repeat(450));
        console.put(10);
        let lines = console.take();
        assert_eq!(lines.iter().map(String::len).collect::<Vec<_>>(), vec![200, 200, 50]);
        console.put(1);
        console.put(b'a');
        console.put(10);
        assert_eq!(console.take(), vec!["\u{FFFD}a"]);
    }

    #[test]
    fn lines_are_named_for_the_machine() {
        assert_eq!(output_line("Spectrum", "score 120"), "Spectrum: score 120");
    }
}
