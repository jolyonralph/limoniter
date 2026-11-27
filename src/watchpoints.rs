//! Watchpoints and hit counts, shared by every debugger: the built-in Spectrum, the Atari 2600, the Vectrex, and
//! the emulators Limoniter talks to (ZEsarUX, VICE, and the Amiga's).
//!
//! A watchpoint stops the program when it reads or writes an address (or a run of them), as 8, 16 or 32 bits,
//! seen through a mask, when the value meets a rule: any, equals N, differs from N, changed, rises or falls
//! (compared with what it was). The list the person edits is `Watchpoint`s, kept with the source's marks; a
//! machine is given them worked out as `Resolved` (addresses and values as numbers) and a `Watcher` that judges
//! each access it reports. Every machine uses the same judging, so a rule means the same everywhere.
//!
//! A breakpoint may also have a `HitRule`: stop on the Nth time it is reached, on every Nth time, or never but
//! count (shown beside it). Its condition (an expression: see `debug_panel`) is tested first.

use std::collections::{BTreeMap, HashMap};

/// What a watchpoint reacts to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Access {
    Read,
    #[default]
    Write,
    Either,
}

impl Access {
    pub const ALL: [Access; 3] = [Access::Write, Access::Read, Access::Either];

    pub fn label(self) -> String {
        match self {
            Access::Read => crate::i18n::tr("Read"),
            Access::Write => crate::i18n::tr("Write"),
            Access::Either => crate::i18n::tr("Read or write"),
        }
        .to_string()
    }

    /// Whether a watch for this kind of access cares about an access of `what` (which is Read or Write).
    pub fn covers(self, what: Access) -> bool {
        self == Access::Either || self == what
    }
}

/// What the value must do for a watchpoint to stop the program.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuleKind {
    #[default]
    Any,
    Equals,
    Differs,
    Changed,
    Rises,
    Falls,
}

impl RuleKind {
    pub const ALL: [RuleKind; 6] = [RuleKind::Any, RuleKind::Equals, RuleKind::Differs, RuleKind::Changed, RuleKind::Rises, RuleKind::Falls];

    pub fn label(self) -> String {
        match self {
            RuleKind::Any => crate::i18n::tr("any value"),
            RuleKind::Equals => crate::i18n::tr("equals"),
            RuleKind::Differs => crate::i18n::tr("differs from"),
            RuleKind::Changed => crate::i18n::tr("changed"),
            RuleKind::Rises => crate::i18n::tr("rises"),
            RuleKind::Falls => crate::i18n::tr("falls"),
        }
        .to_string()
    }

    /// Whether the rule needs a number to compare with.
    pub fn takes_value(self) -> bool {
        matches!(self, RuleKind::Equals | RuleKind::Differs)
    }
}

/// A watchpoint as the person writes it: expressions as text, so labels and `$8000` work and are kept as typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watchpoint {
    /// An address or a label, or a range as `from..to`.
    pub address: String,
    pub access: Access,
    /// 8, 16 or 32.
    pub bits: u8,
    /// Which bits of the value count, as an expression; empty for all.
    pub mask: String,
    pub rule: RuleKind,
    /// What `Equals` and `Differs` compare with, as an expression.
    pub value: String,
    pub enabled: bool,
}

impl Default for Watchpoint {
    fn default() -> Self {
        Watchpoint { address: String::new(), access: Access::Write, bits: 8, mask: String::new(), rule: RuleKind::Any, value: String::new(), enabled: true }
    }
}

impl Watchpoint {
    /// What marks kept before watchpoints had kinds held: an address or range whose writing stops the program.
    pub fn from_old(address: &str) -> Watchpoint {
        Watchpoint { address: address.trim().to_string(), ..Watchpoint::default() }
    }
}

/// A rule with its number worked out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Any,
    Equals(u32),
    Differs(u32),
    Changed,
    Rises,
    Falls,
}

