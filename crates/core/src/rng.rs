//! Seeds and the random stream. Every choice of a period comes from `seed(draw_hash, n, label)`; the caller sorts
//! lists by id before picking.

use crate::sha256;
use hkdf::Hkdf;
use sha2::Sha256;

pub const SALT: &[u8] = b"venin-v1";

/// HKDF-SHA256 with salt `venin-v1`, 32 bytes out.
pub fn derive(ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(Some(SALT), ikm)
        .expand(info, &mut out)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

pub fn seed(draw_hash: &[u8; 32], n: u64, label: &str) -> [u8; 32] {
    let mut info = n.to_be_bytes().to_vec();
    info.extend_from_slice(label.as_bytes());
    derive(draw_hash, &info)
}

/// Bytes of `SHA-256(seed || i)` for `i = 0, 1, ...` (8 bytes big endian), in order.
pub struct Stream {
    seed: [u8; 32],
    counter: u64,
    block: [u8; 32],
    used: usize,
}

impl Stream {
    pub fn new(seed: [u8; 32]) -> Self {
        Self {
            seed,
            counter: 0,
            block: [0; 32],
            used: 32,
        }
    }

    pub fn byte(&mut self) -> u8 {
        if self.used == self.block.len() {
            let mut input = self.seed.to_vec();
            input.extend_from_slice(&self.counter.to_be_bytes());
            self.block = sha256(&input);
            self.counter += 1;
            self.used = 0;
        }
        self.used += 1;
        self.block[self.used - 1]
    }

    pub fn next_u64(&mut self) -> u64 {
        (0..8).fold(0, |acc, _| acc << 8 | u64::from(self.byte()))
    }

    /// A uniform value in `0..k`, by rejection below `2^64 mod k`.
    pub fn below(&mut self, k: u64) -> u64 {
        assert!(k > 0, "below(0) has no values");
        let floor = k.wrapping_neg() % k;
        loop {
            let x = self.next_u64();
            if x >= floor {
                return x % k;
            }
        }
    }

    fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.below(len as u64)).expect("an index below a usize length fits a usize")
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let i = self.index(items.len());
        items.get(i)
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.index(i + 1);
            items.swap(i, j);
        }
    }

    /// `len` lowercase hex characters, two per byte, high nibble first.
    pub fn hex(&mut self, len: usize) -> String {
        let bytes: Vec<u8> = (0..len.div_ceil(2)).map(|_| self.byte()).collect();
        let mut out = crate::hex(&bytes);
        out.truncate(len);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_matches_an_independent_hkdf() {
        // Python: hmac/hashlib HKDF-SHA256, salt b"venin-v1", ikm 32 zero bytes, info (7).to_bytes(8,"big")+b"strain"
        assert_eq!(
            crate::hex(&seed(&[0; 32], 7, "strain")),
            "10d96e67f339485a68f1ee90f39cb2da1d712e9716851f9cfbe2ac48ffd483be"
        );
    }

    #[test]
    fn stream_blocks_are_sha256_of_seed_and_counter() {
        let seed = [9u8; 32];
        let mut stream = Stream::new(seed);
        let first: Vec<u8> = (0..32).map(|_| stream.byte()).collect();
        let second: Vec<u8> = (0..32).map(|_| stream.byte()).collect();
        let mut input = seed.to_vec();
        input.extend_from_slice(&0u64.to_be_bytes());
        assert_eq!(first, sha256(&input));
        input.truncate(32);
        input.extend_from_slice(&1u64.to_be_bytes());
        assert_eq!(second, sha256(&input));
    }

    #[test]
    fn same_inputs_give_the_same_choices() {
        let a = seed(&[1; 32], 214, "benign:m1-none");
        assert_eq!(a, seed(&[1; 32], 214, "benign:m1-none"));
        assert_ne!(a, seed(&[1; 32], 215, "benign:m1-none"));
        assert_ne!(a, seed(&[1; 32], 214, "benign:m1-venin1"));
        assert_ne!(a, seed(&[2; 32], 214, "benign:m1-none"));

        let mut list: Vec<u32> = (0..50).collect();
        let mut again = list.clone();
        Stream::new(a).shuffle(&mut list);
        Stream::new(a).shuffle(&mut again);
        assert_eq!(list, again);
        let mut sorted = list.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(list, sorted);
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut stream = Stream::new([3; 32]);
        let mut seen = [0u32; 7];
        for _ in 0..7000 {
            seen[stream.below(7) as usize] += 1;
        }
        assert!(
            seen.iter().all(|&count| (800..1200).contains(&count)),
            "{seen:?}"
        );
        assert_eq!(stream.below(1), 0);
    }

    #[test]
    fn pick_and_hex() {
        let mut stream = Stream::new([4; 32]);
        assert_eq!(stream.pick::<u8>(&[]), None);
        assert_eq!(stream.pick(&["only"]), Some(&"only"));
        let h = stream.hex(41);
        assert_eq!(h.len(), 41);
        assert!(
            h.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
    }
}
