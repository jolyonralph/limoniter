//! Boriel's ZX Basic, the BASIC compiler for the Spectrum most people use today (zxbc): telling its programs from
//! Sinclair BASIC listings and QBasic in a `.bas` file, its words with a line on what each does (for completion
//! and hover), and a program to start from. Compiling it is in `compilers`.

/// Whether lines read as Boriel's ZX Basic rather than a Sinclair BASIC listing or QBasic. Its own words settle it
/// (the integer types, the preprocessor's #include, ASM blocks, FASTCALL); otherwise it's a program without line
/// numbers that uses the Spectrum's statements, which a Sinclair listing can't be and QBasic doesn't have.
pub fn looks_like_boriel(lines: &[std::sync::Arc<str>]) -> bool {
    const OWN: [&str; 12] = ["UBYTE", "UINTEGER", "ULONG", "FIXED", "FASTCALL", "STDCALL", "END ASM", "#INCLUDE", "#DEFINE", "#PRAGMA", "BYREF", "BYVAL"];
    const SPECTRUM: [&str; 13] = ["BORDER ", "PAPER ", "INK ", "BRIGHT ", "FLASH ", "PLOT ", "DRAW ", "BEEP ", "RANDOMIZE", "OVER ", "INVERSE ", "PRINT AT ", "INKEY"];
    const STRUCTURED: [&str; 7] = ["SUB ", "FUNCTION ", "END SUB", "END FUNCTION", "END IF", " AS ", "WEND"];
    let (mut own, mut spectrum, mut structured, mut numbered, mut code) = (0, 0, 0, 0, 0);
    for line in lines.iter().take(2000) {
        let trimmed = line.trim();
        // Comments say nothing: a QBasic program can mention a Spectrum.
        if trimmed.is_empty() || trimmed.starts_with('\'') || trimmed.to_ascii_uppercase().starts_with("REM") {
            continue;
        }
        code += 1;
        let upper = format!(" {} ", trimmed.to_ascii_uppercase());
        numbered += usize::from(trimmed.starts_with(|c: char| c.is_ascii_digit()));
        own += OWN.iter().filter(|w| upper.contains(*w)).count();
        // "ASM" on a line of its own opens a block of Z80 assembler.
        own += usize::from(upper.trim() == "ASM");
        spectrum += SPECTRUM.iter().filter(|w| upper.contains(*w)).count();
        structured += STRUCTURED.iter().filter(|w| upper.contains(*w)).count();
    }
    if own > 0 {
        return true;
    }
    // A Sinclair listing has a number on every line.
    let unnumbered = code > 0 && numbered * 2 < code;
    unnumbered && spectrum > 0 && (structured > 0 || spectrum >= 2)
}