/// A watchpoint with its addresses and numbers worked out, as a machine is given it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// Its place in the person's list, so a hit can say which it was.
    pub id: usize,
    pub start: u32,
    /// The last address it covers.
    pub end: u32,
    pub access: Access,
    /// The width in bytes: 1, 2 or 4.
    pub bytes: u32,
    pub mask: u32,
    pub rule: Rule,
}

impl Resolved {
    /// A plain watch on writes to one address, as the older write watches were; for the tests.
    #[cfg(test)]
    pub fn write_to(id: usize, address: u32) -> Resolved {
        Resolved { id, start: address, end: address, access: Access::Write, bytes: 1, mask: 0xFF, rule: Rule::Any }
    }

    fn covers(&self, address: u32) -> bool {
        (self.start..=self.end).contains(&address)
    }

    /// The first address of the unit of `bytes` that holds `address`.
    fn unit(&self, address: u32) -> u32 {
        self.start + (address - self.start) / self.bytes * self.bytes
    }
}

/// Works out the person's watchpoints (and the older write watches) into what a machine is given. `eval` works an
/// expression out, with the machine's labels. A watchpoint that is off, or can't be worked out, is left out.
pub fn resolve(list: &[Watchpoint], old_writes: &[String], eval: &dyn Fn(&str) -> Option<i64>) -> Vec<Resolved> {
    let mut found = Vec::new();
    let everything: Vec<(usize, Watchpoint)> = old_writes.iter().map(|w| Watchpoint::from_old(w)).chain(list.iter().cloned()).enumerate().collect();
    for (id, w) in everything {
        if !w.enabled {
            continue;
        }
        let bytes = u32::from(w.bits.clamp(8, 32) / 8).next_power_of_two().min(4);
        let (start, end) = match w.address.split_once("..") {
            Some((from, to)) => match (eval(from), eval(to)) {
                (Some(from), Some(to)) if to >= from => (from as u32, (to as u32).min((from as u32).saturating_add(0x4000))),
                _ => continue,
            },
            None => match eval(&w.address) {
                Some(a) => (a as u32, (a as u32).saturating_add(bytes - 1)),
                None => continue,
            },
        };
        let all = if bytes >= 4 { u32::MAX } else { (1u32 << (bytes * 8)) - 1 };
        let mask = if w.mask.trim().is_empty() { all } else { eval(&w.mask).map_or(all, |m| m as u32 & all) };
        let number = |text: &str| eval(text).map(|v| v as u32 & mask);
        let rule = match w.rule {
            RuleKind::Any => Rule::Any,
            RuleKind::Equals => match number(&w.value) {
                Some(v) => Rule::Equals(v),
                None => continue,
            },
            RuleKind::Differs => match number(&w.value) {
                Some(v) => Rule::Differs(v),
                None => continue,
            },
            RuleKind::Changed => Rule::Changed,
            RuleKind::Rises => Rule::Rises,
            RuleKind::Falls => Rule::Falls,
        };
        found.push(Resolved { id, start, end, access: w.access, bytes, mask, rule });
    }
    found
}

/// The accesses to watched places, noted as a machine runs (a cheap test on its hot path) for it to judge when the
/// instruction is over.
#[derive(Default)]
pub struct Touches {
    watched: Vec<(u32, u32, Access)>,
    events: Vec<(Access, u32, u8)>,
}

impl Touches {
    pub fn set(&mut self, list: &[Resolved]) {
        self.watched = list.iter().map(|w| (w.start, w.end, w.access)).collect();
    }

    /// Notes an access (`what` is Read or Write) if anything watches it.
    #[inline]
    pub fn note(&mut self, what: Access, address: u32, value: u8) {
        if !self.watched.is_empty() && self.events.len() < 64 && self.watched.iter().any(|(from, to, access)| access.covers(what) && (*from..=*to).contains(&address)) {
            self.events.push((what, address, value));
        }
    }

    pub fn take(&mut self) -> Vec<(Access, u32, u8)> {
        std::mem::take(&mut self.events)
    }
}

/// What a watchpoint saw: which, where, how it was reached, and the value before and after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub id: usize,
    pub address: u32,
    pub access: Access,
    pub bytes: u32,
    pub old: u32,
    pub new: u32,
}

