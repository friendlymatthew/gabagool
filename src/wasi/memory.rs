use crate::{prelude::*, wasi::ctx::Filestat};

use super::{FileType, WasiIOError, WasiInput, WasiOutput, WasiResource};

#[derive(Debug)]
pub enum MemoryResource {
    Input(MemoryInput),
    Output(Vec<u8>),
}

impl WasiResource for MemoryResource {
    fn filestat(&mut self) -> core::result::Result<Filestat, WasiIOError> {
        Ok(Filestat::stream(self.file_type()))
    }

    fn file_type(&self) -> FileType {
        FileType::Unknown
    }

    fn input_mut(&mut self) -> Option<&mut dyn WasiInput> {
        match self {
            Self::Input(input) => Some(input),
            Self::Output(_) => None,
        }
    }

    fn output_mut(&mut self) -> Option<&mut dyn WasiOutput> {
        match self {
            Self::Output(output) => Some(output),
            Self::Input(_) => None,
        }
    }
}

#[derive(Debug)]
pub struct MemoryInput {
    bytes: Vec<u8>,
    position: usize,
}

impl MemoryInput {
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, position: 0 }
    }
}

impl WasiInput for MemoryInput {
    fn read(&mut self, buffer: &mut [u8]) -> core::result::Result<usize, WasiIOError> {
        let remaining = self.bytes.get(self.position..).ok_or(WasiIOError::Io)?;
        let count = buffer.len().min(remaining.len());

        buffer[..count].copy_from_slice(&remaining[..count]);

        self.position += count;

        Ok(count)
    }
}
