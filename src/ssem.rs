//! The Manchester Baby (the Small-Scale Experimental Machine, SSEM) built into Limoniter, for teaching: the first
//! computer to run a program kept in its own memory, on 21 June 1948.
//!
//! It is built here as it was, from these sources:
//!
//! - its store: 32 words ("lines") of 32 bits, kept as dots and dashes on a Williams tube, a dash for a one; numbers
//!   in two's complement, written and shown with the least significant bit first, at the left (Geoff Tootill's
//!   notebook, as the University of Manchester's "Digital 60" pages show it, notes that "binary was written out with
//!   least significant bit first"). Its one register, the accumulator (A), and the control tube's two lines, the
//!   control instruction (CI, which line the program is at) and the present instruction (PI), were tubes of their
//!   own.
//! - its seven instructions (Wikipedia's "Manchester Baby", and the table in the Digital 60 pages): a line's bits 0
//!   to 4 say which store line it uses, and bits 13 to 15 the function:
//!
//!   | Function | Written then  | Written now | Does                                         |
//!   |----------|---------------|-------------|----------------------------------------------|
//!   | 0        | S, Cl         | JMP S       | CI = S (jump to the line after S's number)   |
//!   | 1        | Add S, Cl     | JRP S       | CI = CI + S (a relative jump)                |
//!   | 2        | -S, C         | LDN S       | A = -S                                       |
//!   | 3        | c, S          | STO S       | S = A                                        |
//!   | 4 and 5  | Sub S         | SUB S       | A = A - S                                    |
//!   | 6        | Test          | CMP         | skip the next line if A is negative          |
//!   | 7        | Stop          | STP         | stop, and light the stop lamp                |
//!
//! - its order of working: CI goes up by one, the line it then points at is fetched into PI, and that is done. So a
//!   program starts with CI at 0 and runs from line 1, and a jump's line holds one less than where it goes.
//! - its speed: a store line could be read or written in 360 microseconds, and an instruction took four times that,
//!   about 700 instructions a second (Wikipedia, after Napper).
//!
//! Kilburn's highest factor routine, as amended and written out in Tootill's notebook on 18 July 1948, is the
//! example (see `KILBURN`). The machine had no keyboard and no tape: programs were set a bit at a time on its
//! typewriter buttons, which Limoniter's panel does when the person clicks a dot on the store.
//!
//! The source Limoniter reads for it is a line per store line: an optional line number and a colon, then one of the
//! seven instructions with its store line, or NUM and a number to put there. Labels (a name and a colon) can stand
//! for line numbers, and a 32-digit binary word (least significant bit first, as Tootill wrote them) can be given
//! instead of an instruction. A semicolon starts a comment.

use std::collections::BTreeMap;

/// The store's size, in lines.
pub const LINES: usize = 32;
/// How long an instruction took, in microseconds: four beats of 360 microseconds.
pub const MICROSECONDS_PER_INSTRUCTION: u64 = 4 * 360;

/// The instructions, by their function number (bits 13 to 15), as written today.
pub const MNEMONICS: [&str; 8] = ["JMP", "JRP", "LDN", "STO", "SUB", "SUB", "CMP", "STP"];

/// The Baby: its store, accumulator and control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Baby {
    pub store: [u32; LINES],
    /// The accumulator.
    pub a: u32,
    /// The control instruction: the line the program is at.
    pub ci: u32,
    /// The present instruction: the line being done.
    pub pi: u32,
    /// Whether it has stopped at a STP.
    pub stopped: bool,
    /// Instructions done since it was started.
    pub done: u64,
}

impl Baby {
    /// The machine with a program in its store, its control cleared, ready to run from line 1.
    pub fn new(store: [u32; LINES]) -> Baby {
        Baby { store, a: 0, ci: 0, pi: 0, stopped: false, done: 0 }
    }

    /// Does one instruction, unless it has stopped.
    pub fn step(&mut self) {
        if self.stopped {
            return;
        }
        self.ci = self.ci.wrapping_add(1);
        self.pi = self.store[line_of(self.ci)];
        let s = line_of(self.pi);
        match function(self.pi) {
            0 => self.ci = self.store[s],
            1 => self.ci = self.ci.wrapping_add(self.store[s]),
            2 => self.a = self.store[s].wrapping_neg(),
            3 => self.store[s] = self.a,
            4 | 5 => self.a = self.a.wrapping_sub(self.store[s]),
            6 => {
                if (self.a as i32) < 0 {
                    self.ci = self.ci.wrapping_add(1);
                }
            }
            _ => self.stopped = true,
        }
        self.done += 1;
    }

