//! CPython's `random.Random(seed).shuffle(...)`, bit for bit.
//!
//! Blinded packets are shuffled with a seed so that a run can be replayed and audited. The Python
//! reference uses the Mersenne Twister behind `random.Random`; to read a packet in the same order
//! from either implementation, the generator, its seeding from an integer and the shuffle are
//! reproduced here.

const N: usize = 624;
const M: usize = 397;

/// The Mersenne Twister (MT19937), seeded the way CPython seeds it from an integer.
pub struct PyRandom {
    mt: [u32; N],
    index: usize,
}

impl PyRandom {
    /// `random.Random(seed)` for a non-negative integer seed.
    pub fn new(seed: u64) -> Self {
        let mut key = vec![seed as u32];
        if seed >> 32 != 0 {
            key.push((seed >> 32) as u32);
        }
        let mut rng = PyRandom {
            mt: [0; N],
            index: N,
        };
        rng.init_genrand(19_650_218);
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..N.max(key.len()) {
            let prev = rng.mt[i - 1];
            rng.mt[i] = (rng.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                rng.mt[0] = rng.mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            let prev = rng.mt[i - 1];
            rng.mt[i] = (rng.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_566_083_941))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                rng.mt[0] = rng.mt[N - 1];
                i = 1;
            }
        }
        rng.mt[0] = 0x8000_0000;
        rng
    }

    fn init_genrand(&mut self, seed: u32) {
        self.mt[0] = seed;
        for i in 1..N {
            let prev = self.mt[i - 1];
            self.mt[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        self.index = N;
    }

    /// The next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            for k in 0..N {
                let y = (self.mt[k] & 0x8000_0000) | (self.mt[(k + 1) % N] & 0x7fff_ffff);
                let mag = if y & 1 == 1 { 0x9908_b0df } else { 0 };
                self.mt[k] = self.mt[(k + M) % N] ^ (y >> 1) ^ mag;
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `random._randbelow(n)`: a number in `0..n`, by rejection on `n.bit_length()` bits.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let bits = 32 - n.leading_zeros();
        loop {
            let r = self.next_u32() >> (32 - bits);
            if r < n {
                return r;
            }
        }
    }

    /// `random.shuffle(items)`.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// `order = list(range(n)); random.Random(seed).shuffle(order)`.
pub fn shuffled(n: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    PyRandom::new(seed).shuffle(&mut order);
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generator_matches_cpython() {
        let mut r = PyRandom::new(12345);
        let got: Vec<u32> = (0..4).map(|_| r.next_u32()).collect();
        assert_eq!(
            got,
            [1_789_368_711, 3_146_859_322, 43_676_229, 3_522_623_596]
        );
    }

    #[test]
    fn shuffles_match_cpython_for_small_large_and_two_word_seeds() {
        assert_eq!(shuffled(1, 0), [0]);
        assert_eq!(shuffled(2, 0), [0, 1]);
        assert_eq!(shuffled(5, 0), [2, 1, 0, 4, 3]);
        assert_eq!(shuffled(7, 0), [4, 2, 1, 0, 5, 3, 6]);
        assert_eq!(shuffled(7, 1), [3, 6, 5, 2, 0, 4, 1]);
        assert_eq!(shuffled(7, 42), [1, 3, 4, 2, 6, 0, 5]);
        assert_eq!(shuffled(5, 3_735_928_559), [4, 2, 1, 3, 0]);
        assert_eq!(shuffled(7, (1 << 32) + 5), [4, 5, 6, 0, 2, 3, 1]);
        assert_eq!(shuffled(7, (1 << 63) + 11), [5, 0, 1, 3, 6, 4, 2]);
        assert_eq!(
            shuffled(26, 42),
            [
                16, 12, 9, 25, 19, 6, 5, 10, 15, 22, 11, 18, 1, 14, 23, 13, 2, 17, 24, 4, 21, 7, 8,
                0, 3, 20
            ]
        );
    }
}
