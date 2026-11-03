/// xorshift32. Seeded per run so a replay reproduces every roll.
#[derive(Clone)]
pub struct Rng(u32);

impl Rng {
    pub const fn new(seed: u32) -> Self {
        Rng(if seed == 0 { 0x9e37_79b9 } else { seed })
    }

    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    pub fn state(&self) -> u32 {
        self.0
    }
}
