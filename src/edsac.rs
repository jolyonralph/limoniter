//! EDSAC, built into Limoniter for teaching: Cambridge's Electronic Delay Storage Automatic Calculator, the first
//! stored-program computer to give a regular computing service, which ran its first program on 6 May 1949.
//!
//! It is built here from these sources, which are cited where they're used:
//!
//! - Martin Richards's poster "EDSAC Initial Orders and Squares Program" (Computer Laboratory, Cambridge): the
//!   store, the order code of 1949, Initial Orders 1 and the squares program, and the speeds of the tape reader
//!   (50 characters a second) and the Creed teleprinter (nearly 7 a second).
//! - The Initial Orders 2 of September 1949 (David Wheeler), as listed in Mark Smotherman's EDSAC page (Clemson) and
//!   in the uniselector table of QEdsac (the Warwick simulator's successor), which agree order for order; how they're
//!   used is in Martin Campbell-Kelly's "EDSAC Simulator Tutorial Guide" and in Wilkes, Wheeler and Gill (1951).
//! - Wikipedia's "EDSAC": 1.5 milliseconds for an ordinary order and 6 for a multiplication.
//!
//! The store is 1024 short (17-bit) words, as it was from 1952 (until then only the first 512 were there). Two
//! short words make a long (35-bit) one, w[2n] being m[2n+1], a sandwich bit and m[2n], so the even word holds 18
//! bits. The accumulator is 71 bits (A, its top 17, and AB, its top 35), and the multiplier register 35 (R, its
//! top 17). Numbers are fractions: the top bit is the sign.
//!
//! An order is 17 bits: five for the function (the code of its letter on the tape), one unused, ten for the address
//! and one saying whether it works on a short word (S, or F in Initial Orders 2) or a long one (L, or D).
//!
//! A program was punched on five-hole tape in letters and figures, and read in by the Initial Orders, which were
//! wired into the first words of the store: they turned each order, written as a letter, a number and S or L (F or
//! D), into its 17 bits. Limoniter does the same: the source is the tape, and the real Initial Orders read it. On the
//! tape, the four signs the teleprinter's keys had are typed as # (pi, figure shift), @ (theta, carriage return),
//! ! (phi, space) and & (delta, line feed), as Richards and the Warwick simulator write them; * is erase (letter
//! shift) and . the blank. Spaces and new lines aren't punched, and text in [square brackets] is a comment.

use std::collections::BTreeMap;

/// The tape's characters by their five-bit code, in letter shift (Richards's poster; QEdsac's table).
pub const TAPE_LETTERS: &str = "PQWERTYUIOJ#SZK*.F@D!HNM&LXGABCV";
/// The teleprinter's characters by code, in letter shift and in figure shift; \0 for figure shift, letter shift
/// and blank, \r and \n for carriage return and line feed (Richards's poster).
const PRINTED_LETTERS: [char; 32] = ['P', 'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'J', '\0', 'S', 'Z', 'K', '\0', '\0', 'F', '\r', 'D', ' ', 'H', 'N', 'M', '\n', 'L', 'X', 'G', 'A', 'B', 'C', 'V'];
const PRINTED_FIGURES: [char; 32] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '?', '\0', '"', '+', '(', '\0', '\0', '$', '\r', ';', ' ', '£', ',', '.', '\n', ')', '/', '#', '-', '?', ':', '='];
const FIGURE_SHIFT: u8 = 11;
const LETTER_SHIFT: u8 = 15;

/// The store's size, in short words.
pub const WORDS: usize = 1024;
const SHORT: u32 = 0x1FFFF;
const LONG: u64 = (1 << 35) - 1;

/// How long things take, in microseconds (Wikipedia; Richards's poster for the reader and teleprinter).
pub const ORDER_TIME: u64 = 1_500;
pub const MULTIPLY_TIME: u64 = 6_000;
pub const READ_TIME: u64 = 20_000;
pub const PRINT_TIME: u64 = 150_000;

/// Which Initial Orders are wired into the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orders {
    /// May 1949: 31 orders, S and L only, addresses as they are.
    One,
    /// September 1949: 41 orders, with F and D, the control combinations, and addresses counted from a routine's
    /// start (theta) for subroutines.
    Two,
}

