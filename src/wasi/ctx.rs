use alloc::collections::BTreeMap;

use super::{MemoryInput, MemoryResource};
use crate::{
    prelude::*,
    wasi::traits::{RandomError, WasiRandom},
};

pub const RIGHTS_FD_WRITE: u64 = 1 << 6;
pub const RIGHTS_FD_READ: u64 = 1 << 1;

pub const STDIN_FD: u32 = 0;
pub const STDOUT_FD: u32 = 1;
pub const STDERR_FD: u32 = 2;

#[derive(Debug)]
pub struct WasiCtx<R = MemoryResource> {
    args: Vec<String>,
    environment: Vec<String>,
    descriptors: BTreeMap<u32, FdEntry<R>>,

    random: Box<dyn WasiRandom>,
}

impl<R> WasiCtx<R> {
    pub fn new(stdin: R, stdout: R, stderr: R) -> Self {
        let mut descriptors = BTreeMap::new();

        descriptors.insert(
            STDIN_FD,
            FdEntry {
                resource: stdin,
                flags: 0,
                rights_base: RIGHTS_FD_READ,
                rights_inheriting: 0,
            },
        );

        descriptors.insert(
            STDOUT_FD,
            FdEntry {
                resource: stdout,
                flags: 0,
                rights_base: RIGHTS_FD_WRITE,
                rights_inheriting: 0,
            },
        );

        descriptors.insert(
            STDERR_FD,
            FdEntry {
                resource: stderr,
                flags: 0,
                rights_base: RIGHTS_FD_WRITE,
                rights_inheriting: 0,
            },
        );

        Self {
            args: vec![],
            environment: vec![],
            descriptors,
            random: Box::new(UnavailableRandom),
        }
    }

    pub fn set_random(&mut self, random: impl WasiRandom + 'static) {
        self.random = Box::new(random);
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub const fn args_mut(&mut self) -> &mut Vec<String> {
        &mut self.args
    }

    pub fn environment(&self) -> &[String] {
        &self.environment
    }

    pub const fn environment_mut(&mut self) -> &mut Vec<String> {
        &mut self.environment
    }

    pub const fn descriptors(&self) -> &BTreeMap<u32, FdEntry<R>> {
        &self.descriptors
    }

    pub const fn descriptors_mut(&mut self) -> &mut BTreeMap<u32, FdEntry<R>> {
        &mut self.descriptors
    }

    pub fn random(&self) -> &dyn WasiRandom {
        &*self.random
    }

    pub fn random_mut(&mut self) -> &mut dyn WasiRandom {
        &mut *self.random
    }
}

#[repr(u8)]
#[derive(Debug)]
pub enum FileType {
    Unknown = 0,
    BlockDevice = 1,
    CharacterDevice = 2,
    Directory = 3,
    RegularFile = 4,
    SocketDgram = 5,
    SocketStream = 6,
    SymbolicLink = 7,
}

#[derive(Debug)]
pub struct FdEntry<R> {
    pub resource: R,
    pub flags: u16,
    pub rights_base: u64,
    pub rights_inheriting: u64,
}

impl WasiCtx<MemoryResource> {
    pub fn in_memory(stdin: Vec<u8>) -> Self {
        Self::new(
            MemoryResource::Input(MemoryInput::new(stdin)),
            MemoryResource::Output(Vec::new()),
            MemoryResource::Output(Vec::new()),
        )
    }

    pub fn stdout(&self) -> Option<&[u8]> {
        self.captured_output(STDOUT_FD)
    }

    pub fn stderr(&self) -> Option<&[u8]> {
        self.captured_output(STDERR_FD)
    }

    fn captured_output(&self, fd: u32) -> Option<&[u8]> {
        match &self.descriptors.get(&fd)?.resource {
            MemoryResource::Output(bytes) => Some(bytes),
            MemoryResource::Input(_) => None,
        }
    }
}

impl Default for WasiCtx<MemoryResource> {
    fn default() -> Self {
        Self::in_memory(Vec::new())
    }
}

impl<R> FdEntry<R> {
    pub const fn can_read(&self) -> bool {
        self.rights_base & RIGHTS_FD_READ != 0
    }

    pub const fn can_write(&self) -> bool {
        self.rights_base & RIGHTS_FD_WRITE != 0
    }
}

#[derive(Debug)]
pub struct UnavailableRandom;

impl WasiRandom for UnavailableRandom {
    fn fill_bytes(&mut self, _: &mut [u8]) -> Result<(), RandomError> {
        Err(RandomError::Unavailable)
    }
}