impl Hit {
    /// What to tell the person: which watchpoint stopped it, at what instruction (`pc`), and the values.
    /// `digits` is how many hexadecimal digits an address is written with.
    pub fn reason(&self, pc: Option<u32>, digits: usize) -> String {
        let n = self.id + 1;
        let (address, bits) = (self.address, self.bytes * 8);
        let (old, new) = (self.old, self.new);
        let width = (self.bytes * 2) as usize;
        let at = pc.map(|pc| crate::i18n::tf!(" by the instruction at ${pc:0digits$X}")).unwrap_or_default();
        match self.access {
            Access::Read => crate::i18n::tf!("Stopped by watchpoint {n}: read of ${address:0digits$X} ({bits} bits){at}, reading ${new:0width$X} (it was ${old:0width$X})"),
            _ => crate::i18n::tf!("Stopped by watchpoint {n}: write to ${address:0digits$X} ({bits} bits){at}, ${old:0width$X} becoming ${new:0width$X}"),
        }
    }
}

/// Judges the accesses a machine reports against its watchpoints, remembering each watched value to compare with.
#[derive(Default)]
pub struct Watcher {
    list: Vec<Resolved>,
    last: HashMap<(usize, u32), u32>,
    /// Whether a word is held most significant byte first, as on the 68000 and 6809.
    pub big_endian: bool,
}

impl Watcher {
    /// Makes the list what's wanted. When it's the same as before, what has been remembered is kept; when not, the
    /// values are taken afresh from `peek`.
    pub fn set(&mut self, list: Vec<Resolved>, peek: &dyn Fn(u32) -> u8) {
        if list == self.list {
            return;
        }
        self.list = list;
        self.last.clear();
        for w in &self.list {
            let mut unit = w.start;
            while unit <= w.end && unit - w.start < 0x4000 {
                let value = Self::compose(w.bytes, unit, None, self.big_endian, peek) & w.mask;
                self.last.insert((w.id, unit), value);
                unit = unit.saturating_add(w.bytes);
                if unit == u32::MAX {
                    break;
                }
            }
        }
    }

    fn compose(bytes: u32, unit: u32, touched: Option<(u32, u8)>, big_endian: bool, peek: &dyn Fn(u32) -> u8) -> u32 {
        let mut value = 0u32;
        for i in 0..bytes {
            let at = unit.wrapping_add(i);
            let byte = match touched {
                Some((a, v)) if a == at => v,
                _ => peek(at),
            };
            value = if big_endian { value << 8 | u32::from(byte) } else { value | u32::from(byte) << (8 * i) };
        }
        value
    }

    /// An access to `address` (`what` is Read or Write; `value` is the byte read or written, and `peek` the rest of
    /// memory as it is). The first watchpoint it makes stop, if any; every watchpoint it touches remembers the
    /// value for next time.
    pub fn judge(&mut self, what: Access, address: u32, value: u8, peek: &dyn Fn(u32) -> u8) -> Option<Hit> {
        let mut found = None;
        for w in &self.list {
            if !w.access.covers(what) || !w.covers(address) {
                continue;
            }
            let unit = w.unit(address);
            let new = Self::compose(w.bytes, unit, Some((address, value)), self.big_endian, peek) & w.mask;
            let old = self.last.insert((w.id, unit), new).unwrap_or(new);
            let stops = match w.rule {
                Rule::Any => true,
                Rule::Equals(v) => new == v,
                Rule::Differs(v) => new != v,
                Rule::Changed => new != old,
                Rule::Rises => new > old,
                Rule::Falls => new < old,
            };
            if stops && found.is_none() {
                found = Some(Hit { id: w.id, address: unit, access: what, bytes: w.bytes, old, new });
            }
        }
        found
    }
}