impl Orders {
    /// How many words they fill.
    pub fn len(self) -> usize {
        match self {
            Orders::One => INITIAL_ORDERS_1.len(),
            Orders::Two => INITIAL_ORDERS_2.len(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Orders::One => "Initial Orders 1",
            Orders::Two => "Initial Orders 2",
        }
    }
}

/// Initial Orders 1 (Wheeler, May 1949), as Richards's poster lists them, each as its letter, address and length.
const INITIAL_ORDERS_1: [(char, u32, bool); 31] = [
    ('T', 0, false),
    ('H', 2, false),
    ('T', 0, false),
    ('E', 6, false),
    ('P', 1, false),
    ('P', 5, false),
    ('T', 0, false),
    ('I', 0, false),
    ('A', 0, false),
    ('R', 16, false),
    ('T', 0, true),
    ('I', 2, false),
    ('A', 2, false),
    ('S', 5, false),
    ('E', 21, false),
    ('T', 3, false),
    ('V', 1, false),
    ('L', 8, false),
    ('A', 2, false),
    ('T', 1, false),
    ('E', 11, false),
    ('R', 4, false),
    ('A', 1, false),
    ('L', 0, true),
    ('A', 0, false),
    ('T', 31, false),
    ('A', 25, false),
    ('A', 4, false),
    ('U', 25, false),
    ('S', 31, false),
    ('G', 6, false),
];

/// Initial Orders 2 (Wheeler, September 1949), as Smotherman's page and QEdsac list them.
const INITIAL_ORDERS_2: [(char, u32, bool); 41] = [
    ('T', 0, false),
    ('E', 20, false),
    ('P', 1, false),
    ('U', 2, false),
    ('A', 39, false),
    ('R', 4, false),
    ('V', 0, false),
    ('L', 8, false),
    ('T', 0, false),
    ('I', 1, false),
    ('A', 1, false),
    ('S', 39, false),
    ('G', 4, false),
    ('L', 0, true),
    ('S', 39, false),
    ('E', 17, false),
    ('S', 7, false),
    ('A', 35, false),
    ('T', 20, false),
    ('A', 0, false),
    ('H', 8, false),
    ('A', 40, false),
    ('T', 43, false),
    ('A', 22, false),
    ('A', 2, false),
    ('T', 22, false),
    ('E', 34, false),
    ('A', 43, false),
    ('E', 8, false),
    ('A', 42, false),
    ('A', 40, false),
    ('E', 25, false),
    ('A', 22, false),
    ('T', 42, false),
    ('I', 40, true),
    ('A', 40, true),
    ('R', 16, false),
    ('T', 40, true),
    ('E', 8, false),
    ('P', 5, true),
    ('P', 0, true),
];

/// A letter's five-bit code on the tape.
pub fn code_of(letter: char) -> Option<u8> {
    let letter = letter.to_ascii_uppercase();
    match letter {
        '0'..='9' => Some(letter as u8 - b'0'),
        'π' => Some(11),
        'θ' => Some(18),
        'φ' => Some(20),
        'Δ' | 'δ' => Some(24),
        _ => TAPE_LETTERS.find(letter).map(|at| at as u8),
    }
}

/// An order's 17 bits from its letter, address and length.
pub fn order(letter: char, address: u32, long: bool) -> u32 {
    (u32::from(code_of(letter).unwrap_or(0)) << 12 | (address & 0x7FF) << 1 | u32::from(long)) & SHORT
}

/// An order written as it would be punched ("A 40 D" in Initial Orders 2's way, or "A40L" in the first's).
pub fn describe(word: u32, orders: Orders) -> String {
    let letter = TAPE_LETTERS.as_bytes()[(word >> 12 & 31) as usize] as char;
    let address = word >> 1 & 0x3FF;
    let length = match (orders, word & 1 == 1) {
        (Orders::One, false) => 'S',
        (Orders::One, true) => 'L',
        (Orders::Two, false) => 'F',
        (Orders::Two, true) => 'D',
    };
    format!("{letter} {address} {length}")
}

/// A tape: each character's code, and the source line it was punched from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tape {
    pub codes: Vec<u8>,
    pub lines: Vec<usize>,
}