/// Boriel's ZX Basic's words, each with what it does, as its documentation describes them. Its reserved words
/// first, then the library's functions and subroutines that #include brings in.
pub const WORDS: [(&str, &str); 105] = [
    ("ABS", "ABS(x): x without its sign."),
    ("ACS", "ACS(x): the angle, in radians, whose cosine is x."),
    ("AND", "Logical AND: true when both sides are."),
    ("AS", "Gives a type: DIM x AS UBYTE, FUNCTION f(a AS INTEGER) AS LONG."),
    ("ASM", "Starts a block of Z80 assembler, ended by END ASM."),
    ("ASN", "ASN(x): the angle, in radians, whose sine is x."),
    ("AT", "PRINT AT row, column; puts the text there."),
    ("ATN", "ATN(x): the angle, in radians, whose tangent is x."),
    ("BAND", "bAND: bitwise AND of two integers."),
    ("BEEP", "BEEP duration, pitch: plays a note, as on the Spectrum."),
    ("BIN", "BIN 01010101: a number written in binary."),
    ("BNOT", "bNOT: every bit of an integer flipped."),
    ("BOLD", "BOLD 1: prints in bold (BOLD 0 to stop)."),
    ("BOR", "bOR: bitwise OR of two integers."),
    ("BORDER", "BORDER colour: sets the border's colour, 0 to 7."),
    ("BRIGHT", "BRIGHT 1: prints bright (BRIGHT 0 to stop)."),
    ("BXOR", "bXOR: bitwise exclusive OR of two integers."),
    ("BYREF", "Passes a parameter by reference: the SUB or FUNCTION can change the caller's variable."),
    ("BYVAL", "Passes a parameter by value, a copy (the usual way)."),
    ("BYTE", "A whole number from -128 to 127, in one byte."),
    ("CAST", "CAST(type, x): x converted to that type."),
    ("CHR", "CHR(n) or CHR$(n): the character with that code. Several codes give a string of them."),
    ("CIRCLE", "CIRCLE x, y, radius: draws a circle."),
    ("CLS", "CLS: clears the screen."),
    ("CODE", "CODE(s$): the code of the string's first character."),
    ("CONST", "CONST name AS type = value: a value that never changes."),
    ("CONTINUE", "CONTINUE FOR, WHILE or DO: goes straight to the loop's next turn."),
    ("COS", "COS(x): the cosine of x, in radians."),
    ("DATA", "DATA values: values for READ to take, in order."),
    ("DECLARE", "DECLARE FUNCTION f(...) AS type: lets a FUNCTION be used before it's written."),
    ("DIM", "DIM name AS type, or DIM a(10) AS UBYTE for an array: declares a variable."),
    ("DO", "DO ... LOOP, with WHILE or UNTIL at either end: a loop."),
    ("DRAW", "DRAW dx, dy: draws a line from the last point plotted."),
    ("ELSE", "IF ... THEN ... ELSE ... END IF: what's done when the condition is false."),
    ("ELSEIF", "ELSEIF condition THEN: another test, when the ones before were false."),
    ("END", "END: ends the program. END IF, END SUB, END FUNCTION, END WHILE and END ASM close a block."),
    ("EXIT", "EXIT FOR, WHILE, DO, SUB or FUNCTION: leaves it straight away."),
    ("EXP", "EXP(x): e to the power x."),
    ("FASTCALL", "SUB FASTCALL or FUNCTION FASTCALL: the first parameter comes in registers (A, HL or DE HL), for routines written in assembler."),
    ("FIXED", "A fixed-point number, 16 bits each side of the point: faster than FLOAT."),
    ("FLASH", "FLASH 1: prints flashing (FLASH 0 to stop)."),
    ("FLOAT", "A floating-point number, as Sinclair BASIC's (5 bytes)."),
    ("FOR", "FOR i = 1 TO 10 STEP 1 ... NEXT i: a counted loop."),
    ("FUNCTION", "FUNCTION name(parameters) AS type ... END FUNCTION: a routine that gives back a value."),
    ("GOSUB", "GOSUB label: calls the lines there, until RETURN."),
    ("GOTO", "GOTO label (or line number): carries on from there."),
    ("IF", "IF condition THEN ... END IF: does it only if the condition is true."),
    ("IN", "IN(port): reads a byte from a Z80 port."),
    ("INK", "INK colour: sets the colour text and drawing are in, 0 to 7."),
    ("INKEY", "INKEY or INKEY$: the key being pressed, or \"\" if none."),
    ("INPUT", "INPUT(n): reads up to n characters typed. Library: #include <input.bas>."),
    ("INT", "INT(x): x rounded down to a whole number."),
    ("INTEGER", "A whole number from -32768 to 32767, in two bytes."),
    ("INVERSE", "INVERSE 1: prints with ink and paper swapped (INVERSE 0 to stop)."),
    ("ITALIC", "ITALIC 1: prints slanted (ITALIC 0 to stop)."),
    ("LBOUND", "LBOUND(array, n): the lowest index of the array's nth dimension."),
    ("LEN", "LEN(s$): how many characters a string has."),
    ("LET", "LET x = value: sets a variable (LET can be left out)."),
    ("LN", "LN(x): the natural logarithm of x."),
    ("LOAD", "LOAD \"name\" CODE: loads bytes from tape, as on the Spectrum."),
    ("LONG", "A whole number from -2147483648 to 2147483647, in four bytes."),
    ("LOOP", "Ends a DO loop: LOOP, LOOP WHILE condition or LOOP UNTIL condition."),
    ("MOD", "a MOD b: the remainder when a is divided by b."),
    ("NEXT", "NEXT i: ends a FOR loop."),
    ("NOT", "NOT x: true when x is false."),
    ("OR", "Logical OR: true when either side is."),
    ("OUT", "OUT port, value: writes a byte to a Z80 port."),
    ("OVER", "OVER 1: prints and draws by exclusive OR, so drawing twice rubs out (OVER 0 to stop)."),
    ("PAPER", "PAPER colour: sets the background colour, 0 to 7."),
    ("PAUSE", "PAUSE n: waits n frames (50 a second); PAUSE 0 waits for a key."),
    ("PEEK", "PEEK(address), or PEEK(type, address): the byte (or value of that type) in memory there."),
    ("PI", "PI: 3.14159265..."),
    ("PLOT", "PLOT x, y: puts a dot there, 0 to 255 across and 0 to 191 up."),
    ("POKE", "POKE address, value, or POKE type address, value: writes to memory."),
    ("PRINT", "PRINT things: shows them, at the next place on the screen. ; keeps on the same line, , moves to the next half."),
    ("RANDOMIZE", "RANDOMIZE, or RANDOMIZE n: starts RND's sequence somewhere new."),
    ("READ", "READ variables: takes the next values from DATA."),
    ("REM", "REM or ': a comment, to the end of the line."),
    ("RESTORE", "RESTORE label: READ starts again from the DATA there."),
    ("RETURN", "RETURN: back from a GOSUB; RETURN value gives back a FUNCTION's value."),
    ("RND", "RND: a random number from 0 up to (not including) 1."),
    ("SAVE", "SAVE \"name\" CODE start, length: saves bytes to tape."),
    ("SGN", "SGN(x): -1, 0 or 1, as x is negative, zero or positive."),
    ("SHL", "a SHL n, or a << n: a's bits shifted n places left."),
    ("SHR", "a SHR n, or a >> n: a's bits shifted n places right."),
    ("SIN", "SIN(x): the sine of x, in radians."),
    ("SQR", "SQR(x): the square root of x."),
    ("STDCALL", "SUB STDCALL or FUNCTION STDCALL: parameters passed on the stack (the usual way)."),
    ("STEP", "FOR i = a TO b STEP n: how much i changes each time round."),
    ("STOP", "STOP: stops the program."),
    ("STR", "STR(x) or STR$(x): a number written as a string."),
    ("STRING", "A string of characters."),
    ("SUB", "SUB name(parameters) ... END SUB: a routine called by its name, which gives nothing back."),
    ("TAB", "PRINT TAB n; moves to column n."),
    ("TAN", "TAN(x): the tangent of x, in radians."),
    ("THEN", "IF condition THEN: what follows is done when the condition is true."),
    ("TO", "FOR i = a TO b: where the loop ends."),
    ("UBOUND", "UBOUND(array, n): the highest index of the array's nth dimension."),
    ("UBYTE", "A whole number from 0 to 255, in one byte: the fastest type."),
    ("UINTEGER", "A whole number from 0 to 65535, in two bytes."),
    ("ULONG", "A whole number from 0 to 4294967295, in four bytes."),
    ("UNTIL", "DO ... LOOP UNTIL condition: loops until it's true."),
    ("USR", "USR(address): calls machine code there and gives back BC."),
    ("VAL", "VAL(s$): the number a string holds."),
    ("WHILE", "WHILE condition ... END WHILE (or WEND): loops while it's true."),
];