/// For an emulator that only says it stopped at a watched place, not what was read or written: judges the
/// watchpoints from memory as it was at the stop before and as it is now. A unit whose value changed was written;
/// one that didn't may have been read (or written with the same value). The old value is the one at the last stop,
/// so "changed", "rises" and "falls" mean since then. None when no watchpoint's rule is met.
pub fn judge_between(list: &[Resolved], before: &dyn Fn(u32) -> Option<u8>, now: &dyn Fn(u32) -> Option<u8>, big_endian: bool) -> Option<Hit> {
    let value = |bytes: u32, unit: u32, peek: &dyn Fn(u32) -> Option<u8>| -> Option<u32> {
        let mut found = 0u32;
        for i in 0..bytes {
            let byte = u32::from(peek(unit.wrapping_add(i))?);
            found = if big_endian { found << 8 | byte } else { found | byte << (8 * i) };
        }
        Some(found)
    };
    for changed_only in [true, false] {
        for w in list {
            let mut unit = w.start;
            while unit <= w.end && unit - w.start < 0x4000 {
                if let (Some(old), Some(new)) = (value(w.bytes, unit, before), value(w.bytes, unit, now)) {
                    let (old, new) = (old & w.mask, new & w.mask);
                    let access = if old != new || w.access == Access::Write { Access::Write } else { Access::Read };
                    let met = match w.rule {
                        Rule::Any => true,
                        Rule::Equals(v) => new == v,
                        Rule::Differs(v) => new != v,
                        Rule::Changed => new != old,
                        Rule::Rises => new > old,
                        Rule::Falls => new < old,
                    };
                    if met && (old != new || !changed_only) && w.access.covers(access) {
                        return Some(Hit { id: w.id, address: unit, access, bytes: w.bytes, old, new });
                    }
                }
                unit = unit.saturating_add(w.bytes);
            }
        }
    }
    None
}

/// What to do about a breakpoint that has been reached, beyond its condition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HitRule {
    /// Stop each time.
    #[default]
    Always,
    /// Stop on the Nth time only.
    Nth(u32),
    /// Stop on every Nth time.
    Every(u32),
    /// Never stop; only count.
    Counter,
}

impl HitRule {
    /// Counts one more time the breakpoint was reached (with its condition true), and says whether to stop now.
    pub fn reached(self, count: &mut u64) -> bool {
        *count += 1;
        match self {
            HitRule::Always => true,
            HitRule::Nth(n) => *count == u64::from(n.max(1)),
            HitRule::Every(n) => count.is_multiple_of(u64::from(n.max(1))),
            HitRule::Counter => false,
        }
    }

    pub fn is_counting(self) -> bool {
        self != HitRule::Always
    }
}