/// Punches source onto tape: letters and figures as their codes, spaces and new lines left out, and [comments]
/// too. Each problem is a line (from 0) and what's wrong.
pub fn punch(source: &str) -> Result<Tape, Vec<(usize, String)>> {
    let mut tape = Tape::default();
    let mut problems = Vec::new();
    let mut comment = false;
    for (line, text) in source.lines().enumerate() {
        for c in text.chars() {
            match c {
                '[' => comment = true,
                ']' if comment => comment = false,
                _ if comment || c.is_whitespace() => {}
                _ => match code_of(c) {
                    Some(code) => {
                        tape.codes.push(code);
                        tape.lines.push(line);
                    }
                    None => problems.push((line, crate::i18n::tf!("{c} can't be punched on EDSAC's tape. It takes letters, figures, and # @ ! & * . for its other keys."))),
                },
            }
        }
    }
    if problems.is_empty() { Ok(tape) } else { Err(problems) }
}

/// Which Initial Orders a tape is for: as a [comment] says (Initial Orders 1 or 2), or else 2 if it uses their
/// control combinations (K) or F and D, and 1 if its orders end only in S and L.
pub fn orders_for(source: &str) -> Orders {
    let lower = source.to_ascii_lowercase();
    if lower.contains("initial orders 1") {
        return Orders::One;
    }
    if lower.contains("initial orders 2") {
        return Orders::Two;
    }
    let mut comment = false;
    let mut two = false;
    for c in source.chars() {
        match c {
            '[' => comment = true,
            ']' => comment = false,
            'K' | 'F' | 'D' | 'k' | 'f' | 'd' if !comment => two = true,
            _ => {}
        }
    }
    if two { Orders::Two } else { Orders::One }
}

/// Why it stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// A Z order: the program has finished (and rings the bell).
    Finished,
    /// An I order with no more tape.
    TapeEnded,
    /// A function that isn't an order (P, Q, W...), and where.
    NotAnOrder(usize),
}

/// The teleprinter: what it has printed, a line at a time, and whether it's in figure shift.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Teleprinter {
    pub lines: Vec<String>,
    column: usize,
    pub figures: bool,
}

impl Teleprinter {
    fn print(&mut self, code: u8) {
        let code = usize::from(code & 31);
        match code as u8 {
            FIGURE_SHIFT => self.figures = true,
            LETTER_SHIFT => self.figures = false,
            _ => {
                let c = if self.figures { PRINTED_FIGURES[code] } else { PRINTED_LETTERS[code] };
                match c {
                    '\0' => {}
                    '\r' => self.column = 0,
                    '\n' => self.lines.push(String::new()),
                    _ => {
                        if self.lines.is_empty() {
                            self.lines.push(String::new());
                        }
                        let line = self.lines.last_mut().expect("a line");
                        let mut chars: Vec<char> = line.chars().collect();
                        while chars.len() < self.column {
                            chars.push(' ');
                        }
                        if self.column < chars.len() {
                            chars[self.column] = c;
                        } else {
                            chars.push(c);
                        }
                        *line = chars.into_iter().collect();
                        self.column += 1;
                    }
                }
            }
        }
    }

