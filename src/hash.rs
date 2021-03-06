use serde::{Deserialize, Serialize};
use core::fmt;

pub struct RollingHash {
    a: u16,
    b: u16,
    count: usize,
}

impl RollingHash {
    pub fn new() -> Self {
        RollingHash {
            a: 0,
            b: 0,
            count: 0,
        }
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn update(&mut self, input: &[u8]) {
        for x in input {
            self.add(*x);
        }
    }
    pub fn get(&self) -> u32 {
        (self.a as u32) | ((self.b as u32) << 16)
    }
    pub fn add(&mut self, x: u8) {
        self.a = self.a.wrapping_add((x.wrapping_add(31)) as u16);
        self.b = self.b.wrapping_add(self.a);
        self.count += 1;
    }
    pub fn sub(&mut self, x: u8) {
        let x2 = (x.wrapping_add(31)) as u16;
        self.a = self.a.wrapping_sub(x2);
        self.b = self.b.wrapping_sub((self.count * (x2 as usize)) as u16);
        self.count -= 1;
    }
}

#[derive(Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct Hash128([u8; 16]);

impl Hash128 {
    pub fn new_zero() -> Self{
        Self([0; 16])
    }
    pub fn new_from_blake3(hash: &blake3::Hash) -> Self {
        let mut bytes: [u8; 16] = [0; 16];
        bytes.copy_from_slice(&hash.as_bytes()[0..16]);
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
	}
	pub fn to_hex_string(&self) -> String {
        let mut s = String::new();
        let table = b"0123456789abcdef";
        for &b in self.0.iter() {
            s.push(table[(b >> 4) as usize] as char);
            s.push(table[(b & 0xf) as usize] as char);
        }
        s
    }
}

impl fmt::Debug for Hash128 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Hash128({})", self.to_hex_string())
    }
}

pub fn compute_hash_strong(input: &[u8]) -> Hash128 {
    let mut hasher_blake3 = blake3::Hasher::new();
    hasher_blake3.update(input);
    Hash128::new_from_blake3(&hasher_blake3.finalize())
}

pub fn compute_hash_weak(input: &[u8]) -> u32 {
    let mut hash_rolling = RollingHash::new();
    hash_rolling.update(&input);
    hash_rolling.get()
}