/// The hit rules by address, from those by line.
pub fn hit_rules_by_address(rules: &BTreeMap<usize, HitRule>, address_of: &dyn Fn(usize) -> Option<u16>) -> BTreeMap<u16, HitRule> {
    rules.iter().filter(|(_, r)| r.is_counting()).filter_map(|(line, rule)| address_of(*line).map(|a| (a, *rule))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: u32, access: Access, bytes: u32, mask: u32, rule: Rule) -> Resolved {
        Resolved { id: 0, start, end: start + bytes - 1, access, bytes, mask, rule }
    }

    fn memory() -> std::cell::RefCell<Vec<u8>> {
        std::cell::RefCell::new(vec![0; 0x10000])
    }

    #[test]
    fn rules_compare_with_what_the_value_was() {
        let m = memory();
        let peek = |a: u32| m.borrow()[a as usize];
        let write = |w: &mut Watcher, v: u8| {
            m.borrow_mut()[0x8000] = v;
            w.judge(Access::Write, 0x8000, v, &peek)
        };
        for (rule, writes, stops) in [
            (Rule::Any, [5u8, 5, 5], [true, true, true]),
            (Rule::Equals(5), [4, 5, 6], [false, true, false]),
            (Rule::Differs(5), [4, 5, 6], [true, false, true]),
            (Rule::Changed, [0, 7, 7], [false, true, false]),
            (Rule::Rises, [3, 2, 9], [true, false, true]),
            (Rule::Falls, [3, 2, 9], [false, true, false]),
        ] {
            m.borrow_mut()[0x8000] = 0;
            let mut w = Watcher::default();
            w.set(vec![at(0x8000, Access::Write, 1, 0xFF, rule)], &peek);
            let got: Vec<bool> = writes.iter().map(|v| write(&mut w, *v).is_some()).collect();
            assert_eq!(got, stops, "{rule:?}");
        }
    }

    #[test]
    fn a_stop_is_judged_from_memory_before_and_after() {
        let (mut before, mut now) = (vec![0u8; 0x100], vec![0u8; 0x100]);
        before[0x10] = 5;
        now[0x10] = 9;
        let (b, n) = (|a: u32| before.get(a as usize).copied(), |a: u32| now.get(a as usize).copied());
        let list = vec![at(0x10, Access::Either, 1, 0xFF, Rule::Rises), Resolved { id: 1, ..at(0x20, Access::Read, 1, 0xFF, Rule::Any) }];
        let hit = judge_between(&list, &b, &n, false).unwrap();
        assert_eq!((hit.id, hit.old, hit.new, hit.access), (0, 5, 9, Access::Write));
        let falls = vec![at(0x10, Access::Either, 1, 0xFF, Rule::Falls), Resolved { id: 1, ..at(0x20, Access::Read, 1, 0xFF, Rule::Any) }];
        let hit = judge_between(&falls, &b, &n, false).unwrap();
        assert_eq!((hit.id, hit.address, hit.access), (1, 0x20, Access::Read), "the rise isn't one it wants, so the read of the other is");
        assert_eq!(judge_between(&falls[..1], &b, &n, false), None, "nothing it wants happened: carry on");
    }

    #[test]
    fn a_hit_has_the_old_and_new_values_and_the_kind_of_access() {
        let m = memory();
        let peek = |a: u32| m.borrow()[a as usize];
        m.borrow_mut()[0x8000] = 9;
        let mut w = Watcher::default();
        w.set(vec![at(0x8000, Access::Either, 1, 0xFF, Rule::Changed)], &peek);
        m.borrow_mut()[0x8000] = 12;
        let hit = w.judge(Access::Write, 0x8000, 12, &peek).unwrap();
        assert_eq!((hit.old, hit.new, hit.access), (9, 12, Access::Write));
        assert_eq!(w.judge(Access::Read, 0x8000, 12, &peek), None, "a read of an unchanged value isn't a change");
        assert!(hit.reason(Some(0x8003), 4).contains("$09 becoming $0C"), "{}", hit.reason(Some(0x8003), 4));
    }

    #[test]
    fn access_kinds_are_told_apart() {
        let m = memory();
        let peek = |a: u32| m.borrow()[a as usize];
        for (access, read, write) in [(Access::Read, true, false), (Access::Write, false, true), (Access::Either, true, true)] {
            let mut w = Watcher::default();
            w.set(vec![at(0x10, access, 1, 0xFF, Rule::Any)], &peek);
            assert_eq!((w.judge(Access::Read, 0x10, 0, &peek).is_some(), w.judge(Access::Write, 0x10, 0, &peek).is_some()), (read, write), "{access:?}");
        }
    }

    #[test]
    fn widths_join_bytes_in_the_machines_order() {
        let m = memory();
        m.borrow_mut()[0x2000..0x2004].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        let peek = |a: u32| m.borrow()[a as usize];
        for (big, word, long) in [(false, 0x3412, 0x7856_3412), (true, 0x1234, 0x1234_5678)] {
            let mut w = Watcher { big_endian: big, ..Watcher::default() };
            w.set(vec![at(0x2000, Access::Read, 2, 0xFFFF, Rule::Any)], &peek);
            assert_eq!(w.judge(Access::Read, 0x2001, 0x34, &peek).unwrap().new, word);
            w.set(vec![at(0x2000, Access::Read, 4, u32::MAX, Rule::Any)], &peek);
            assert_eq!(w.judge(Access::Read, 0x2003, 0x78, &peek).unwrap().new, long);
            assert_eq!(w.judge(Access::Read, 0x2004, 0, &peek), None, "past the end");
        }
    }

    #[test]
    fn a_mask_hides_the_bits_it_leaves_out() {
        let m = memory();
        let peek = |a: u32| m.borrow()[a as usize];
        let mut w = Watcher::default();
        w.set(vec![at(0x40, Access::Write, 1, 0x0F, Rule::Changed)], &peek);
        m.borrow_mut()[0x40] = 0xF0;
        assert_eq!(w.judge(Access::Write, 0x40, 0xF0, &peek), None, "only the high bits changed");
        m.borrow_mut()[0x40] = 0xF3;
        let hit = w.judge(Access::Write, 0x40, 0xF3, &peek).unwrap();
        assert_eq!((hit.old, hit.new), (0, 3));
    }

    #[test]
    fn ranges_judge_each_unit_by_itself() {
        let m = memory();
        let peek = |a: u32| m.borrow()[a as usize];
        let mut w = Watcher::default();
        w.set(vec![Resolved { id: 3, start: 0x100, end: 0x107, access: Access::Write, bytes: 2, mask: 0xFFFF, rule: Rule::Rises }], &peek);
        m.borrow_mut()[0x105] = 1;
        let hit = w.judge(Access::Write, 0x105, 1, &peek).unwrap();
        assert_eq!((hit.id, hit.address, hit.new), (3, 0x104, 0x0100));
        m.borrow_mut()[0x105] = 0;
        assert_eq!(w.judge(Access::Write, 0x105, 0, &peek), None, "it fell");
    }

    #[test]
    fn watchpoints_are_worked_out_from_what_is_typed() {
        let eval = |t: &str| match t.trim() {
            "count" => Some(0x8000),
            "total" => Some(0x8010),
            other => other.strip_prefix('$').and_then(|h| i64::from_str_radix(h, 16).ok()),
        };
        let list = vec![
            Watchpoint { address: "count".into(), access: Access::Read, bits: 16, mask: "$F0F0".into(), rule: RuleKind::Equals, value: "$1234".into(), ..Watchpoint::default() },
            Watchpoint { address: "count..total".into(), enabled: false, ..Watchpoint::default() },
            Watchpoint { address: "nowhere".into(), ..Watchpoint::default() },
            Watchpoint { address: "total".into(), bits: 32, rule: RuleKind::Falls, ..Watchpoint::default() },
        ];
        let found = resolve(&list, &["$40..$42".to_string()], &eval);
        assert_eq!(found.len(), 3);
        assert_eq!((found[0].start, found[0].end, found[0].bytes, found[0].access), (0x40, 0x42, 1, Access::Write), "an older write watch comes first");
        assert_eq!((found[1].start, found[1].end, found[1].bytes, found[1].mask, found[1].rule), (0x8000, 0x8001, 2, 0xF0F0, Rule::Equals(0x1030)), "the value is masked too");
        assert_eq!((found[2].bytes, found[2].mask, found[2].rule), (4, u32::MAX, Rule::Falls));
    }

    #[test]
    fn older_write_watches_become_watchpoints() {
        let old = Watchpoint::from_old(" $8000..$8003 ");
        assert_eq!((old.address.as_str(), old.access, old.bits, old.rule, old.enabled), ("$8000..$8003", Access::Write, 8, RuleKind::Any, true));
    }

    #[test]
    fn hit_rules_stop_on_the_nth_or_every_nth_or_count_only() {
        let run = |rule: HitRule| {
            let mut count = 0;
            let stops: Vec<bool> = (0..6).map(|_| rule.reached(&mut count)).collect();
            (stops, count)
        };
        assert_eq!(run(HitRule::Always).0, [true; 6]);
        assert_eq!(run(HitRule::Nth(3)).0, [false, false, true, false, false, false]);
        assert_eq!(run(HitRule::Every(2)).0, [false, true, false, true, false, true]);
        assert_eq!(run(HitRule::Counter), ([false; 6].to_vec(), 6));
    }
}