    /// Everything printed, as text.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// The machine.
#[derive(Clone, Debug)]
pub struct Edsac {
    /// The store: short words, the even ones with the sandwich bit above them.
    pub store: Vec<u32>,
    /// The accumulator, 71 bits, kept as a signed number.
    pub acc: i128,
    /// The multiplier register, 35 bits, signed.
    pub rs: i64,
    /// The sequence control tank: where the next order comes from.
    pub sct: usize,
    /// The order tank: the order last done.
    pub order: u32,
    pub orders: Orders,
    pub tape: Tape,
    pub read: usize,
    pub printer: Teleprinter,
    last_printed: u8,
    pub stopped: Option<Stop>,
    /// Orders done, and the time they took, in microseconds.
    pub done: u64,
    pub time: u64,
    /// Whether the Initial Orders have finished reading the tape and the program is running.
    pub loaded: bool,
    /// Which source line put each word in the store, as the Initial Orders put it there.
    pub placed: BTreeMap<usize, usize>,
}

/// Keeps a value to the accumulator's 71 bits, as a signed number.
fn wrap71(x: i128) -> i128 {
    (x << 57) >> 57
}

fn signed35(w: u64) -> i64 {
    ((w << 29) as i64) >> 29
}

impl Edsac {
    /// The machine cleared, with the Initial Orders in the store and a tape in the reader, ready to start.
    pub fn new(orders: Orders, tape: Tape) -> Edsac {
        let mut store = vec![0u32; WORDS];
        let table: &[(char, u32, bool)] = match orders {
            Orders::One => &INITIAL_ORDERS_1,
            Orders::Two => &INITIAL_ORDERS_2,
        };
        for (at, (letter, address, long)) in table.iter().enumerate() {
            store[at] = order(*letter, *address, *long);
        }
        Edsac { store, acc: 0, rs: 0, sct: 0, order: 0, orders, tape, read: 0, printer: Teleprinter::default(), last_printed: 0, stopped: None, done: 0, time: 0, loaded: false, placed: BTreeMap::new() }
    }

    /// The machine reading a source's tape with the Initial Orders it's for.
    pub fn from_source(source: &str) -> Result<Edsac, Vec<(usize, String)>> {
        Ok(Edsac::new(orders_for(source), punch(source)?))
    }

    /// A short word.
    pub fn short(&self, n: usize) -> u32 {
        self.store[n % WORDS] & SHORT
    }

    /// A long word, from the even address at or below n.
    pub fn long(&self, n: usize) -> u64 {
        let n = (n % WORDS) & !1;
        u64::from(self.store[n + 1] & SHORT) << 18 | u64::from(self.store[n] & 0x3FFFF)
    }

    fn set_short(&mut self, n: usize, value: u32) {
        self.store[n % WORDS] = value & SHORT;
    }

    fn set_long(&mut self, n: usize, value: u64) {
        let n = (n % WORDS) & !1;
        self.store[n] = (value & 0x3FFFF) as u32;
        self.store[n + 1] = (value >> 18) as u32 & SHORT;
    }

    /// The accumulator's top 17 bits (A) and top 35 (AB).
    pub fn a(&self) -> u32 {
        (self.acc >> 54) as u32 & SHORT
    }

    pub fn ab(&self) -> u64 {
        (self.acc >> 36) as u64 & LONG
    }

    /// The accumulator's 71 bits, as an unsigned number.
    pub fn abc(&self) -> u128 {
        (self.acc as u128) & ((1u128 << 71) - 1)
    }

    /// An operand as a 35-bit pattern: a short word at the top, or a long word.
    fn operand(&self, n: usize, long: bool) -> u64 {
        if long { self.long(n) } else { u64::from(self.short(n)) << 18 }
    }

