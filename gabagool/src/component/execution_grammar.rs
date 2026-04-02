use crate::{ComponentValueKind, StringEncoding};
use std::collections::HashMap;
use std::result::Result as StdResult;

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
    Record(Vec<(std::string::String, Self)>),
    Tuple(Vec<Self>),
    Variant(std::string::String, Option<Box<Self>>),
    Enum(std::string::String),
    Option(Option<Box<Self>>),
    Result(StdResult<Option<Box<Self>>, Option<Box<Self>>>),
    Flags(Vec<std::string::String>),
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
    fn from(v: std::string::String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for ComponentValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ComponentInstance(pub(crate) usize);

#[derive(Debug, Clone)]
pub struct LiftedFunc {
    pub func_addr: usize,
    pub memory_addr: Option<usize>,
    pub realloc_addr: Option<usize>,
    pub string_encoding: StringEncoding,
    pub result_types: Vec<ComponentValueKind>,
}

#[derive(Debug)]
pub struct InstantiatedComponent {
    pub exports: HashMap<String, LiftedFunc>,
    pub may_leave: bool,
}
