use crate::module::MemoryType;

#[derive(Debug)]
pub struct MemoryInstance {
    pub memory_type: MemoryType,
    pub data: GuestMemory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestMemory {
    bytes: Vec<u8>,
}

impl GuestMemory {
    pub fn new(size: usize) -> Self {
        Self {
            bytes: vec![0u8; size],
        }
    }

    pub const fn from_vec(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn read_u8(&self, ptr: usize) -> u8 {
        self.bytes[ptr]
    }

    pub fn read_u16(&self, ptr: usize) -> u16 {
        u16::from_le_bytes(self.bytes[ptr..ptr + 2].try_into().unwrap())
    }

    pub fn read_u32(&self, ptr: usize) -> u32 {
        u32::from_le_bytes(self.bytes[ptr..ptr + 4].try_into().unwrap())
    }

    pub fn read_u64(&self, ptr: usize) -> u64 {
        u64::from_le_bytes(self.bytes[ptr..ptr + 8].try_into().unwrap())
    }

    pub fn read_bytes(&self, ptr: usize, len: usize) -> &[u8] {
        &self.bytes[ptr..ptr + len]
    }

    pub fn write_u8(&mut self, ptr: usize, val: u8) {
        self.bytes[ptr] = val;
    }

    pub fn write_u16(&mut self, ptr: usize, val: u16) {
        self.bytes[ptr..ptr + 2].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_u32(&mut self, ptr: usize, val: u32) {
        self.bytes[ptr..ptr + 4].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_u64(&mut self, ptr: usize, val: u64) {
        self.bytes[ptr..ptr + 8].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_bytes(&mut self, ptr: usize, data: &[u8]) {
        self.bytes[ptr..ptr + data.len()].copy_from_slice(data);
    }

    pub fn fill(&mut self, ptr: usize, len: usize, val: u8) {
        self.bytes[ptr..ptr + len].fill(val);
    }

    pub fn resize(&mut self, new_len: usize, val: u8) {
        self.bytes.resize(new_len, val);
    }

    pub fn read_fixed<const N: usize>(&self, ptr: usize) -> [u8; N] {
        self.bytes[ptr..ptr + N].try_into().unwrap()
    }

    pub fn copy_within(&mut self, src_start: usize, src_end: usize, dest: usize) {
        self.bytes.copy_within(src_start..src_end, dest);
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}