    /// Does one order, unless it has stopped.
    pub fn step(&mut self) {
        if self.stopped.is_some() {
            return;
        }
        if self.sct >= self.orders.len() {
            self.loaded = true;
        }
        let at = self.sct;
        let word = self.short(at);
        self.order = word;
        self.sct = (self.sct + 1) % WORDS;
        let function = (word >> 12 & 31) as usize;
        let n = (word >> 1 & 0x3FF) as usize;
        let long = word & 1 == 1;
        let letter = TAPE_LETTERS.as_bytes()[function] as char;
        let mut time = ORDER_TIME;
        match letter {
            'A' => self.acc = wrap71(self.acc + (i128::from(self.operand(n, long)) << 36)),
            'S' => self.acc = wrap71(self.acc - (i128::from(self.operand(n, long)) << 36)),
            'H' => self.rs = signed35(self.operand(n, long)),
            'V' | 'N' => {
                // Fractions multiplied: a 35-bit fraction (x 2^34) by another, into the 71-bit accumulator (x 2^70).
                let product = i128::from(signed35(self.operand(n, long))) * i128::from(self.rs) * 4;
                self.acc = wrap71(if letter == 'V' { self.acc + product } else { self.acc - product });
                time = MULTIPLY_TIME;
            }
            'C' => self.acc = wrap71(self.acc + (i128::from(self.operand(n, long) & (self.rs as u64 & LONG)) << 36)),
            'T' | 'U' => {
                if long {
                    self.set_long(n, self.ab());
                } else {
                    self.set_short(n, self.a());
                }
                // A word the Initial Orders put in the store came from the tape line last read.
                if at < self.orders.len() && n >= self.orders.len() && self.read > 0 {
                    let line = self.tape.lines[self.read - 1];
                    self.placed.insert(n, line);
                    if long {
                        self.placed.insert((n & !1) + 1, line);
                    }
                }
                if letter == 'T' {
                    self.acc = 0;
                }
            }
            'R' | 'L' => {
                // As many places as where the order's lowest one is, counting from one.
                let places = word.trailing_zeros() + 1;
                self.acc = if letter == 'L' { wrap71(self.acc << places) } else { self.acc >> places };
            }
            'E' if self.acc >= 0 => self.sct = n,
            'G' if self.acc < 0 => self.sct = n,
            'E' | 'G' | 'X' => {}
            'Y' => self.acc = wrap71(self.acc + (1 << 35)),
            'I' => {
                let Some(code) = self.tape.codes.get(self.read).copied() else {
                    self.sct = at;
                    self.stopped = Some(Stop::TapeEnded);
                    return;
                };
                self.read += 1;
                if long {
                    self.set_short(n & !1, 0);
                    self.set_short((n & !1) + 1, u32::from(code));
                } else {
                    self.set_short(n, u32::from(code));
                }
                time = READ_TIME;
            }
            'O' => {
                let from = if long && n.is_multiple_of(2) { n + 1 } else { n };
                let code = (self.short(from) >> 12 & 31) as u8;
                self.printer.print(code);
                self.last_printed = code;
                time = PRINT_TIME;
            }
            'F' => {
                if long {
                    self.set_short(n & !1, 0);
                    self.set_short((n & !1) + 1, u32::from(self.last_printed) << 12);
                } else {
                    self.set_short(n, u32::from(self.last_printed) << 12);
                }
            }
            'Z' => self.stopped = Some(Stop::Finished),
            _ => {
                self.sct = at;
                self.stopped = Some(Stop::NotAnOrder(at));
                return;
            }
        }
        self.done += 1;
        self.time += time;
    }

    /// Runs until it stops, or for at most `most` orders.
    #[cfg(test)]
    pub fn run(&mut self, most: u64) {
        let start = self.done;
        while self.stopped.is_none() && self.done - start < most {
            self.step();
        }
    }

    /// Where it is: the next order, or the Z order it finished at.
    pub fn at(&self) -> usize {
        if self.stopped == Some(Stop::Finished) { (self.sct + WORDS - 1) % WORDS } else { self.sct }
    }

    /// The source line the order it's at came from, once the program has been loaded.
    pub fn line_now(&self) -> Option<usize> {
        let at = self.at();
        if self.loaded || at >= self.orders.len() { self.placed.get(&at).copied() } else { None }
    }

