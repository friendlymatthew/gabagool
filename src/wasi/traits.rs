use crate::{prelude::*, wasi::FileType};
use core::{num::NonZeroU64, result::Result};

pub trait WasiOutput {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, WasiIOError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasiIOError {
    Io,
    Interrupted,
    WouldBlock,
    Unsupported,
}

impl WasiOutput for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, WasiIOError> {
        self.extend_from_slice(bytes);
        Ok(bytes.len())
    }
}

pub trait WasiResource {
    fn file_type(&self) -> FileType;

    fn datasync(&mut self) -> Result<(), WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    /// Apply descriptor flags. On error, leave the resource unchanged.
    /// The ABI layer updates its metadata only after this succeeds.
    fn set_fdflags(&mut self, _flags: u16) -> Result<(), WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    fn output_mut(&mut self) -> Option<&mut dyn WasiOutput> {
        None
    }

    fn input_mut(&mut self) -> Option<&mut dyn WasiInput> {
        None
    }
}

impl WasiResource for Vec<u8> {
    fn file_type(&self) -> super::FileType {
        super::FileType::Unknown
    }

    fn output_mut(&mut self) -> Option<&mut dyn WasiOutput> {
        Some(self)
    }
}

pub trait WasiInput {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, WasiIOError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomError {
    Unavailable,
}

pub trait WasiRandom: core::fmt::Debug {
    fn fill_bytes(&mut self, _buffer: &mut [u8]) -> Result<(), RandomError> {
        Err(RandomError::Unavailable)
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum ClockId {
    Realtime = 0,
    Monotonic = 1,
    ProcessCpuTime = 2,
    ThreadCpuTime = 3,
}

impl TryFrom<u32> for ClockId {
    type Error = ClockError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        let out = match value {
            0 => Self::Realtime,
            1 => Self::Monotonic,
            2 => Self::ProcessCpuTime,
            3 => Self::ThreadCpuTime,
            _ => return Err(ClockError::Unsupported),
        };

        Ok(out)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ClockError {
    Unsupported,
    Unavailable,
    Overflow,
}

pub trait WasiClock: core::fmt::Debug {
    fn now_nanos(&mut self, _clock: ClockId, _precision_ns: u64) -> Result<u64, ClockError> {
        Err(ClockError::Unavailable)
    }

    fn resolution_ns(&mut self, _clock: ClockId) -> Result<NonZeroU64, ClockError> {
        Err(ClockError::Unavailable)
    }
}
