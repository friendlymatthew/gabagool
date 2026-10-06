use std::{array::TryFromSliceError, fmt, str::Utf8Error};

use crate::execution_grammar::RawValue;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trap {
    Unreachable,
    IntegerDivideByZero,
    IntegerOverflow,
    InvalidConversionToInteger,
    OutOfBoundsMemoryAccess,
    OutOfBoundsTableAccess,
    UndefinedElement,
    OutOfBoundsDataAccess,
    IndirectCallTypeMismatch,
    NullReference,
    CastFailure,
    OutOfBoundsArrayAccess,
    CallStackExhausted,
}

/// A thrown WebAssembly exception (uncaught)
#[derive(Debug, Clone)]
pub struct Exception {
    pub tag_addr: usize,
    pub values: Vec<RawValue>,
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable => write!(f, "unreachable"),
            Self::IntegerDivideByZero => write!(f, "integer divide by zero"),
            Self::IntegerOverflow => write!(f, "integer overflow"),
            Self::InvalidConversionToInteger => write!(f, "invalid conversion to integer"),
            Self::OutOfBoundsMemoryAccess => write!(f, "out of bounds memory access"),
            Self::OutOfBoundsTableAccess => write!(f, "out of bounds table access"),
            Self::UndefinedElement => write!(f, "undefined element"),
            Self::OutOfBoundsDataAccess => write!(f, "out of bounds data segment access"),
            Self::IndirectCallTypeMismatch => write!(f, "indirect call type mismatch"),
            Self::NullReference => write!(f, "null reference"),
            Self::CastFailure => write!(f, "cast failure"),
            Self::OutOfBoundsArrayAccess => write!(f, "out of bounds array access"),
            Self::CallStackExhausted => write!(f, "call stack exhausted"),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Parse(String),
    Instantiation(String),
    Trap(Trap),
    Exception(Exception),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::Instantiation(msg) => write!(f, "instantiation error: {msg}"),
            Self::Trap(trap) => write!(f, "trap: {trap}"),
            Self::Exception(_) => write!(f, "unhandled exception"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Trap> for Error {
    fn from(trap: Trap) -> Self {
        Self::Trap(trap)
    }
}

impl From<TryFromSliceError> for Error {
    fn from(e: TryFromSliceError) -> Self {
        Self::Parse(e.to_string())
    }
}

impl From<Utf8Error> for Error {
    fn from(e: Utf8Error) -> Self {
        Self::Parse(e.to_string())
    }
}

#[macro_export]
macro_rules! parse_err {
    ($($arg:tt)*) => {
        return Err($crate::error::Error::Parse(format!($($arg)*)))
    };
}

#[macro_export]
macro_rules! instantiation_err {
    ($($arg:tt)*) => {
        return Err($crate::error::Error::Instantiation(format!($($arg)*)))
    };
}

#[macro_export]
macro_rules! trap {
    ($trap:expr) => {
        return Err($crate::error::Error::Trap($trap))
    };
}

#[macro_export]
macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !$cond {
            return Err($err);
        }
    };
}