    /// Runs until it stops, or for at most `most` instructions; how many were done.
    #[cfg(test)]
    pub fn run(&mut self, most: u64) -> u64 {
        let start = self.done;
        while !self.stopped && self.done - start < most {
            self.step();
        }
        self.done - start
    }

    /// The line the next instruction will come from.
    pub fn next_line(&self) -> usize {
        line_of(self.ci.wrapping_add(1))
    }

    /// Sets or clears one bit of a store line, as the typewriter buttons did.
    pub fn flip(&mut self, line: usize, bit: u32) {
        self.store[line % LINES] ^= 1 << (bit % 32);
    }
}

/// The store line an instruction (or CI) names: its bits 0 to 4.
pub fn line_of(word: u32) -> usize {
    (word & 31) as usize
}

/// An instruction's function number: its bits 13 to 15.
pub fn function(word: u32) -> u32 {
    (word >> 13) & 7
}

/// A line as an instruction, as written today ("LDN 24"), and as a number.
pub fn describe(word: u32) -> String {
    let f = function(word);
    match f {
        6 | 7 => MNEMONICS[f as usize].to_string(),
        _ => format!("{} {}", MNEMONICS[f as usize], line_of(word)),
    }
}

/// A word written as Tootill wrote them: 32 binary digits, the least significant first.
pub fn binary(word: u32) -> String {
    (0..32).map(|bit| if word >> bit & 1 == 1 { '1' } else { '0' }).collect()
}

/// A program assembled: the store it fills, and which source line each store line came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Program {
    pub store: [u32; LINES],
    pub lines: BTreeMap<usize, usize>,
}

impl Program {
    /// The store line a source line put something in.
    pub fn address_of(&self, source_line: usize) -> Option<usize> {
        self.lines.iter().find(|(_, l)| **l == source_line).map(|(a, _)| *a)
    }

    /// The source line a store line came from.
    pub fn line_at(&self, address: usize) -> Option<usize> {
        self.lines.get(&address).copied()
    }
}

/// A problem in the source: the line (from 0) and what's wrong.
pub type Problem = (usize, String);

/// Reads Baby source into its store.
pub fn assemble(source: &str) -> Result<Program, Vec<Problem>> {
    // Two passes: the first places every line and learns the labels, the second makes the words.
    let mut labels: BTreeMap<String, i64> = BTreeMap::new();
    let mut statements: Vec<(usize, usize, String, String)> = Vec::new();
    let mut problems: Vec<Problem> = Vec::new();
    let mut at = 0usize;
    for (number, raw) in source.lines().enumerate() {
        let text = raw.split(';').next().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        let mut rest = text;
        // A line number or label before a colon (or a bare line number, as listings were written).
        while let Some((head, tail)) = rest.split_once(':') {
            let head = head.trim();
            if head.is_empty() || head.contains(char::is_whitespace) {
                break;
            }
            match head.parse::<usize>() {
                Ok(n) if n < LINES => at = n,
                Ok(_) => problems.push((number, crate::i18n::tf!("The Baby's store has lines 0 to 31; there's no line {head}."))),
                Err(_) => {
                    labels.insert(head.to_ascii_uppercase(), at as i64);
                }
            }
            rest = tail.trim();
        }
        if rest.is_empty() {
            continue;
        }
        let (word, operand) = match rest.split_once(char::is_whitespace) {
            Some((w, o)) => (w.to_string(), o.trim().to_string()),
            None => (rest.to_string(), String::new()),
        };
        if word.parse::<usize>().is_ok() && !operand.is_empty() && !is_binary_word(&word) {
            // "01 LDN 24": a line number without its colon.
            match word.parse::<usize>() {
                Ok(n) if n < LINES => at = n,
                _ => problems.push((number, crate::i18n::tf!("The Baby's store has lines 0 to 31; there's no line {word}."))),
            }
            let (w, o) = match operand.split_once(char::is_whitespace) {
                Some((w, o)) => (w.to_string(), o.trim().to_string()),
                None => (operand.clone(), String::new()),
            };
            statements.push((number, at, w, o));
        } else {
            statements.push((number, at, word, operand));
        }
        at += 1;
        if at > LINES {
            problems.push((number, crate::i18n::tr("The program goes past the store's last line, 31.").into()));
            at = LINES;
        }
    }
    let mut program = Program::default();
    for (number, at, word, operand) in statements {
        if at >= LINES {
            continue;
        }
        match encode(&word, &operand, &labels) {
            Ok(value) => {
                if let Some(earlier) = program.lines.insert(at, number) {
                    problems.push((number, crate::i18n::tf!("Line {at} of the store was already filled, by source line {}.", earlier + 1)));
                }
                program.store[at] = value;
            }
            Err(problem) => problems.push((number, problem)),
        }
    }
    if problems.is_empty() { Ok(program) } else { Err(problems) }
}

