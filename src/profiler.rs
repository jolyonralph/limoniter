//! Where a program's time goes, as the built-in Spectrum counts it: T-states spent at each address, gathered by
//! routine (a label and what follows it up to the next) and by source line.

use crate::asm::Assembly;
use std::collections::BTreeMap;

/// A routine's share of the time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: String,
    pub t_states: u64,
    /// How many times its first instruction ran: how often it was called or looped to.
    pub entered: u64,
    /// Its first line, where the source says.
    pub line: Option<usize>,
}

/// A source line's share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotLine {
    pub line: usize,
    pub t_states: u64,
    pub runs: u64,
}

/// The time by routine, most first, by line, most first, and the total.
pub fn summarise(t_states: &[u64], runs: &[u64], assembly: Option<&Assembly>) -> (Vec<Routine>, Vec<HotLine>, u64) {
    let total: u64 = t_states.iter().sum();
    let mut routines: BTreeMap<String, Routine> = BTreeMap::new();
    let mut lines: BTreeMap<usize, HotLine> = BTreeMap::new();
    for (address, spent) in t_states.iter().enumerate().filter(|(_, t)| **t > 0) {
        let address = address as u16;
        let named = assembly.and_then(|a| crate::debug_panel::label_of(address, a));
        let (name, start) = match &named {
            Some(label) => {
                let base = label.split('+').next().unwrap_or(label).to_string();
                let start = assembly.and_then(|a| a.symbols.get(&base)).map(|v| *v as u16);
                (base, start)
            }
            None if address < 0x4000 => ("the ROM".to_string(), None),
            None => ("elsewhere".to_string(), None),
        };
        let line = start.and_then(|s| assembly.and_then(|a| a.line_at(i64::from(s)))).filter(|(file, _)| *file == 0).map(|(_, l)| l);
        let entry = routines.entry(name.clone()).or_insert(Routine { name, t_states: 0, entered: 0, line });
        entry.t_states += spent;
        if Some(address) == start {
            entry.entered += runs[usize::from(address)];
        }
        if let Some((0, line)) = assembly.and_then(|a| a.line_at(i64::from(address))) {
            let hot = lines.entry(line).or_insert(HotLine { line, t_states: 0, runs: 0 });
            hot.t_states += spent;
            hot.runs = hot.runs.max(runs[usize::from(address)]);
        }
    }
    let mut routines: Vec<Routine> = routines.into_values().collect();
    routines.sort_by_key(|r| std::cmp::Reverse(r.t_states));
    let mut lines: Vec<HotLine> = lines.into_values().collect();
    lines.sort_by_key(|l| std::cmp::Reverse(l.t_states));
    (routines, lines, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::running::{Format, Machine, Runnable};

    /// A loop that calls a slow routine and a quick one: the slow one takes most of the time, as counted by the
    /// built-in Spectrum.
    #[test]
    fn the_time_goes_where_the_program_spends_it() {
        let source = "  org 32768\nmain: call slow\n  call quick\n  jr main\nslow: ld b,200\n.wait: djnz .wait\n  ret\nquick: ld a,1\n  ret\n";
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("t.asm"), source, &|_| Err("no".into()));
        assert!(assembly.ok());
        let tape = crate::asm::output::spectrum_tape(&assembly, "t").unwrap();
        let run = Runnable { machine: Machine::Spectrum48, format: Format::Tap, name: "t.tap".into(), bytes: std::sync::Arc::new(tape) };
        let mut machine = crate::zx_machine::BuiltIn::start(&run).unwrap();
        // Until it's running at main.
        machine.set_breakpoints([0x8000]);
        for _ in 0..50 * 30 {
            machine.frame().unwrap();
            if machine.stopped.is_some() {
                break;
            }
        }
        assert!(machine.stopped.is_some());
        machine.set_breakpoints([]);
        machine.set_profiling(true);
        machine.carry_on();
        for _ in 0..10 {
            machine.frame().unwrap();
        }
        machine.set_profiling(false);
        assert!(!machine.profiling());
        machine.frame().unwrap();
        let (t, runs) = machine.profile().expect("what was counted is kept after it stops");
        let (routines, lines, total) = summarise(&t, &runs, Some(&assembly));
        // Ten frames of a 48K are 698,880 T-states.
        assert!((690_000..=710_000).contains(&total), "{total}");
        assert_eq!(routines[0].name, "slow", "{routines:?}");
        assert!(routines[0].t_states * 100 / total > 70, "{routines:?}");
        let quick = routines.iter().find(|r| r.name == "quick").unwrap();
        assert!(routines[0].entered.abs_diff(quick.entered) <= 1, "each called as often as the other, give or take where counting stopped");
        assert!(quick.entered > 100);
        assert_eq!(lines[0].line, 5, "the DJNZ is the busiest line");
        // DJNZ taken is 13 T-states (and 8 the last time), so 200 times round costs about 2600.
        let per_call = routines[0].t_states / routines[0].entered;
        assert!((2600..=2900).contains(&per_call), "{per_call}");
    }
}