/// The library's routines, from the files #include brings in, each with the file it's in.
pub const LIBRARY: [(&str, &str, &str); 17] = [
    ("ATTR", "attr.bas", "ATTR(row, column): the colours of a character square."),
    ("POINT", "point.bas", "POINT(x, y): 1 if the pixel there is set."),
    ("SCREEN", "screen.bas", "SCREEN(row, column) or SCREEN$: the character at a square."),
    ("CSRLIN", "csrlin.bas", "CSRLIN: the row PRINT will print on next."),
    ("POS", "pos.bas", "POS: the column PRINT will print in next."),
    ("HEX", "hex.bas", "HEX(n): a 32-bit number as eight hex digits."),
    ("HEX16", "hex.bas", "HEX16(n): a 16-bit number as four hex digits."),
    ("ASC", "asc.bas", "ASC(s$, n): the code of a string's nth character."),
    ("LCASE", "string.bas", "LCASE(s$): the string in small letters."),
    ("UCASE", "string.bas", "UCASE(s$): the string in capitals."),
    ("GETKEY", "keys.bas", "GetKey: waits for a key and gives its code."),
    ("MULTIKEYS", "keys.bas", "MultiKeys(code): whether a key is held down, read straight from the keyboard."),
    ("GETKEYSCANCODE", "keys.bas", "GetKeyScanCode: the code of the key held down, for MultiKeys."),
    ("PRINT42", "print42.bas", "print42(s$): prints in 42 columns."),
    ("PRINTAT42", "print42.bas", "printat42(row, column): where print42 prints next."),
    ("PRINT64", "print64.bas", "print64(s$): prints in 64 columns."),
    ("PRINTAT64", "print64.bas", "printat64(row, column): where print64 prints next."),
];

/// The words offered as you type, in capitals as the documentation writes them.
pub fn keywords() -> Vec<String> {
    let mut words: Vec<String> = WORDS.iter().map(|(w, _)| w.to_string()).chain(LIBRARY.iter().map(|(w, _, _)| w.to_string())).collect();
    words.sort();
    words.dedup();
    words
}

