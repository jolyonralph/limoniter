//! Debugging a BASIC program on the built-in Spectrum: where the ROM's interpreter starts each statement, so it
//! can stop at a line or step a statement at a time, and the program's variables, read from the VARS area as the
//! ROM keeps them (the 48K manual, chapter 24).

/// Where the 48K ROM's statement loop has just counted the next statement (after INC (SUBPPC) at $1B2C), and
/// where the 128K's own BASIC ROM does the same. PPC and SUBPPC there are the line and statement about to run.
pub const STATEMENT_48: u16 = 0x1B32;
pub const STATEMENT_128: u16 = 0x17C7;

/// System variables.
pub const PPC: u16 = 23621;
pub const SUBPPC: u16 = 23623;
pub const VARS: u16 = 23627;
pub const E_LINE: u16 = 23641;

/// A number in the Spectrum's five bytes: a small whole number, or floating point with an exponent byte.
pub fn number(bytes: [u8; 5]) -> f64 {
    if bytes[0] == 0 {
        let whole = f64::from(u16::from_le_bytes([bytes[2], bytes[3]]));
        return if bytes[1] == 0xFF { whole - 65536.0 } else { whole };
    }
    let exponent = i32::from(bytes[0]) - 128;
    let negative = bytes[1] & 0x80 != 0;
    let mantissa = u32::from_be_bytes([bytes[1] | 0x80, bytes[2], bytes[3], bytes[4]]);
    let value = f64::from(mantissa) / 4_294_967_296.0 * 2f64.powi(exponent);
    if negative { -value } else { value }
}

/// A number as PRINT would show it, near enough: whole numbers without a point, others to eight figures.
pub fn shown(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e10 {
        return format!("{}", value as i64);
    }
    let text = format!("{value:.8}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.len() > 14 { format!("{value:.6e}") } else { text.to_string() }
}

/// One of the program's variables: its name as BASIC writes it, and its value in words.
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub name: String,
    pub value: String,
}

/// The program's variables, read from VARS up to the 0x80 that ends them.
pub fn variables(peek: &dyn Fn(u16) -> u8) -> Vec<Variable> {
    let word = |a: u16| u16::from_le_bytes([peek(a), peek(a.wrapping_add(1))]);
    let five = |a: u16| [peek(a), peek(a.wrapping_add(1)), peek(a.wrapping_add(2)), peek(a.wrapping_add(3)), peek(a.wrapping_add(4))];
    let mut at = word(VARS);
    let end = word(E_LINE);
    let mut found = Vec::new();
    while at < end && found.len() < 500 {
        let first = peek(at);
        if first == 0x80 {
            break;
        }
        let letter = char::from((first & 0x1F) + 0x60);
        match first >> 5 {
            // A number with a one-letter name.
            0b011 => {
                found.push(Variable { name: letter.to_string(), value: shown(number(five(at + 1))) });
                at += 6;
            }
            // A number with a longer name: its letters, the last with bit 7 set.
            0b101 => {
                let mut name = letter.to_string();
                let mut i = at + 1;
                loop {
                    let c = peek(i);
                    name.push(char::from(c & 0x7F).to_ascii_lowercase());
                    i += 1;
                    if c & 0x80 != 0 || i.wrapping_sub(at) > 255 {
                        break;
                    }
                }
                found.push(Variable { name, value: shown(number(five(i))) });
                at = i + 5;
            }
            // A FOR loop's control variable: its value, limit and step, and where it loops back to.
            0b111 => {
                let value = shown(number(five(at + 1)));
                let limit = shown(number(five(at + 6)));
                let step = shown(number(five(at + 11)));
                let line = word(at + 16);
                let statement = peek(at + 18);
                let by = if step == "1" { String::new() } else { format!(" STEP {step}") };
                found.push(Variable { name: letter.to_string(), value: format!("{value}   (FOR to {limit}{by}, back to line {line}:{statement})") });
                at += 19;
            }
            // A string.
            0b010 => {
                let length = word(at + 1);
                let text: String = (0..length.min(200)).map(|i| zx_char(peek(at + 3 + i))).collect();
                let more = if length > 200 { "\u{2026}" } else { "" };
                found.push(Variable { name: format!("{letter}$"), value: format!("\"{text}{more}\"") });
                at = at + 3 + length;
            }
            // An array of numbers, or of characters: the dimensions and the first few.
            kind @ (0b100 | 0b110) => {
                let length = word(at + 1);
                let count = peek(at + 3);
                let dims: Vec<u16> = (0..u16::from(count)).map(|d| word(at + 4 + d * 2)).collect();
                let data = at + 4 + u16::from(count) * 2;
                let shape: Vec<String> = dims.iter().map(u16::to_string).collect();
                let value = if kind == 0b100 {
                    let total: u32 = dims.iter().map(|d| u32::from(*d)).product();
                    let first: Vec<String> = (0..total.min(8) as u16).map(|i| shown(number(five(data + i * 5)))).collect();
                    let more = if total > 8 { ", \u{2026}" } else { "" };
                    format!("({}) = {}{more}", shape.join(","), first.join(", "))
                } else {
                    let total: u32 = dims.iter().map(|d| u32::from(*d)).product();
                    let text: String = (0..total.min(32) as u16).map(|i| zx_char(peek(data + i))).collect();
                    format!("({}) = \"{text}{}\"", shape.join(","), if total > 32 { "\u{2026}" } else { "" })
                };
                let suffix = if kind == 0b110 { "$" } else { "" };
                found.push(Variable { name: format!("{letter}{suffix}"), value });
                at = at + 3 + length;
            }
            _ => break,
        }
    }
    found
}