fn is_binary_word(word: &str) -> bool {
    word.len() == 32 && word.chars().all(|c| c == '0' || c == '1')
}

/// One line's word.
fn encode(word: &str, operand: &str, labels: &BTreeMap<String, i64>) -> Result<u32, String> {
    if is_binary_word(word) {
        return Ok(word.chars().enumerate().fold(0u32, |w, (bit, c)| if c == '1' { w | 1 << bit } else { w }));
    }
    let upper = word.to_ascii_uppercase();
    let function = match upper.as_str() {
        "JMP" => 0,
        "JRP" | "JPR" => 1,
        "LDN" => 2,
        "STO" => 3,
        "SUB" => 4,
        "CMP" | "SKN" => 6,
        "STP" | "HLT" => 7,
        "NUM" => return value(operand, labels).map(|v| v as u32),
        _ => return Err(crate::i18n::tf!("{word} isn't one of the Baby's instructions (JMP, JRP, LDN, STO, SUB, CMP, STP) or NUM.")),
    };
    let line = if function >= 6 {
        if !operand.is_empty() {
            return Err(crate::i18n::tf!("{upper} doesn't use a store line."));
        }
        0
    } else {
        let v = value(operand, labels)?;
        if !(0..LINES as i64).contains(&v) {
            return Err(crate::i18n::tf!("{upper} needs a store line from 0 to 31."));
        }
        v as u32
    };
    Ok(function << 13 | line)
}

/// A number, a label, or a label with a number added or taken away.
fn value(text: &str, labels: &BTreeMap<String, i64>) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(crate::i18n::tr("A number or a label is needed here.").into());
    }
    let mut total: i64 = 0;
    let mut sign = 1;
    let mut term = String::new();
    let add = |term: &mut String, sign: i64, total: &mut i64| -> Result<(), String> {
        let t = term.trim();
        if t.is_empty() {
            return Ok(());
        }
        let v = match t.parse::<i64>() {
            Ok(v) => v,
            Err(_) => *labels.get(&t.to_ascii_uppercase()).ok_or_else(|| crate::i18n::tf!("There's no label called {t}."))?,
        };
        *total += sign * v;
        term.clear();
        Ok(())
    };
    for c in text.chars() {
        match c {
            '+' | '-' => {
                add(&mut term, sign, &mut total)?;
                sign = if c == '-' { -1 } else { 1 };
            }
            _ => term.push(c),
        }
    }
    add(&mut term, sign, &mut total)?;
    if !(-(1i64 << 31)..(1i64 << 32)).contains(&total) {
        return Err(crate::i18n::tr("That number is too big for one of the Baby's 32-bit words.").into());
    }
    Ok(total)
}

