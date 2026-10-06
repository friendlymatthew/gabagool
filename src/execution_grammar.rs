use crate::binary_grammar::{Function, FunctionType, GlobalType, MemoryType, RefType, TableType};
#[cfg(unix)]
use crate::mmap_backing::MmapBacking;
use std::io::{self, ErrorKind};
use std::sync::Arc;

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Ref {
    Null = 0,
    FunctionAddr(usize) = 1,
    RefExtern(usize) = 2,
    I31(i32) = 3,
    ExnRef(usize) = 4,
}

#[derive(Debug, Copy, Clone, Default)]
pub struct RawValue(u64);

impl RawValue {
    pub const fn as_i32(self) -> i32 {
        self.0 as i32
    }

    pub const fn as_i64(self) -> i64 {
        self.0 as i64
    }

    pub const fn as_f32(self) -> f32 {
        f32::from_bits(self.0 as u32)
    }

    pub const fn as_f64(self) -> f64 {
        f64::from_bits(self.0)
    }

    pub const fn as_ref(self) -> Ref {
        let tag = self.0 >> 61;
        let payload = self.0 & 0x1FFFFFFFFFFFFFFF;

        match tag {
            0 => Ref::Null,
            1 => Ref::FunctionAddr(payload as usize),
            2 => Ref::RefExtern(payload as usize),
            3 => Ref::I31(payload as i32),
            4 => Ref::ExnRef(payload as usize),
            _ => unreachable!(),
        }
    }

    pub const fn from_ref(r: Ref) -> Self {
        let raw = match r {
            Ref::Null => 0u64,
            Ref::FunctionAddr(a) => (1u64 << 61) | a as u64,
            Ref::RefExtern(a) => (2u64 << 61) | a as u64,
            Ref::I31(v) => (3u64 << 61) | (v as u32 as u64),
            Ref::ExnRef(a) => (4u64 << 61) | a as u64,
        };

        Self(raw)
    }

    pub const fn from_v128(v: i128) -> (Self, Self) {
        let hi = (v >> 64) as u64;
        let lo = v as u64;

        (Self(hi), Self(lo))
    }

    pub const fn as_v128(self, lo: Self) -> i128 {
        (self.0 as i128) << 64 | lo.0 as i128
    }
}

impl From<i32> for RawValue {
    fn from(value: i32) -> Self {
        Self(value as u64)
    }
}

impl From<i64> for RawValue {
    fn from(value: i64) -> Self {
        Self(value as u64)
    }
}

impl From<f32> for RawValue {
    fn from(value: f32) -> Self {
        Self(f32::to_bits(value) as u64)
    }
}

impl From<f64> for RawValue {
    fn from(value: f64) -> Self {
        Self(f64::to_bits(value))
    }
}

/// A temporary struct that accumulates address mappings during instantiation
#[derive(Debug, Clone, Default)]
pub struct AddressMap {
    pub function_addrs: Vec<usize>,
    pub table_addrs: Vec<usize>,
    pub mem_addrs: Vec<usize>,
    pub global_addrs: Vec<usize>,
    pub tag_addrs: Vec<usize>,
    pub elem_addrs: Vec<usize>,
    pub data_addrs: Vec<usize>,
    pub exports: Vec<ExportInstance>,
}

#[derive(Debug)]
pub enum FunctionInstance {
    Local {
        function_type: FunctionType,
        address_map: Arc<AddressMap>,
        code: Function,
    },
    Host {
        function_type: FunctionType,
        module_name: String,
        function_name: String,
    },
}

#[derive(Debug)]
pub struct TableInstance {
    pub table_type: TableType,
    pub elem: Vec<Ref>,
}

