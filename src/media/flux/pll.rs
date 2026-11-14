//! From flux to bit cells. A flux image records only when the drive's head saw the magnetic field change: a list of
//! times between changes. The disk was written as a stream of equal bit cells, a change in a cell being a 1 and no
//! change a 0, so the times are whole numbers of cells apart, near enough. "Near enough" is the whole difficulty:
//! the disk spun at a slightly different speed when written and read, the motor wobbles, and bits written close
//! together push each other apart (peak shift). A phase-locked loop recovers the cells as a disk controller's data
//! separator does: it keeps its own clock, counts how many of its cells fit in each gap, and nudges the clock's
//! period and phase towards each change it sees, so it follows slow drift without being thrown by one late bit.

/// How hard the loop pulls towards each change it sees.
#[derive(Clone, Copy, Debug)]
pub struct Loop {
    /// The share of each phase error that adjusts the clock's period.
    pub period: f64,
    /// The share of each phase error taken out at once by moving the cell window.
    pub phase: f64,
}

/// The usual loop: it locks on quickly and follows a long or uneven track (Keir Fraser's Greaseweazle uses the same
/// strength by default).
pub const QUICK: Loop = Loop { period: 0.05, phase: 0.6 };
/// A steadier loop for a second try: slower to follow, so a little noise from a dirty or mouldy disk doesn't throw it.
pub const STEADY: Loop = Loop { period: 0.01, phase: 0.1 };

/// The commonest shortest gap between changes, in nanoseconds: one bit cell for FM and GCR, two for MFM, whose rules
/// never allow two changes in neighbouring cells. Found from a histogram of the gaps, as the first peak that holds a
/// fair share of them; gaps under half a microsecond are noise, not data. None when there's no such peak.
pub fn shortest_peak(gaps: &[f32]) -> Option<f64> {
    const BIN: f64 = 25.0;
    const BINS: usize = 640; // to 16 microseconds
    let mut counts = vec![0u32; BINS];
    let mut total = 0u32;
    for &gap in gaps {
        let bin = (f64::from(gap) / BIN) as usize;
        if (20..BINS).contains(&bin) {
            counts[bin] += 1;
            total += 1;
        }
    }
    if total < 200 {
        return None;
    }
    // Each bin's share is taken over the gaps within an eighth of it either side, as jitter spreads a peak wider the
    // longer the gap.
    let mut sums = vec![0u32; BINS + 1];
    for i in 0..BINS {
        sums[i + 1] = sums[i] + counts[i];
    }
    let span = |i: usize| ((i as f64 * 0.88) as usize, ((i as f64 * 1.12) as usize + 1).min(BINS));
    let mass = |i: usize| {
        let (from, to) = span(i);
        sums[to] - sums[from]
    };
    let floor = total / 25;
    let peak = (21..BINS - 1).find(|&i| {
        let here = mass(i);
        // A peak, not the rising edge of one: nothing larger just after it.
        here >= floor && here >= mass(i - 1) && (i + 1..(i + i / 8 + 2).min(BINS)).all(|j| mass(j) <= here)
    })?;
    // The peak's centre, weighted over the gaps around it.
    let (from, to) = span(peak);
    let (sum, weight) = (from..to).fold((0.0, 0.0), |(s, w), i| (s + (i as f64 + 0.5) * BIN * f64::from(counts[i]), w + f64::from(counts[i])));
    (weight > 0.0).then(|| sum / weight)
}

/// The gaps with very short ones merged away: some drives add a spurious pair of changes in the middle of a long
/// gap, which a 1541's own electronics would ignore. A short gap is merged with its neighbours: long, short, long
/// becomes one long gap.
pub fn low_pass(gaps: &[f32], threshold: f32) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::with_capacity(gaps.len());
    let mut i = 0;
    while i < gaps.len() {
        let gap = gaps[i];
        i += 1;
        if gap > threshold {
            out.push(gap);
            continue;
        }
        let next = gaps.get(i).copied().unwrap_or(0.0);
        i += 1;
        if next <= gap {
            let after = gaps.get(i).copied().unwrap_or(0.0);
            i += 1;
            out.push(gap + next + after);
        } else if let Some(last) = out.last_mut() {
            *last += gap + next;
        } else {
            out.push(gap + next);
        }
    }
    out
}

/// The bit cells the gaps make, one byte (0 or 1) to a cell, with the loop's clock starting at `cell` nanoseconds
/// and allowed to drift a tenth either way of it.
pub fn cells(gaps: &[f32], cell: f64, pull: Loop) -> Vec<u8> {
    let (low, high) = (cell * 0.9, cell * 1.1);
    let mut clock = cell;
    let mut out = Vec::with_capacity((gaps.iter().map(|g| f64::from(*g)).sum::<f64>() / cell) as usize + 16);
    // How far past the middle of the cell the loop expects the next change, carried over from the last one.
    let mut time = 0.0f64;
    for &gap in gaps {
        time += f64::from(gap);
        // A change less than half a cell after the last is taken together with the next one, as noise.
        if time < clock / 2.0 {
            continue;
        }
        let mut zeros = 0usize;
        loop {
            time -= clock;
            if time < clock / 2.0 {
                break;
            }
            zeros += 1;
            out.push(0);
            // A long stretch with no changes is unformatted or damaged; a cap keeps a bad gap from filling memory.
            if zeros > 4096 {
                time = 0.0;
                break;
            }
        }
        out.push(1);
        // The phase error is `time`: how far the change was from the middle of the cell the loop gave it.
        if zeros <= 3 {
            clock += time * pull.period;
        } else {
            // Out of step after a long run without changes: the clock goes back towards where it started.
            clock += (cell - clock) * pull.period;
        }
        clock = clock.clamp(low, high);
        time *= 1.0 - pull.phase;
    }
    out
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Gaps for a run of cells, each a multiple of `cell`, with slow drift and a little jitter.
    pub fn gaps_of(cells: &[u8], cell: f64, drift: f64, jitter: f64) -> Vec<f32> {
        let mut out = Vec::new();
        let mut since = 0.0;
        let mut state = 12345u32;
        for (n, &c) in cells.iter().enumerate() {
            // The speed swings slowly, `drift` either way, over a few thousand cells.
            let speed = 1.0 + drift * ((n as f64) / 3000.0).sin();
            since += cell * speed;
            if c == 1 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let noise = (f64::from((state >> 16) & 0x7FFF) / 32768.0 - 0.5) * 2.0 * jitter * cell;
                out.push((since + noise) as f32);
                since = -noise;
            }
        }
        out
    }

    #[test]
    fn the_loop_recovers_cells_through_drift_and_jitter() {
        let mut state = 7u32;
        // MFM-like cells: a 1 then one to three 0s.
        let mut pattern = Vec::new();
        for _ in 0..20_000 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            pattern.push(1);
            pattern.extend(std::iter::repeat_n(0, 1 + (state >> 16) as usize % 3));
        }
        let gaps = gaps_of(&pattern, 2000.0, 0.04, 0.12);
        let peak = shortest_peak(&gaps).unwrap();
        assert!((peak - 4000.0).abs() < 200.0, "{peak}");
        let got = cells(&gaps, peak / 2.0, QUICK);
        // The first cell is lost to the gap before the first change; after that, every cell matches.
        assert_eq!(&got[..], &pattern[..got.len()]);
        assert!(got.len() + 4 >= pattern.len());
    }
}