/// Kilburn's highest factor routine, as Tootill wrote it out on 18 July 1948 ("Kilburn Highest Factor Routine
/// (amended)"), from the page of his notebook the University of Manchester shows in its Digital 60 pages. It finds
/// the highest factor of a, here 2^18 as in the first run on 21 June 1948, by trying each b from a - 1 downwards,
/// taking b away again and again until what's left goes negative. The original words are in the comments; the
/// answer, b, is left in line 27 (and -b in line 26), and line 25 holds the remainder, 0.
pub const KILBURN: &str = "\
; Kilburn's highest factor routine (amended), as Geoff Tootill wrote it out in his
; notebook on 18 July 1948. The first version ran on 21 June 1948, the first program
; ever run from a computer's own memory. It finds the highest factor of a (2^18 here,
; as then) by trying every b from a-1 downwards, dividing by taking b away again and
; again. Its words in 1948 are in the comments.
;
; It took 52 minutes on the Baby. Choose \"As fast as it goes\" to see the answer at once:
; b (131072) in line 27, -b in line 26, and the remainder, 0, in line 25.
00: NUM 0
01: LDN 24     ; -24 to C      A = -b1 (the first b to try)
02: STO 26     ; c to 26       line 26 = -b
03: LDN 26     ; -26 to C      A = b
04: STO 27     ; c to 27       line 27 = b
05: LDN 23     ; -23 to C      A = a
06: SUB 27     ; Sub 27        take b away
07: CMP        ; Test          gone negative?
08: JRP 20     ; Add 20 to Cl  not yet: back to line 6 (CI + -3)
09: SUB 26     ; Sub 26        add b back: the remainder
10: STO 25     ; c to 25
11: LDN 25     ; -25 to C      A = -remainder
12: CMP        ; Test          remainder above 0?
13: STP        ; Stop          no: b divides a exactly
14: LDN 26     ; -26 to C      A = b
15: SUB 21     ; Sub 21        b - 1
16: STO 27     ; c to 27
17: LDN 27     ; -27 to C
18: STO 26     ; c to 26
19: JMP 22     ; 22 to Cl      back to line 5 (CI = 4)
20: NUM -3     ; the jump back from line 8
21: NUM 1
22: NUM 4      ; the jump back from line 19
23: NUM -262144 ; -a
24: NUM 262143  ; b1 = a - 1
";

/// A first program of Limoniter's own, small enough to follow a line at a time: it counts down from 5, keeping
/// the count in line 30, where the dots can be seen changing, and stops at 0.
pub const COUNTDOWN: &str = "\
; A first program for the Manchester Baby, written for Limoniter.
; It counts down from 5 to 0 in line 30, where you can watch the dots change,
; then stops. Step through it with Step to see each instruction work.
; The Baby can only load a number negated (LDN), subtract (SUB) and store (STO),
; so adding is done by subtracting a negative number.
00:     NUM 0
01:     LDN count   ; A = -count
        STO neg     ; keep -count
        LDN neg     ; A = count
        SUB one     ; A = count - 1
        STO count
        CMP         ; is count now below 0? (it went 0 -> -1)
        JMP loop    ; no: go round again (to line 1)
        LDN zero    ; yes: put 0 back
        STO count
        STP
