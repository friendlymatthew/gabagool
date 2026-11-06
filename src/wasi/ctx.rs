use alloc::collections::BTreeMap;

use super::{MemoryInput, MemoryResource};
use crate::{
    prelude::*,
    wasi::traits::{WasiClock, WasiRandom},
};

pub const RIGHTS_FD_WRITE: u64 = 1 << 6;
pub const RIGHTS_FD_READ: u64 = 1 << 1;
pub const RIGHTS_FD_SEEK: u64 = 1 << 2;
pub const RIGHTS_FD_TELL: u64 = 1 << 5;
pub const RIGHTS_FD_READDIR: u64 = 1 << 14;
pub const RIGHTS_FD_ADVISE: u64 = 1 << 7;
pub const RIGHTS_FD_DATASYNC: u64 = 1 << 0;
pub const RIGHTS_FD_SYNC: u64 = 1 << 4;
pub const RIGHTS_FD_FDSTAT_SET_FLAGS: u64 = 1 << 3;
pub const RIGHTS_FD_FILESTAT_GET: u64 = 1 << 21;
pub const RIGHTS_FD_FILESTAT_SET_SIZE: u64 = 1 << 22;
pub const RIGHTS_FD_FILESTAT_SET_TIMES: u64 = 1 << 23;
pub const RIGHTS_PATH_CREATE_DIRECTORY: u64 = 1 << 9;
pub const RIGHTS_PATH_CREATE_FILE: u64 = 1 << 10;
pub const RIGHTS_PATH_OPEN: u64 = 1 << 13;
pub const RIGHTS_PATH_READLINK: u64 = 1 << 15;
pub const RIGHTS_PATH_SYMLINK: u64 = 1 << 24;
pub const RIGHTS_PATH_REMOVE_DIRECTORY: u64 = 1 << 25;
pub const RIGHTS_PATH_UNLINK_FILE: u64 = 1 << 26;
pub const RIGHTS_PATH_FILESTAT_SET_SIZE: u64 = 1 << 19;

pub const OFLAGS_CREAT: u16 = 1 << 0;
pub const OFLAGS_DIRECTORY: u16 = 1 << 1;
pub const OFLAGS_EXCL: u16 = 1 << 2;
pub const OFLAGS_TRUNC: u16 = 1 << 3;
pub(super) const OFLAGS_ALL: u16 = OFLAGS_CREAT | OFLAGS_DIRECTORY | OFLAGS_EXCL | OFLAGS_TRUNC;
pub const RIGHTS_PATH_LINK_SOURCE: u64 = 1 << 11;
pub const RIGHTS_PATH_LINK_TARGET: u64 = 1 << 12;
pub const RIGHTS_PATH_RENAME_SOURCE: u64 = 1 << 16;
pub const RIGHTS_PATH_RENAME_TARGET: u64 = 1 << 17;
pub const RIGHTS_PATH_FILESTAT_GET: u64 = 1 << 18;
pub const RIGHTS_PATH_FILESTAT_SET_TIMES: u64 = 1 << 20;
pub const LOOKUPFLAGS_SYMLINK_FOLLOW: u32 = 1 << 0;

pub const FSTFLAGS_ATIM: u16 = 1 << 0;
pub const FSTFLAGS_ATIM_NOW: u16 = 1 << 1;
pub const FSTFLAGS_MTIM: u16 = 1 << 2;
pub const FSTFLAGS_MTIM_NOW: u16 = 1 << 3;

pub const FDFLAGS_APPEND: u16 = 1 << 0;
pub const FDFLAGS_DSYNC: u16 = 1 << 1;
pub const FDFLAGS_NONBLOCK: u16 = 1 << 2;
pub const FDFLAGS_RSYNC: u16 = 1 << 3;
pub const FDFLAGS_SYNC: u16 = 1 << 4;
pub(super) const FDFLAGS_ALL: u16 =
    FDFLAGS_APPEND | FDFLAGS_DSYNC | FDFLAGS_NONBLOCK | FDFLAGS_RSYNC | FDFLAGS_SYNC;

