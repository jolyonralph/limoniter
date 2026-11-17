//! A Z80 with 64K of memory and nothing else, counting T-states (with no contention) and keeping every OUT: what
//! the beeper's player is heard from, and what the AY players are tested on.

use rustzx_z80::{Z80, Z80Bus};

pub struct Machine {
    pub cpu: Z80,
    pub bus: Bus,
}

pub struct Bus {
    pub memory: Vec<u8>,
    /// T-states so far.
    pub t: u64,
    /// Each OUT: when, the port, the value.
    pub outs: Vec<(u64, u16, u8)>,
    pub halted: bool,
}

impl Z80Bus for Bus {
    fn read_internal(&mut self, addr: u16) -> u8 {
        self.memory[usize::from(addr)]
    }
    fn write_internal(&mut self, addr: u16, data: u8) {
        self.memory[usize::from(addr)] = data;
    }
    fn wait_mreq(&mut self, _addr: u16, clk: usize) {
        self.t += clk as u64;
    }
    fn wait_no_mreq(&mut self, _addr: u16, clk: usize) {
        self.t += clk as u64;
    }
    fn wait_internal(&mut self, clk: usize) {
        self.t += clk as u64;
    }
    fn read_io(&mut self, _port: u16) -> u8 {
        self.t += 4;
        0xFF
    }
    fn write_io(&mut self, port: u16, data: u8) {
        // The value reaches the port partway through the cycle; when doesn't matter so long as it's always the same.
        self.outs.push((self.t, port, data));
        self.t += 4;
    }
    fn read_interrupt(&mut self) -> u8 {
        0xFF
    }
    fn reti(&mut self) {}
    fn halt(&mut self, halted: bool) {
        self.halted = halted;
    }
    fn int_active(&self) -> bool {
        false
    }
    fn nmi_active(&self) -> bool {
        false
    }
    fn pc_callback(&mut self, _addr: u16) {}
}

/// Where a call returns to, which stops the run.
const RETURN: u16 = 0x0000;

impl Machine {
    /// Code loaded at an address.
    pub fn with(code: &[u8], at: u16) -> Machine {
        let mut memory = vec![0u8; 0x10000];
        let at = usize::from(at);
        let end = (at + code.len()).min(0x10000);
        memory[at..end].copy_from_slice(&code[..end - at]);
        Machine { cpu: Z80::default(), bus: Bus { memory, t: 0, outs: Vec::new(), halted: false } }
    }

    /// Calls a routine with A and HL set, and runs until it returns, or until `most` T-states have gone by.
    /// Whether it returned.
    pub fn call(&mut self, address: u16, a: u8, hl: u16, most: u64) -> bool {
        let regs = &mut self.cpu.regs;
        regs.set_acc(a);
        regs.set_hl(hl);
        let sp = 0xFF00u16.wrapping_sub(2);
        regs.set_sp(sp);
        self.bus.memory[usize::from(sp)..usize::from(sp) + 2].copy_from_slice(&RETURN.to_le_bytes());
        regs.set_pc(address);
        self.bus.halted = false;
        let until = self.bus.t + most;
        while self.bus.t < until {
            if self.cpu.regs.get_pc() == RETURN {
                return true;
            }
            self.cpu.emulate(&mut self.bus);
            if self.bus.halted {
                return false;
            }
        }
        false
    }
}