    /// The short word as a signed integer.
    pub fn integer(word: u32) -> i32 {
        ((word << 15) as i32) >> 15
    }
}

/// Wilkes's table of squares and their differences, the program run at the conference in Cambridge in June 1949,
/// for Initial Orders 1, as Martin Richards's poster gives it (with its orders, locations and comments) and as his
/// simulator's squares.txt punches it.
pub const SQUARES: &str = "\
[Initial Orders 1]
[A table of squares and their differences for 1 to 100, by Maurice Wilkes,
 run on EDSAC in June 1949, as Martin Richards's poster gives it. The orders
 go in from location 31; the comments give each one's location.
 Choose As fast as it goes to see the whole table quickly.]
T123S [31: the first word: the address after the last, for the loader]
E84S  [32: jump to the start, at 84]
PS    [33: the next decimal digit]
PS    [34: the current power of ten]
P10000S [35-39: the powers of ten]
P1000S
P100S
P10S
P1S
QS    [40: one in the top five bits, to make digits]
#S    [41: figure shift]
A40S  [42: the end limit for the order in 52]
!S    [43: space]
&S    [44: line feed]
@S    [45: carriage return]
O43S  [46: write a space]
O33S  [47: write a digit]
PS    [48: the number to print]
A46S  [49: the print subroutine starts here]
T65S
T129S [51]
A35S  [52: steps through A35S to A39S]
T34S
E61S
T48S  [55]
A47S
T65S
A33S
A40S
T33S  [60]
A48S  [61]
S34S
E55S
A34S
PS    [65: O43S (a space) or O33S (a digit) is put here]
T48S
T33S
A52S
A4S
U52S  [70]
S42S
G51S
A117S
T52S
PS    [75: the return jump]
PS    [76: x]
PS    [77: x squared]
PS    [78: the previous square]
PS    [79: the difference]
E110S [80]
E118S
P100S [82: the last x]
E95S
O41S  [84: the main program: figure shift]
T129S
O44S
O45S
A76S
A4S
U76S  [90: x = x + 1]
T48S
A83S
T75S
E49S  [print x]
O43S  [95]
O43S
H76S
V76S
L64S
L32S  [100]
U77S  [x squared]
S78S
T79S  [the difference]
A77S
U78S  [105]
T48S
A80S
T75S
E49S  [print the square]
O43S  [110]
O43S
A79S
T48S
A81S
T75S  [115]
E49S  [print the difference]
A35S
A76S  [118]
S82S
G85S  [120: again, until x is 100]
O41S
ZS    [122: stop]
";

/// A first program for Initial Orders 2, written for Limoniter: it prints a greeting a letter at a time, by
/// changing the address of its own print order each time round, as programs did then.
pub const HELLO: &str = "\
[Initial Orders 2]
[A first EDSAC program, written for Limoniter. It prints HELLO EDSAC on the
 teleprinter a letter at a time. EDSAC had no index registers, so the loop adds
 one to the address in its own print order (O) each time round, as programs did
 then. Addresses ending in @ (theta) count from where this routine starts.]
T64K    [load the program from location 64]
GK      [theta: this routine starts here]
O12@    [ 0: print the character in location 12 of the routine]
T11@    [ 1: clear the accumulator]
A@      [ 2: take order 0 into the accumulator]
A9@     [ 3: add one to its address]
U@      [ 4: and put it back]
S10@    [ 5: is it past the last character yet?]
G@      [ 6: not yet: print the next]
ZF      [ 7: stop, and ring the bell]
PF      [ 8: spare]
P1F     [ 9: one, in the address's place]
O26@    [10: the print order past the last character]
PF      [11: a spare word to clear the accumulator into]
*F      [12: letter shift]
HF
EF
LF
LF
OF
!F      [18: space]
EF
DF
SF
AF
CF
@F      [24: carriage return]
&F      [25: line feed]
EZPF    [start the program, at theta]
";

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(orders: Orders, program: &[(usize, u32)]) -> Edsac {
        let mut edsac = Edsac::new(orders, Tape::default());
        for (at, word) in program {
            edsac.store[*at] = *word;
        }
        edsac
    }

    // The bits are grouped as the poster groups them: function, unused bit, address, length.
    #[allow(clippy::unusual_byte_groupings)]
    #[test]
    fn orders_are_put_together_as_the_poster_shows_them() {
        // Richards's poster: R16S is 00100 0 0000010000 0, T0L 00101 0 0000000000 1, and IO1's second order H2S
        // is 10101 0 0000000010 0.
        assert_eq!(order('R', 16, false), 0b00100_0_0000010000_0);
        assert_eq!(order('T', 0, true), 0b00101_0_0000000000_1);
        let edsac = Edsac::new(Orders::One, Tape::default());
        assert_eq!(edsac.short(1), 0b10101_0_0000000010_0);
        assert_eq!(edsac.short(30), 0b11011_0_0000000110_0);
        assert_eq!(describe(edsac.short(10), Orders::One), "T 0 L");
        let two = Edsac::new(Orders::Two, Tape::default());
        assert_eq!(describe(two.short(34), Orders::Two), "I 40 D");
        assert_eq!(code_of('#'), Some(11));
        assert_eq!(code_of('V'), Some(31));
        assert_eq!(code_of('7'), Some(7));
    }

    #[test]
    fn shifts_go_by_the_lowest_one_in_the_order() {
        // The poster: R0L, R1S, R16S and R0S shift by 1, 2, 6 and 15 places; L0L, L1S, L16S, L64S and L0S by 1,
        // 2, 6, 8 and 13.
        for (letter, address, long, places) in [('R', 0, true, 1), ('R', 1, false, 2), ('R', 16, false, 6), ('R', 0, false, 15), ('L', 0, true, 1), ('L', 1, false, 2), ('L', 16, false, 6), ('L', 64, false, 8), ('L', 0, false, 13)] {
            let mut edsac = machine(Orders::One, &[(100, order(letter, address, long))]);
            edsac.sct = 100;
            edsac.acc = 1 << 40;
            edsac.step();
            let expected: i128 = if letter == 'L' { 1 << (40 + places) } else { 1 << (40 - places) };
            assert_eq!(edsac.acc, expected, "{letter}{address}{}", if long { "L" } else { "S" });
        }
    }

    #[test]
    fn the_arithmetic_orders_work_on_fractions() {
        // m[200] = 1/4 and m[201] = 1/2, as 17-bit fractions; m[202] = -1/4.
        let quarter = 1 << 14;
        let half = 1 << 15;
        let minus_quarter = (SHORT + 1 - quarter) & SHORT;
        let program = [(100, order('A', 200, false)), (101, order('A', 201, false)), (102, order('S', 202, false)), (103, order('T', 210, false)), (104, order('H', 201, false)), (105, order('V', 200, false)), (106, order('U', 211, false)), (107, order('N', 201, false)), (108, order('U', 212, false)), (109, order('Z', 0, false)), (200, quarter), (201, half), (202, minus_quarter)];
        let mut edsac = machine(Orders::One, &program);
        edsac.sct = 100;
        edsac.run(100);
        assert_eq!(edsac.stopped, Some(Stop::Finished));
        // 1/4 + 1/2 - (-1/4) = 1 would overflow; it's 1/4 + 1/2 + 1/4 wrapped into -1, the sign bit alone.
        assert_eq!(edsac.short(210), 1 << 16);
        // T cleared it; then 1/2 x 1/4 = 1/8.
        assert_eq!(edsac.short(211), 1 << 13);
        // And 1/8 - 1/2 x 1/2 = -1/8.
        assert_eq!(Edsac::integer(edsac.short(212)), -(1 << 13));
        assert_eq!(edsac.time, 8 * ORDER_TIME + 2 * MULTIPLY_TIME, "Z takes its time too");
    }

    #[test]
    fn jumps_test_the_sign_of_the_accumulator() {
        let program = [(100, order('E', 110, false)), (110, order('S', 200, false)), (111, order('E', 120, false)), (112, order('G', 130, false)), (130, order('Z', 0, false)), (200, 1)];
        let mut edsac = machine(Orders::One, &program);
        edsac.sct = 100;
        edsac.run(10);
        // E jumps on zero, which isn't negative; after taking 1 away, G jumps and E doesn't.
        assert_eq!(edsac.stopped, Some(Stop::Finished));
        assert_eq!(edsac.sct, 131);
        assert_eq!(edsac.done, 5);
    }

    #[test]
    fn long_words_are_two_short_ones_with_the_sandwich_bit() {
        let mut edsac = machine(Orders::One, &[(100, order('A', 200, true)), (101, order('T', 300, true)), (102, order('Z', 0, false))]);
        edsac.store[200] = 0x3FFFF;
        edsac.store[201] = 5;
        edsac.sct = 100;
        edsac.run(10);
        assert_eq!(edsac.long(300), 5 << 18 | 0x3FFFF);
        assert_eq!(edsac.short(301), 5);
        // The short word reads without the sandwich bit.
        assert_eq!(edsac.short(300), SHORT);
    }

    #[test]
    fn the_initial_orders_1_read_a_tape() {
        // T35S (the first word: one past the last), then two orders.
        let tape = punch("T35S A33S ZS PS P10S").unwrap();
        let mut edsac = Edsac::new(Orders::One, tape);
        edsac.run(10_000);
        assert_eq!(edsac.short(32), order('A', 33, false));
        assert_eq!(edsac.short(33), order('Z', 0, false));
        assert_eq!(edsac.short(34), 0);
        // The loader then runs into the program at 31's order, T35S, then 32 onwards, which stops at Z.
        assert_eq!(edsac.stopped, Some(Stop::Finished));
    }

    #[test]
    fn wilkes_squares_print_the_table() {
        let mut edsac = Edsac::from_source(SQUARES).unwrap();
        assert_eq!(edsac.orders, Orders::One);
        edsac.run(5_000_000);
        assert_eq!(edsac.stopped, Some(Stop::Finished));
        let rows: Vec<Vec<i64>> = edsac.printer.lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.split_whitespace().map(|n| n.parse().unwrap()).collect()).collect();
        assert_eq!(rows.len(), 100, "{}", edsac.printer.text());
        for (i, row) in rows.iter().enumerate() {
            let x = i as i64 + 1;
            assert_eq!(row, &vec![x, x * x, 2 * x - 1]);
        }
        // Printed as the poster shows it, numbers right-aligned in five places.
        assert!(edsac.printer.lines.iter().any(|l| l.trim_end() == "  100  10000    199"), "{}", edsac.printer.text());
        // Each word knows the line of the tape it was punched from.
        let line = SQUARES.lines().position(|l| l.starts_with("O41S  [84")).unwrap();
        assert_eq!(edsac.placed.get(&84), Some(&line));
    }