/// What a word does, for hover: a reserved word's line, or a library routine's and the file to #include.
pub fn describe(word: &str) -> Option<String> {
    let upper = word.trim_end_matches('$').to_ascii_uppercase();
    if let Some((_, file, words)) = LIBRARY.iter().find(|(w, _, _)| *w == upper) {
        return Some(format!("{}\n{}", crate::i18n::tr(words), crate::i18n::tf!("From the library: #include <{file}>")));
    }
    WORDS.iter().find(|(w, _)| *w == upper).map(|(_, words)| crate::i18n::tr(words).to_string())
}

/// Whether a column is in a comment (after ' or REM) or a string, where nothing is offered or described.
pub fn in_comment_or_string(line: &str, col: usize) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let mut quoted = false;
    for (i, c) in chars.iter().enumerate().take(col.min(chars.len())) {
        match c {
            '"' => quoted = !quoted,
            '\'' if !quoted => return true,
            'R' | 'r' if !quoted && (i == 0 || !chars[i - 1].is_alphanumeric()) => {
                let word: String = chars[i..].iter().take(4).collect();
                if word.len() >= 3 && word[..3].eq_ignore_ascii_case("rem") && !word[3..].starts_with(|c: char| c.is_alphanumeric() || c == '_') && col > i + 3 {
                    return true;
                }
            }
            _ => {}
        }
    }
    quoted
}

/// A program to start from: a border, some colour and a SUB, printed once it's run.
pub const STARTER: &str = "' A Boriel ZX Basic program: F9 compiles it with zxbc and runs it on the Spectrum.
#include <keys.bas>

DIM count AS UBYTE

SUB Banner(row AS UBYTE, colour AS UBYTE, text AS STRING)
    INK colour: PRINT AT row, 16 - LEN(text) / 2; text
END SUB

BORDER 1: PAPER 1: INK 7: CLS
Banner(10, 6, \"Hello from Limoniter\")
FOR count = 0 TO 7
    BEEP 0.05, count * 2
NEXT count
Banner(21, 5, \"Press a key\")
GetKey()
";

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn arcs(lines: &[&str]) -> Vec<Arc<str>> {
        lines.iter().map(|l| Arc::from(*l)).collect()
    }

    #[test]
    fn boriel_programs_are_told_from_sinclair_listings_and_qbasic() {
        assert!(looks_like_boriel(&arcs(&["DIM x AS UBYTE", "PRINT x"])), "its own types settle it");
        assert!(looks_like_boriel(&arcs(&["#include <keys.bas>", "PRINT \"hi\""])));
        assert!(looks_like_boriel(&arcs(&["SUB Wait()", "  ASM", "    halt", "  END ASM", "END SUB"])));
        assert!(looks_like_boriel(&arcs(&["BORDER 1: PAPER 1", "SUB hello()", "  PRINT \"hi\"", "END SUB"])), "the Spectrum's statements with no line numbers");
        assert!(looks_like_boriel(&arcs(&[STARTER])) || looks_like_boriel(&arcs(&STARTER.lines().collect::<Vec<_>>())));
        assert!(!looks_like_boriel(&arcs(&["10 BORDER 1: PAPER 1", "20 GO TO 10"])), "a Sinclair listing is numbered");
        assert!(!looks_like_boriel(&arcs(&["SCREEN 12", "DIM x AS INTEGER", "PRINT \"hello\""])), "QBasic");
        assert!(!looks_like_boriel(&arcs(&["10 PRINT\"{clr}HI\"", "20 GOTO10"])));
        assert!(!looks_like_boriel(&arcs(&["' about the BORDER and PAPER of a Spectrum", "PRINT 1"])), "comments don't count");
    }

    #[test]
    fn its_words_are_offered_and_described() {
        let words = keywords();
        assert!(words.contains(&"UBYTE".to_string()) && words.contains(&"FASTCALL".to_string()) && words.contains(&"MULTIKEYS".to_string()));
        assert!(words.windows(2).all(|w| w[0] < w[1]), "sorted, each once");
        assert!(describe("ubyte").is_some_and(|d| d.contains("0 to 255")));
        assert!(describe("Chr$").is_some_and(|d| d.contains("character")));
        assert!(describe("GetKey").is_some_and(|d| d.contains("#include <keys.bas>")));
        assert_eq!(describe("frobnicate"), None);
        assert!(in_comment_or_string("PRINT \"BORDER\"", 9));
        assert!(in_comment_or_string("x = 1 ' BORDER", 10));
        assert!(in_comment_or_string("REM BORDER", 6));
        assert!(!in_comment_or_string("PRINT AT 1,1; INK 2", 16), "a semicolon is PRINT's");
        assert!(!in_comment_or_string("REMOVE = 1", 4));
        for (word, _) in WORDS {
            assert_eq!(word, word.to_ascii_uppercase(), "kept in capitals, as they're looked up");
        }
    }
}