loop:   NUM 0       ; a jump goes to the line after the number it finds here
one:    NUM 1
zero:   NUM 0
neg:    NUM 0
30: count: NUM 5
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seven_instructions_work_as_the_sources_say() {
        let mut store = [0u32; LINES];
        // LDN 10 makes A the negative of line 10.
        store[1] = 2 << 13 | 10;
        store[10] = 7;
        let mut baby = Baby::new(store);
        baby.step();
        assert_eq!(baby.a as i32, -7);
        assert_eq!(baby.ci, 1, "CI is increased before the line is fetched, so it starts at line 1");
        // SUB 11 takes line 11 away from A.
        baby.store[2] = 4 << 13 | 11;
        baby.store[11] = 3;
        baby.step();
        assert_eq!(baby.a as i32, -10);
        // Function 5 subtracts too.
        baby.store[3] = 5 << 13 | 11;
        baby.step();
        assert_eq!(baby.a as i32, -13);
        // STO 12 stores A.
        baby.store[4] = 3 << 13 | 12;
        baby.step();
        assert_eq!(baby.store[12] as i32, -13);
        // CMP skips the next line when A is negative...
        baby.store[5] = 6 << 13;
        baby.step();
        assert_eq!(baby.ci, 6);
        // ...and not when it isn't.
        baby.a = 0;
        baby.store[7] = 6 << 13;
        baby.step();
        assert_eq!(baby.ci, 7);
        // JMP 13 sets CI to line 13's number, so the next line done is the one after it.
        baby.store[8] = 13;
        baby.store[13] = 19;
        baby.step();
        assert_eq!(baby.ci, 19);
        assert_eq!(baby.next_line(), 20);
        // JRP 14 adds line 14 to CI.
        baby.store[20] = 1 << 13 | 14;
        baby.store[14] = (-5i32) as u32;
        baby.step();
        assert_eq!(baby.ci, 15);
        // STP stops it, and it stays stopped.
        baby.store[16] = 7 << 13;
        baby.step();
        assert!(baby.stopped);
        let done = baby.done;
        baby.step();
        assert_eq!(baby.done, done);
    }

    #[test]
    fn words_are_written_least_significant_bit_first_as_tootill_wrote_them() {
        // Line 1 of the notebook's listing, -24 to C: address 00011 and function 010, read from the left.
        let program = assemble(KILBURN).unwrap();
        let word = binary(program.store[1]);
        assert_eq!(&word[0..5], "00011");
        assert_eq!(&word[13..16], "010");
        // Line 20 is -3: "10111 etc".
        assert_eq!(&binary(program.store[20])[0..5], "10111");
        assert_eq!(&binary(program.store[22])[0..5], "00100");
        // And a binary word reads back as it was written.
        let back = assemble(&format!("5: {}", binary(0x8000_0005))).unwrap();
        assert_eq!(back.store[5], 0x8000_0005);
        assert_eq!(describe(program.store[19]), "JMP 22");
        assert_eq!(describe(program.store[12]), "CMP");
    }

    #[test]
    fn kilburns_routine_finds_the_highest_factor_of_two_to_the_eighteen() {
        let program = assemble(KILBURN).unwrap();
        let mut baby = Baby::new(program.store);
        baby.run(10_000_000);
        assert!(baby.stopped);
        // It stops at line 13, with b = 131072 in line 27, -b in 26 and no remainder in 25.
        assert_eq!(baby.ci, 13);
        assert_eq!(baby.store[27], 131_072);
        assert_eq!(baby.store[26] as i32, -131_072);
        assert_eq!(baby.store[25], 0);
        // Sixteen instructions for each b tried, from 262143 down to 131072: about 2.1 million, which at the Baby's
        // 1.44 milliseconds an instruction is the 52 minutes Wikipedia gives for the first run. (It also says
        // "about 3.5 million operations", for the first, unamended version.)
        assert!((2_000_000..2_200_000).contains(&baby.done), "{} instructions", baby.done);
        let minutes = baby.done * MICROSECONDS_PER_INSTRUCTION / 60_000_000;
        assert!((48..=53).contains(&minutes), "{minutes} minutes");
    }

    #[test]
    fn the_routine_works_for_other_numbers_too() {
        // 35 = 5 x 7: the highest factor is 7.
        let source = KILBURN.replace("NUM -262144", "NUM -35").replace("NUM 262143", "NUM 34");
        let mut baby = Baby::new(assemble(&source).unwrap().store);
        baby.run(100_000);
        assert_eq!(baby.store[27], 7);
    }

    #[test]
    fn the_countdown_counts_down_and_labels_work() {
        let program = assemble(COUNTDOWN).unwrap();
        assert_eq!(program.store[30], 5);
        let mut baby = Baby::new(program.store);
        baby.run(1000);
        assert!(baby.stopped);
        assert_eq!(baby.store[30], 0);
        // Every source line that made a word is known by its store line, and back.
        assert_eq!(program.line_at(30).map(|l| COUNTDOWN.lines().nth(l).unwrap().trim().starts_with("30:")), Some(true));
        assert_eq!(program.address_of(program.line_at(1).unwrap()), Some(1));
    }

    #[test]
    fn mistakes_are_found_with_their_lines() {
        let problems = assemble("1: LDN 40\n2: FRED 3\n3: CMP 4\n4: LDN nowhere").unwrap_err();
        assert_eq!(problems.iter().map(|p| p.0).collect::<Vec<_>>(), [0, 1, 2, 3]);
        let problems = assemble("1: NUM 1\n1: NUM 2").unwrap_err();
        assert_eq!(problems[0].0, 1);
        assert!(assemble("40: NUM 1").is_err());
    }

    #[test]
    fn listings_without_colons_are_read_too() {
        let program = assemble("01 LDN 24\n02 STO 26\n24 NUM 9").unwrap();
        assert_eq!(program.store[1], 2 << 13 | 24);
        assert_eq!(program.store[2], 3 << 13 | 26);
        assert_eq!(program.store[24], 9);
    }
}