    #[test]
    fn the_initial_orders_2_read_relative_addresses_and_start_the_program() {
        let mut edsac = Edsac::from_source(HELLO).unwrap();
        assert_eq!(edsac.orders, Orders::Two);
        edsac.run(1_000_000);
        assert_eq!(edsac.stopped, Some(Stop::Finished), "{}", edsac.printer.text());
        assert_eq!(edsac.printer.text().trim_end(), "HELLO EDSAC");
        // O12@ was put at 64 with 64 added: O 76 F; and then changed by the program as it went.
        assert_eq!(edsac.short(64 + 10), order('O', 64 + 26, false));
        assert_eq!(edsac.short(64 + 9), order('P', 1, false));
        assert_eq!(edsac.placed.get(&64), HELLO.lines().position(|l| l.starts_with("O12@")).as_ref());
    }

    #[test]
    fn the_initial_orders_2_set_the_long_bit_with_d() {
        let mut edsac = Edsac::from_source("T 64 K\nA 100 D\nT 101 F\nP 7 F\nZ F\nE 64 K P F").unwrap();
        edsac.run(100_000);
        assert_eq!(edsac.short(64), order('A', 100, true));
        assert_eq!(edsac.short(65), order('T', 101, false));
        assert_eq!(edsac.short(66), order('P', 7, false));
    }

    #[test]
    fn the_teleprinter_shifts_between_letters_and_figures() {
        let mut printer = Teleprinter::default();
        for c in "#QW!E*QW@&R".chars() {
            printer.print(code_of(c).unwrap());
        }
        assert_eq!(printer.text(), "12 3QW\nR");
    }

    #[test]
    fn what_cant_be_punched_is_found() {
        let problems = punch("A 1 S\nB ~ 2").unwrap_err();
        assert_eq!(problems[0].0, 1);
        assert_eq!(orders_for("T 64 K\nZ F"), Orders::Two);
        assert_eq!(orders_for("T35S ZS"), Orders::One);
        assert_eq!(orders_for("[Initial Orders 1] ZF"), Orders::One);
    }

    #[test]
    fn an_order_that_isnt_one_stops_it_and_so_does_the_end_of_the_tape() {
        let mut edsac = machine(Orders::One, &[(100, order('P', 0, false))]);
        edsac.sct = 100;
        edsac.step();
        assert_eq!(edsac.stopped, Some(Stop::NotAnOrder(100)));
        let mut edsac = Edsac::new(Orders::One, punch("T40S").unwrap());
        edsac.run(1000);
        assert_eq!(edsac.stopped, Some(Stop::TapeEnded));
    }
}
