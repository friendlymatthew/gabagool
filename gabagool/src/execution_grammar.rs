use crate::binary_grammar::{Function, FunctionType, GlobalType, MemoryType, RefType, TableType};
use std::rc::Rc;
use std::result::Result as StdResult;
use std::{collections::HashMap, fmt::Debug};

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

#[derive(Debug, Clone, PartialEq)]
pub enum ComponentValue {
    Bool(bool),
    S8(i8),
    U8(u8),
    S16(i16),
    U16(u16),
    S32(i32),
    U32(u32),
    S64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    List(Vec<Self>),
    Record(Vec<(String, Self)>),
    Tuple(Vec<Self>),
    Variant(String, Option<Box<Self>>),
    Enum(String),
    Option(Option<Box<Self>>),
    Result(StdResult<Option<Box<Self>>, Option<Box<Self>>>),
    Flags(Vec<String>),
}

impl From<bool> for ComponentValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i8> for ComponentValue {
    fn from(v: i8) -> Self {
        Self::S8(v)
    }
}

impl From<u8> for ComponentValue {
    fn from(v: u8) -> Self {
        Self::U8(v)
    }
}

impl From<i16> for ComponentValue {
    fn from(v: i16) -> Self {
        Self::S16(v)
    }
}

impl From<u16> for ComponentValue {
    fn from(v: u16) -> Self {
        Self::U16(v)
    }
}

impl From<i32> for ComponentValue {
    fn from(v: i32) -> Self {
        Self::S32(v)
    }
}

impl From<u32> for ComponentValue {
    fn from(v: u32) -> Self {
        Self::U32(v)
    }
}

impl From<i64> for ComponentValue {
    fn from(v: i64) -> Self {
        Self::S64(v)
    }
}

impl From<u64> for ComponentValue {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}

impl From<f32> for ComponentValue {
    fn from(v: f32) -> Self {
        Self::F32(v)
    }
}

impl From<f64> for ComponentValue {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}

impl From<char> for ComponentValue {
    fn from(v: char) -> Self {
        Self::Char(v)
    }
}

impl From<String> for ComponentValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for ComponentValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
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
    pub data: Vec<u8>,
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

#[derive(Debug)]
pub struct InstantiatedComponent {
    // maps from name to func addr
    pub exports: HashMap<String, usize>,
    pub may_leave: bool,
}