#[derive(Debug)]
pub struct MemoryInstance {
    pub memory_type: MemoryType,
    pub data: GuestMemory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestMemory {
    backing: Backing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Backing {
    Owned(Vec<u8>),
    #[cfg(unix)]
    Mmap(MmapBacking),
}

impl GuestMemory {
    pub fn new(size: usize) -> Self {
        Self {
            backing: Backing::Owned(vec![0u8; size]),
        }
    }

    pub const fn from_vec(bytes: Vec<u8>) -> Self {
        Self {
            backing: Backing::Owned(bytes),
        }
    }

    /// create an mmap-backed linear memory
    ///
    /// initial is the logical size in bytes
    /// capacity is the maximum size the memory can ever reach via resize
    #[cfg(unix)]
    pub fn with_mmap(initial: usize, capacity: usize) -> std::io::Result<Self> {
        Ok(Self {
            backing: Backing::Mmap(MmapBacking::new(initial, capacity)?),
        })
    }

    const fn slice(&self) -> &[u8] {
        match &self.backing {
            Backing::Owned(v) => v.as_slice(),
            #[cfg(unix)]
            Backing::Mmap(m) => m.as_slice(),
        }
    }

    const fn slice_mut(&mut self) -> &mut [u8] {
        match &mut self.backing {
            Backing::Owned(v) => v.as_mut_slice(),
            #[cfg(unix)]
            Backing::Mmap(m) => m.as_mut_slice(),
        }
    }

    pub const fn len(&self) -> usize {
        match &self.backing {
            Backing::Owned(v) => v.len(),
            #[cfg(unix)]
            Backing::Mmap(m) => m.len(),
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn read_u8(&self, ptr: usize) -> u8 {
        self.slice()[ptr]
    }

    pub fn read_u16(&self, ptr: usize) -> u16 {
        u16::from_le_bytes(self.slice()[ptr..ptr + 2].try_into().unwrap())
    }

    pub fn read_u32(&self, ptr: usize) -> u32 {
        u32::from_le_bytes(self.slice()[ptr..ptr + 4].try_into().unwrap())
    }

    pub fn read_u64(&self, ptr: usize) -> u64 {
        u64::from_le_bytes(self.slice()[ptr..ptr + 8].try_into().unwrap())
    }

    pub fn read_bytes(&self, ptr: usize, len: usize) -> &[u8] {
        &self.slice()[ptr..ptr + len]
    }

    pub fn write_u8(&mut self, ptr: usize, val: u8) {
        self.slice_mut()[ptr] = val;
    }

    pub fn write_u16(&mut self, ptr: usize, val: u16) {
        self.slice_mut()[ptr..ptr + 2].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_u32(&mut self, ptr: usize, val: u32) {
        self.slice_mut()[ptr..ptr + 4].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_u64(&mut self, ptr: usize, val: u64) {
        self.slice_mut()[ptr..ptr + 8].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_bytes(&mut self, ptr: usize, data: &[u8]) {
        self.slice_mut()[ptr..ptr + data.len()].copy_from_slice(data);
    }

    pub fn fill(&mut self, ptr: usize, len: usize, val: u8) {
        self.slice_mut()[ptr..ptr + len].fill(val);
    }

    pub fn resize(&mut self, new_len: usize, val: u8) {
        match &mut self.backing {
            Backing::Owned(v) => v.resize(new_len, val),
            #[cfg(unix)]
            Backing::Mmap(m) => m.resize(new_len, val),
        }
    }

    pub fn read_fixed<const N: usize>(&self, ptr: usize) -> [u8; N] {
        self.slice()[ptr..ptr + N].try_into().unwrap()
    }

    pub fn copy_within(&mut self, src_start: usize, src_end: usize, dest: usize) {
        self.slice_mut().copy_within(src_start..src_end, dest);
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.slice()
    }

    pub const fn is_mmap(&self) -> bool {
        match &self.backing {
            Backing::Owned(_) => false,
            #[cfg(unix)]
            Backing::Mmap(_) => true,
        }
    }

    #[cfg(unix)]
    pub fn fork_private(&self, n: usize) -> std::io::Result<Vec<Self>> {
        match &self.backing {
            Backing::Mmap(m) => Ok(m
                .fork_private(n)?
                .into_iter()
                .map(|m| Self {
                    backing: Backing::Mmap(m),
                })
                .collect::<Vec<_>>()),
            Backing::Owned(_) => Err(io::Error::new(
                ErrorKind::InvalidInput,
                "fork_private requires mmap-backed GuestMemory; use with_mmap",
            )),
        }
    }
}

#[cfg(all(test, unix))]
mod mmap_tests {
    use super::*;

    const PAGE: usize = 64 * 1024;

    #[test]
    fn basic_read_write() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_u32(0, 0xDEAD_BEEF);
        mem.write_u8(100, 42);
        mem.write_u64(200, 0x0123_4567_89AB_CDEF);

        assert_eq!(mem.read_u32(0), 0xDEAD_BEEF);
        assert_eq!(mem.read_u8(100), 42);
        assert_eq!(mem.read_u64(200), 0x0123_4567_89AB_CDEF);
    }

    #[test]
    fn fresh_memory_is_zero() {
        let mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        assert!(mem.as_slice().iter().all(|&b| b == 0));
    }

    #[test]
    fn resize_grow_zeros_new_region() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_u32(100, 0xCAFE_BABE);

        mem.resize(2 * PAGE, 0);
        assert_eq!(mem.len(), 2 * PAGE);
        assert_eq!(mem.read_u32(100), 0xCAFE_BABE);
        assert_eq!(mem.read_u8(PAGE), 0);
        assert_eq!(mem.read_u8(2 * PAGE - 1), 0);
    }

    #[test]
    fn resize_grow_with_nonzero_val() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.resize(2 * PAGE, 0xAA);
        assert_eq!(mem.read_u8(PAGE), 0xAA);
        assert_eq!(mem.read_u8(2 * PAGE - 1), 0xAA);
    }

    #[test]
    fn read_write_bytes_and_fill() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_bytes(50, &[1, 2, 3, 4, 5]);
        assert_eq!(mem.read_bytes(50, 5), &[1, 2, 3, 4, 5]);

        mem.fill(50, 5, 0xFF);
        assert_eq!(mem.read_bytes(50, 5), &[0xFF; 5]);
    }

    #[test]
    fn copy_within_works() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_bytes(0, &[1, 2, 3, 4, 5]);
        mem.copy_within(0, 5, 100);
        assert_eq!(mem.read_bytes(100, 5), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn clone_is_independent() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_u32(0, 0xAAAA_AAAA);
        let cloned = mem.clone();

        mem.write_u32(0, 0xBBBB_BBBB);
        assert_eq!(mem.read_u32(0), 0xBBBB_BBBB);
        assert_eq!(cloned.read_u32(0), 0xAAAA_AAAA);
    }

    #[test]
    #[should_panic]
    fn resize_beyond_capacity_panics() {
        let mut mem = GuestMemory::with_mmap(PAGE, 4 * PAGE).unwrap();
        mem.resize(8 * PAGE, 0);
    }

    #[test]
    fn many_allocations_no_fd_leak() {
        for _ in 0..1024 {
            let _mem = GuestMemory::with_mmap(PAGE, PAGE).unwrap();
        }
    }

    #[test]
    fn fork_children_see_parent_state() {
        let mut parent = GuestMemory::with_mmap(4 * PAGE, 16 * PAGE).unwrap();
        parent.write_u32(0, 0x1111_1111);
        parent.write_u32(PAGE, 0x2222_2222);
        parent.write_u32(3 * PAGE - 8, 0x3333_3333);

        let children = parent.fork_private(4).unwrap();

        for child in &children {
            assert_eq!(child.read_u32(0), 0x1111_1111);
            assert_eq!(child.read_u32(PAGE), 0x2222_2222);
            assert_eq!(child.read_u32(3 * PAGE - 8), 0x3333_3333);
            assert_eq!(child.len(), parent.len());
        }
    }

    #[test]
    fn fork_children_are_isolated_from_each_other() {
        let mut parent = GuestMemory::with_mmap(4 * PAGE, 16 * PAGE).unwrap();
        parent.write_u32(0, 0xAAAA_AAAA);

        let mut children = parent.fork_private(3).unwrap();

        children[0].write_u32(0, 0xBBBB_BBBB);
        children[1].write_u32(0, 0xCCCC_CCCC);
        // children[2] does not write

        assert_eq!(children[0].read_u32(0), 0xBBBB_BBBB);
        assert_eq!(children[1].read_u32(0), 0xCCCC_CCCC);
        assert_eq!(children[2].read_u32(0), 0xAAAA_AAAA);
    }

    #[test]
    fn fork_child_writes_do_not_leak_to_parent() {
        let mut parent = GuestMemory::with_mmap(4 * PAGE, 16 * PAGE).unwrap();
        parent.write_u32(0, 0xAAAA_AAAA);
        parent.write_u32(PAGE, 0xBBBB_BBBB);

        let mut children = parent.fork_private(2).unwrap();

        children[0].write_u32(0, 0xDEAD_BEEF);
        children[1].write_u32(PAGE, 0xCAFE_BABE);

        // parent unchanged after children's writes
        assert_eq!(parent.read_u32(0), 0xAAAA_AAAA);
        assert_eq!(parent.read_u32(PAGE), 0xBBBB_BBBB);
    }

    #[test]
    fn fork_child_can_resize_independently() {
        let mut parent = GuestMemory::with_mmap(2 * PAGE, 16 * PAGE).unwrap();
        parent.write_u32(0, 0x1234_5678);

        let mut children = parent.fork_private(2).unwrap();

        children[0].resize(4 * PAGE, 0);
        // children[1] keeps original len

        assert_eq!(children[0].len(), 4 * PAGE);
        assert_eq!(children[1].len(), 2 * PAGE);
        assert_eq!(children[0].read_u32(0), 0x1234_5678);
        assert_eq!(children[0].read_u8(3 * PAGE), 0);
        assert_eq!(parent.len(), 2 * PAGE);
    }

    #[test]
    fn fork_zero_children_is_empty() {
        let parent = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        let children = parent.fork_private(0).unwrap();
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn fork_owned_memory_errors() {
        let parent = GuestMemory::new(PAGE);
        let err = parent.fork_private(2).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}

#[derive(Debug)]
pub struct GlobalInstance {
    pub global_type: GlobalType,
    pub value: RawValue,
}

#[derive(Debug)]
pub struct ElementInstance {
    pub ref_type: RefType,
    pub elem: Vec<Ref>,
}

#[derive(Debug)]
pub struct TagInstance {
    pub tag_type: FunctionType,
}

#[derive(Debug)]
pub struct DataInstance {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum ExternalValue {
    Function { addr: usize },
    Table { addr: usize },
    Memory { addr: usize },
    Global { addr: usize },
    Tag { addr: usize },
}

#[derive(Debug, Clone)]
pub struct ExportInstance {
    pub name: String,
    pub value: ExternalValue,
}
