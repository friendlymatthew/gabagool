use crate::{prelude::*, wasi::FileType};
use core::result::Result;

pub trait WasiOutput {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, WasiIOError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasiIOError {
    Io,
    Interrupted,
    WouldBlock,
}

impl WasiOutput for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, WasiIOError> {
        self.extend_from_slice(bytes);
        Ok(bytes.len())
    }
}

pub trait WasiResource {
    fn file_type(&self) -> FileType;

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
