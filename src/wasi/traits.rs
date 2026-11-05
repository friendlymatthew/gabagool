use crate::prelude::*;
use core::result::Result;

pub trait WasiOutput {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, OutputError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputError {
    Io,
    Interrupted,
    WouldBlock,
}

impl WasiOutput for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, OutputError> {
        self.extend_from_slice(bytes);
        Ok(bytes.len())
    }
}
