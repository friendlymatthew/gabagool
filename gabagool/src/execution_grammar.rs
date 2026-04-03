use crate::binary_grammar::{Function, FunctionType, GlobalType, MemoryType, RefType, TableType};
use std::rc::Rc;

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
        address_map: Rc<AddressMap>,
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
pub struct GuestMemory(pub(crate) Vec<u8>);

impl GuestMemory {
    pub fn new(size: usize) -> Self {
        Self(vec![0u8; size])
    }

    pub const fn len(&self) -> usize {
        self.0.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn read_u8(&self, ptr: usize) -> u8 {
        self.0[ptr]
    }

    pub fn read_u16(&self, ptr: usize) -> u16 {
        u16::from_le_bytes(self.0[ptr..ptr + 2].try_into().unwrap())
    }

    pub fn read_u32(&self, ptr: usize) -> u32 {
        u32::from_le_bytes(self.0[ptr..ptr + 4].try_into().unwrap())
    }

    pub fn read_u64(&self, ptr: usize) -> u64 {
        u64::from_le_bytes(self.0[ptr..ptr + 8].try_into().unwrap())
    }

    pub fn read_bytes(&self, ptr: usize, len: usize) -> &[u8] {
        &self.0[ptr..ptr + len]
    }

    pub fn write_u8(&mut self, ptr: usize, val: u8) {
        self.0[ptr] = val;
    }

    pub fn write_u16(&mut self, ptr: usize, val: u16) {
        self.0[ptr..ptr + 2].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_u32(&mut self, ptr: usize, val: u32) {
        self.0[ptr..ptr + 4].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_u64(&mut self, ptr: usize, val: u64) {
        self.0[ptr..ptr + 8].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_bytes(&mut self, ptr: usize, data: &[u8]) {
        self.0[ptr..ptr + data.len()].copy_from_slice(data);
    }

    pub fn fill(&mut self, ptr: usize, len: usize, val: u8) {
        self.0[ptr..ptr + len].fill(val);
    }

    pub fn resize(&mut self, new_len: usize, val: u8) {
        self.0.resize(new_len, val);
    }

    pub fn read_fixed<const N: usize>(&self, ptr: usize) -> [u8; N] {
        self.0[ptr..ptr + N].try_into().unwrap()
    }

    pub fn copy_within(&mut self, src_start: usize, src_end: usize, dest: usize) {
        self.0.copy_within(src_start..src_end, dest);
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
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