fn zx_char(byte: u8) -> char {
    match byte {
        0x60 => '\u{A3}',
        0x7F => '\u{A9}',
        0x20..=0x7E => char::from(byte),
        _ => '\u{B7}',
    }
}

/// The BASIC line number a line of a listing is, from its start.
pub fn line_number(text: &str) -> Option<u16> {
    let digits: String = text.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|n| *n <= 9999)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_read_as_the_spectrum_keeps_them() {
        assert_eq!(number([0, 0, 5, 0, 0]), 5.0);
        assert_eq!(number([0, 0xFF, 0xFB, 0xFF, 0]), -5.0);
        for value in [2.5, -0.1, 1e6, 1.23456, 65536.0, -100000.5] {
            let bytes = crate::basic::zx_number(value).unwrap();
            assert!((number(bytes) - value).abs() < value.abs() * 1e-8, "{value}: {}", number(bytes));
        }
        assert_eq!(shown(2.5), "2.5");
        assert_eq!(shown(7.0), "7");
        assert_eq!(shown(-0.25), "-0.25");
        assert_eq!(line_number("  20 PRINT a"), Some(20));
        assert_eq!(line_number("PRINT"), None);
    }

    fn basic_machine(lines: &[&str], machine: crate::running::Machine) -> crate::zx_machine::BuiltIn {
        use crate::running::{Format, Runnable};
        let program = crate::basic::tokenise_spectrum(lines, crate::basic::ZxDialect::Spectrum).unwrap();
        let tape = crate::basic::spectrum_tape(&program, "steps", Some(10));
        let run = Runnable { machine, format: Format::Tap, name: "steps.tap".into(), bytes: std::sync::Arc::new(tape) };
        crate::zx_machine::BuiltIn::start(&run).unwrap()
    }

    #[test]
    fn a_basic_program_stops_at_its_lines_and_steps_a_statement_at_a_time() {
        for kind in [crate::running::Machine::Spectrum48, crate::running::Machine::Spectrum128] {
            let mut machine = basic_machine(&["10 LET a=1", "20 LET a=a+1: LET b=a*2", "30 GO TO 20"], kind);
            machine.set_basic_breakpoints(Some([20].into()));
            let a = |m: &crate::zx_machine::BuiltIn| variables(&|x| m.peek(x)).into_iter().find(|v| v.name == "a").map(|v| v.value);
            let run = |m: &mut crate::zx_machine::BuiltIn| {
                for _ in 0..100 {
                    m.frame().unwrap();
                    if m.basic_stopped.is_some() {
                        return;
                    }
                }
            };
            run(&mut machine);
            assert_eq!(machine.basic_stopped, Some((20, 1)), "{kind:?}: stopped as line 20 starts");
            assert_eq!(a(&machine).as_deref(), Some("1"), "line 10 has run, line 20 hasn't");
            machine.carry_on();
            run(&mut machine);
            assert_eq!((machine.basic_stopped, a(&machine).as_deref()), (Some((20, 1)), Some("2")), "{kind:?}: round again");
            machine.step_basic().unwrap();
            assert_eq!(machine.basic_stopped, Some((20, 2)), "{kind:?}: the next statement, on the same line");
            machine.step_basic().unwrap();
            assert_eq!(machine.basic_stopped, Some((30, 1)));
            assert_eq!(variables(&|x| machine.peek(x)).iter().find(|v| v.name == "b").map(|v| v.value.as_str()), Some("6"));
            machine.step_basic().unwrap();
            assert_eq!(machine.basic_stopped, Some((20, 1)), "{kind:?}: GO TO 20");
            machine.set_basic_breakpoints(None);
            machine.carry_on();
            run(&mut machine);
            assert_eq!(machine.basic_stopped, None, "{kind:?}: with debugging off it runs on");
            // A breakpoint on the first line stops before it's run at all.
            let mut machine = basic_machine(&["10 LET a=1", "20 GO TO 20"], kind);
            machine.set_basic_breakpoints(Some([10].into()));
            assert_eq!(machine.basic_stopped, Some((10, 1)), "{kind:?}");
            assert!(variables(&|x| machine.peek(x)).is_empty(), "nothing has run yet");
            machine.step_basic().unwrap();
            assert_eq!(a(&machine).as_deref(), Some("1"));
        }
    }

    #[test]
    fn a_running_program_s_variables_are_read() {
        use crate::running::{Format, Machine, Runnable};
        let program = crate::basic::tokenise_spectrum(&["10 LET a=5: LET total=2.5: DIM d(3): LET d(2)=7: LET s$=\"hi\": DIM c$(2,3): FOR i=1 TO 3", "20 GO TO 20"], crate::basic::ZxDialect::Spectrum).unwrap();
        let tape = crate::basic::spectrum_tape(&program, "vars", Some(10));
        let run = Runnable { machine: Machine::Spectrum48, format: Format::Tap, name: "vars.tap".into(), bytes: std::sync::Arc::new(tape) };
        let mut machine = crate::zx_machine::BuiltIn::start(&run).unwrap();
        for _ in 0..50 {
            machine.frame().unwrap();
        }
        let vars = variables(&|a| machine.peek(a));
        let get = |name: &str| vars.iter().find(|v| v.name == name).map(|v| v.value.clone());
        assert_eq!(get("a").as_deref(), Some("5"));
        assert_eq!(get("total").as_deref(), Some("2.5"));
        assert_eq!(get("d").as_deref(), Some("(3) = 0, 7, 0"));
        assert_eq!(get("s$").as_deref(), Some("\"hi\""));
        assert_eq!(get("c$").as_deref(), Some("(2,3) = \"      \""));
        assert_eq!(get("i").as_deref(), Some("1   (FOR to 3, back to line 10:8)"));
    }
}
