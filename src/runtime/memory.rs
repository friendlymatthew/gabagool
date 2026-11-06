use crate::module::MemoryType;
use crate::{prelude::*, Error, Result, Trap};

const OOB: Error = Error::Trap(Trap::OutOfBoundsMemoryAccess);

#[derive(Debug)]
pub struct MemoryInstance {
    pub memory_type: MemoryType,
    pub data: GuestMemory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestMemory {
    bytes: Vec<u8>,
}

impl From<Vec<u8>> for GuestMemory {
    fn from(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl GuestMemory {
    pub fn new(size: usize) -> Self {
        Self {
            bytes: vec![0u8; size],
        }
    }

    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn read_bytes(&self, ptr: usize, len: usize) -> Result<&[u8]> {
        let end = ptr.checked_add(len).ok_or(OOB)?;
        self.bytes.get(ptr..end).ok_or(OOB)
    }

    pub fn read_bytes_mut(&mut self, ptr: usize, len: usize) -> Result<&mut [u8]> {
        let end = ptr.checked_add(len).ok_or(OOB)?;
        self.bytes.get_mut(ptr..end).ok_or(OOB)
    }

    pub fn read<const N: usize>(&self, ptr: usize) -> Result<[u8; N]> {
        Ok(self.read_bytes(ptr, N)?.try_into().unwrap())
    }

    pub fn write<const N: usize>(&mut self, ptr: usize, val: [u8; N]) -> Result<()> {
        self.write_bytes(ptr, &val)
    }

    pub fn write_bytes(&mut self, ptr: usize, data: &[u8]) -> Result<()> {
        self.read_bytes_mut(ptr, data.len())?.copy_from_slice(data);
        Ok(())
    }

    pub fn fill(&mut self, ptr: usize, len: usize, val: u8) -> Result<()> {
        self.read_bytes_mut(ptr, len)?.fill(val);
        Ok(())
    }

    pub fn copy_within(&mut self, src: usize, dst: usize, len: usize) -> Result<()> {
        self.read_bytes(src, len)?;
        self.read_bytes(dst, len)?;
        self.bytes.copy_within(src..src + len, dst);
        Ok(())
    }

    pub fn resize(&mut self, new_len: usize, val: u8) {
        self.bytes.resize(new_len, val);
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}
