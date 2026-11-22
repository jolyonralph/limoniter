//! MD5 (RFC 1321), which PostgreSQL's older "md5" logins still use. It's far too weak to keep anything safe on its
//! own now, but a server that asks for it has to be answered, and it's short enough to write here rather than bring
//! in a crate for it.

/// How far each step turns its word left, round by round.
const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// The digest of some bytes: sixteen bytes.
pub fn digest(input: &[u8]) -> [u8; 16] {
    // The constants are the whole parts of abs(sin(i + 1)) times 2^32, as the RFC gives them.
    let k: [u32; 64] = std::array::from_fn(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32);
    let mut state: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    // The message is padded with a 1 bit, then 0 bits up to 56 bytes in the last block, then its length in bits.
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((input.len() as u64).wrapping_mul(8)).to_le_bytes());
    for block in message.as_chunks::<64>().0 {
        let words: [u32; 16] = std::array::from_fn(|i| u32::from_le_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]));
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let turned = a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(words[g]).rotate_left(SHIFTS[i]);
            (a, d, c) = (d, c, b);
            b = b.wrapping_add(turned);
        }
        for (kept, new) in state.iter_mut().zip([a, b, c, d]) {
            *kept = kept.wrapping_add(new);
        }
    }
    let mut out = [0u8; 16];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// The digest as 32 lower-case hexadecimal digits, as PostgreSQL wants it.
pub fn hex(input: &[u8]) -> String {
    digest(input).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_1321s_test_suite_gives_its_digests() {
        // The test suite at the end of RFC 1321.
        let cases = [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            ("abcdefghijklmnopqrstuvwxyz", "c3fcd3d76192e4007dfb496cca67e13b"),
            ("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789", "d174ab98d277d9f5a5611c2c9f419d9f"),
            ("12345678901234567890123456789012345678901234567890123456789012345678901234567890", "57edf4a22be3c955ac49da2e2107b67a"),
        ];
        for (input, expected) in cases {
            assert_eq!(hex(input.as_bytes()), expected, "MD5 of {input:?}");
        }
    }

    #[test]
    fn messages_either_side_of_a_block_boundary_are_padded_rightly() {
        // 55 bytes fit their length in the same block; 56 need another.
        assert_eq!(hex(&[b'a'; 55]), "ef1772b6dff9a122358552954ad0df65");
        assert_eq!(hex(&[b'a'; 56]), "3b0c8ac703f828b04c6c197006d17218");
        assert_eq!(hex(&[b'a'; 64]), "014842d480b571495a4a0363793f7367");
    }
}