pub const STDIN_FD: u32 = 0;
pub const STDOUT_FD: u32 = 1;
pub const STDERR_FD: u32 = 2;

#[derive(Debug)]
pub struct WasiCtx<R = MemoryResource> {
    args: Vec<String>,
    environment: Vec<String>,
    descriptors: BTreeMap<u32, FdEntry<R>>,

    random: Box<dyn WasiRandom>,
    clock: Box<dyn WasiClock>,
}

impl<R> WasiCtx<R> {
    pub fn new(stdin: R, stdout: R, stderr: R) -> Self {
        let mut descriptors = BTreeMap::new();

        descriptors.insert(
            STDIN_FD,
            FdEntry {
                resource: stdin,
                flags: 0,
                rights_base: RIGHTS_FD_READ | RIGHTS_FD_FILESTAT_GET,
                rights_inheriting: 0,
                preopen_name: None,
            },
        );

        descriptors.insert(
            STDOUT_FD,
            FdEntry {
                resource: stdout,
                flags: 0,
                rights_base: RIGHTS_FD_WRITE | RIGHTS_FD_FILESTAT_GET,
                rights_inheriting: 0,
                preopen_name: None,
            },
        );

        descriptors.insert(
            STDERR_FD,
            FdEntry {
                resource: stderr,
                flags: 0,
                rights_base: RIGHTS_FD_WRITE | RIGHTS_FD_FILESTAT_GET,
                rights_inheriting: 0,
                preopen_name: None,
            },
        );

        Self {
            args: vec![],
            environment: vec![],
            descriptors,
            random: Box::new(UnavailableRandom),
            clock: Box::new(UnavailableClock),
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

    pub fn random(&self) -> &dyn WasiRandom {
        &*self.random
    }

    pub fn random_mut(&mut self) -> &mut dyn WasiRandom {
        &mut *self.random
    }

    pub fn set_random(&mut self, random: impl WasiRandom + 'static) {
        self.random = Box::new(random);
    }

    pub fn clock(&self) -> &dyn WasiClock {
        &*self.clock
    }

    pub fn clock_mut(&mut self) -> &mut dyn WasiClock {
        &mut *self.clock
    }

    pub fn set_clock(&mut self, clock: impl WasiClock + 'static) {
        self.clock = Box::new(clock);
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
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
    /// Guest-visible directory name for a preopened capability, without a NUL.
    /// Ordinary descriptors, including standard streams, have no preopen name.
    pub preopen_name: Option<Vec<u8>>,
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

    pub const fn can_pread(&self) -> bool {
        self.can_read() && self.rights_base & RIGHTS_FD_SEEK != 0
    }

    pub const fn can_seek(&self) -> bool {
        self.rights_base & RIGHTS_FD_SEEK != 0
    }

    pub const fn can_tell(&self) -> bool {
        self.can_seek() || self.rights_base & RIGHTS_FD_TELL != 0
    }

    pub const fn can_create_directory(&self) -> bool {
        self.rights_base & RIGHTS_PATH_CREATE_DIRECTORY != 0
    }

    pub const fn can_remove_directory(&self) -> bool {
        self.rights_base & RIGHTS_PATH_REMOVE_DIRECTORY != 0
    }

    pub const fn can_unlink_file(&self) -> bool {
        self.rights_base & RIGHTS_PATH_UNLINK_FILE != 0
    }

    pub const fn can_open(&self) -> bool {
        self.rights_base & RIGHTS_PATH_OPEN != 0
    }

    pub const fn can_readlink(&self) -> bool {
        self.rights_base & RIGHTS_PATH_READLINK != 0
    }

    pub const fn can_symlink(&self) -> bool {
        self.rights_base & RIGHTS_PATH_SYMLINK != 0
    }

    pub const fn can_link_source(&self) -> bool {
        self.rights_base & RIGHTS_PATH_LINK_SOURCE != 0
    }

    pub const fn can_link_target(&self) -> bool {
        self.rights_base & RIGHTS_PATH_LINK_TARGET != 0
    }

    pub const fn can_rename_source(&self) -> bool {
        self.rights_base & RIGHTS_PATH_RENAME_SOURCE != 0
    }

    pub const fn can_rename_target(&self) -> bool {
        self.rights_base & RIGHTS_PATH_RENAME_TARGET != 0
    }

    pub const fn can_path_filestat_get(&self) -> bool {
        self.rights_base & RIGHTS_PATH_FILESTAT_GET != 0
    }

    pub const fn can_path_filestat_set_times(&self) -> bool {
        self.rights_base & RIGHTS_PATH_FILESTAT_SET_TIMES != 0
    }

    pub const fn can_write(&self) -> bool {
        self.rights_base & RIGHTS_FD_WRITE != 0
    }

    pub const fn can_pwrite(&self) -> bool {
        self.can_write() && self.rights_base & RIGHTS_FD_SEEK != 0
    }

    pub const fn can_readdir(&self) -> bool {
        self.rights_base & RIGHTS_FD_READDIR != 0
    }

    pub const fn can_advise(&self) -> bool {
        self.rights_base & RIGHTS_FD_ADVISE != 0
    }

    pub const fn can_datasync(&self) -> bool {
        self.rights_base & RIGHTS_FD_DATASYNC != 0
    }

    pub const fn can_sync(&self) -> bool {
        self.rights_base & RIGHTS_FD_SYNC != 0
    }

    pub const fn can_set_flags(&self) -> bool {
        self.rights_base & RIGHTS_FD_FDSTAT_SET_FLAGS != 0
    }

    pub const fn can_filestat_get(&self) -> bool {
        self.rights_base & RIGHTS_FD_FILESTAT_GET != 0
    }

    pub const fn can_filestat_set_size(&self) -> bool {
        self.rights_base & RIGHTS_FD_FILESTAT_SET_SIZE != 0
    }

    pub const fn can_filestat_set_times(&self) -> bool {
        self.rights_base & RIGHTS_FD_FILESTAT_SET_TIMES != 0
    }
}

#[derive(Debug)]
pub struct UnavailableRandom;
impl WasiRandom for UnavailableRandom {}

#[derive(Debug)]
pub struct UnavailableClock;
impl WasiClock for UnavailableClock {}

#[derive(Debug)]
pub struct Filestat {
    pub dev: u64,
    pub ino: u64,
    pub filetype: FileType,
    pub nlink: u64,
    pub size: u64,
    pub atim: u64,
    pub mtim: u64,
    pub ctim: u64,
}

impl Filestat {
    pub const fn stream(filetype: FileType) -> Self {
        Self {
            dev: 0,
            ino: 0,
            filetype,
            nlink: 1,
            size: 0,
            atim: 0,
            mtim: 0,
            ctim: 0,
        }
    }

    pub(super) fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0; 64];
        bytes[0..8].copy_from_slice(&self.dev.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.ino.to_le_bytes());
        bytes[16] = self.filetype as u8;

        // Offsets 17..24 are ABI padding.
        bytes[24..32].copy_from_slice(&self.nlink.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.size.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.atim.to_le_bytes());
        bytes[48..56].copy_from_slice(&self.mtim.to_le_bytes());
        bytes[56..64].copy_from_slice(&self.ctim.to_le_bytes());

        bytes
    }
}

#[derive(Debug)]
pub struct DirectoryEntry {
    pub next_cookie: u64,
    pub ino: u64,
    pub filetype: FileType,
    pub name: Vec<u8>,
}

impl DirectoryEntry {
    pub(super) fn header(&self, name_len: u32) -> [u8; 24] {
        let mut header = [0; 24];
        header[..8].copy_from_slice(&self.next_cookie.to_le_bytes());
        header[8..16].copy_from_slice(&self.ino.to_le_bytes());
        header[16..20].copy_from_slice(&name_len.to_le_bytes());
        header[20] = self.filetype as u8;

        header
    }
}
