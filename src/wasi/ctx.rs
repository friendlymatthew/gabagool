use alloc::collections::BTreeMap;

use crate::prelude::*;

pub const RIGHTS_FD_WRITE: u64 = 1 << 6;

pub const STDOUT_FD: u32 = 1;
pub const STDERR_FD: u32 = 2;

#[derive(Debug)]
pub struct WasiCtx<R = Vec<u8>> {
    args: Vec<String>,
    environment: Vec<String>,
    descriptors: BTreeMap<u32, FdEntry<R>>,
}

impl<R> WasiCtx<R> {
    pub fn new(stdout: R, stderr: R) -> Self {
        let mut descriptors = BTreeMap::new();

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
        }
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

impl<R: Default> Default for WasiCtx<R> {
    fn default() -> Self {
        Self::new(R::default(), R::default())
    }
}
