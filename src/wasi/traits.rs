use crate::{
    prelude::*,
    wasi::{
        ctx::{DirectoryEntry, Filestat},
        FileType,
    },
};
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

#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    Start(u64),
    Current(i64),
    End(i64),
}

#[derive(Debug, Clone, Copy)]
pub enum SeekError {
    Unsupported,
    InvalidOffset,
    Overflow,
    Io,
}

#[derive(Debug, Clone, Copy)]
pub enum PathError {
    Io,
    Unsupported,
    NotFound,
    AlreadyExists,
    NotDirectory,
    PermissionDenied,
    NotCapable,
    InvalidInput,
    CrossDevice,
}

#[derive(Debug, Clone, Copy)]
pub struct OpenOptions {
    pub follow_symlink: bool,
    pub create: bool,
    pub directory: bool,
    pub exclusive: bool,
    pub truncate: bool,
    pub fdflags: u16,
    pub rights_base: u64,
    pub rights_inheriting: u64,
}

impl WasiOutput for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, WasiIOError> {
        self.extend_from_slice(bytes);
        Ok(bytes.len())
    }
}

pub trait WasiResource {
    fn file_type(&self) -> FileType;

    fn open(&mut self, _path: &str, _options: OpenOptions) -> Result<Self, PathError>
    where
        Self: Sized,
    {
        Err(PathError::Unsupported)
    }

    fn link(
        &self,
        _old_path: &str,
        _follow_symlink: bool,
        _target_directory: &Self,
        _new_path: &str,
    ) -> Result<(), PathError>
    where
        Self: Sized,
    {
        Err(PathError::Unsupported)
    }

    fn create_directory(&mut self, _path: &str) -> Result<(), PathError> {
        Err(PathError::Unsupported)
    }

    fn path_filestat(&mut self, _path: &str, _follow_symlink: bool) -> Result<Filestat, PathError> {
        Err(PathError::Unsupported)
    }

    fn path_set_times(
        &mut self,
        _path: &str,
        _follow_symlink: bool,
        _atim: Option<u64>,
        _mtim: Option<u64>,
    ) -> Result<(), PathError> {
        Err(PathError::Unsupported)
    }

    fn seek(&mut self, _from: SeekFrom) -> Result<u64, SeekError> {
        Err(SeekError::Unsupported)
    }

    fn tell(&mut self) -> Result<u64, SeekError> {
        self.seek(SeekFrom::Current(0))
    }

    fn directory_entry(&mut self, _cookie: u64) -> Result<Option<DirectoryEntry>, WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    fn read_at(&mut self, _buffer: &mut [u8], _offset: u64) -> Result<usize, WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    fn write_at(&mut self, _buffer: &[u8], _offset: u64) -> Result<usize, WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    fn filestat(&mut self) -> Result<Filestat, WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    fn set_size(&mut self, _size: u64) -> Result<(), WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    fn set_times(&mut self, _atim: Option<u64>, _mtim: Option<u64>) -> Result<(), WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    fn datasync(&mut self) -> Result<(), WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

    fn sync(&mut self) -> Result<(), WasiIOError> {
        Err(WasiIOError::Unsupported)
    }

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
    fn filestat(&mut self) -> Result<Filestat, WasiIOError> {
        Ok(Filestat::stream(self.file_type()))
    }

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
