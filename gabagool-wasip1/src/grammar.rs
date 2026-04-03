use std::path::PathBuf;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Errno {
    Success = 0,
    TooBig = 1,
    Access = 2,
    AddrInUse = 3,
    AddrNotAvail = 4,
    AfNoSupport = 5,
    Again = 6,
    Already = 7,
    BadF = 8,
    BadMsg = 9,
    Busy = 10,
    Canceled = 11,
    Child = 12,
    ConnAborted = 13,
    ConnRefused = 14,
    ConnReset = 15,
    DeadLk = 16,
    DestAddrReq = 17,
    Dom = 18,
    DQuot = 19,
    Exist = 20,
    Fault = 21,
    FBig = 22,
    HostUnreach = 23,
    IdRm = 24,
    IlSeq = 25,
    InProgress = 26,
    Intr = 27,
    Inval = 28,
    Io = 29,
    IsConn = 30,
    IsDir = 31,
    Loop = 32,
    MFile = 33,
    MLink = 34,
    MsgSize = 35,
    MultiHop = 36,
    NameTooLong = 37,
    NetDown = 38,
    NetReset = 39,
    NetUnreach = 40,
    NFile = 41,
    NoBufs = 42,
    NoDev = 43,
    NoEnt = 44,
    NoExec = 45,
    NoLck = 46,
    NoLink = 47,
    NoMem = 48,
    NoMsg = 49,
    NoProtoOpt = 50,
    NoSpc = 51,
    NoSys = 52,
    NotConn = 53,
    NotDir = 54,
    NotEmpty = 55,
    NotRecoverable = 56,
    NotSock = 57,
    NotSup = 58,
    NoTty = 59,
    Nxio = 60,
    Overflow = 61,
    // #swag
    OwnerDead = 62,
    Perm = 63,
    Pipe = 64,
    Proto = 65,
    ProtoNoSupport = 66,
    ProtoType = 67,
    Range = 68,
    RoFs = 69,
    SPipe = 70,
    Srch = 71,
    Stale = 72,
    TimedOut = 73,
    TxtBsy = 74,
    Xdev = 75,
    NotCapable = 76,
}

#[repr(u32)]
#[derive(Debug)]
pub enum ClockId {
    RealTime = 0,
    Monotonic = 1,
    ProcessCpuTimeId = 2,
    ThreadCpuTimeId = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rights(pub u64);

impl Rights {
    pub const FD_DATASYNC: u64 = 1 << 0;
    pub const FD_READ: u64 = 1 << 1;
    pub const FD_SEEK: u64 = 1 << 2;
    pub const FD_FDSTAT_SET_FLAGS: u64 = 1 << 3;
    pub const FD_SYNC: u64 = 1 << 4;
    pub const FD_TELL: u64 = 1 << 5;
    pub const FD_WRITE: u64 = 1 << 6;
    pub const FD_ADVISE: u64 = 1 << 7;
    pub const FD_ALLOCATE: u64 = 1 << 8;
    pub const PATH_CREATE_DIRECTORY: u64 = 1 << 9;
    pub const PATH_CREATE_FILE: u64 = 1 << 10;
    pub const PATH_LINK_SOURCE: u64 = 1 << 11;
    pub const PATH_LINK_TARGET: u64 = 1 << 12;
    pub const PATH_OPEN: u64 = 1 << 13;
    pub const FD_READDIR: u64 = 1 << 14;
    pub const PATH_READLINK: u64 = 1 << 15;
    pub const PATH_RENAME_SOURCE: u64 = 1 << 16;
    pub const PATH_RENAME_TARGET: u64 = 1 << 17;
    pub const PATH_FILESTAT_GET: u64 = 1 << 18;
    pub const PATH_FILESTAT_SET_SIZE: u64 = 1 << 19;
    pub const PATH_FILESTAT_SET_TIMES: u64 = 1 << 20;
    pub const FD_FILESTAT_GET: u64 = 1 << 21;
    pub const FD_FILESTAT_SET_SIZE: u64 = 1 << 22;
    pub const FD_FILESTAT_SET_TIMES: u64 = 1 << 23;
    pub const PATH_SYMLINK: u64 = 1 << 24;
    pub const PATH_REMOVE_DIRECTORY: u64 = 1 << 25;
    pub const PATH_UNLINK_FILE: u64 = 1 << 26;
    pub const POLL_FD_READWRITE: u64 = 1 << 27;
    pub const SOCK_SHUTDOWN: u64 = 1 << 28;
    pub const SOCK_ACCEPT: u64 = 1 << 29;

    pub const fn fd_datasync(self) -> bool {
        self.0 & (1 << 0) != 0
    }
    pub const fn fd_read(self) -> bool {
        self.0 & (1 << 1) != 0
    }
    pub const fn fd_seek(self) -> bool {
        self.0 & (1 << 2) != 0
    }
    pub const fn fd_fdstat_set_flags(self) -> bool {
        self.0 & (1 << 3) != 0
    }
    pub const fn fd_sync(self) -> bool {
        self.0 & (1 << 4) != 0
    }
    pub const fn fd_tell(self) -> bool {
        self.0 & (1 << 5) != 0
    }
    pub const fn fd_write(self) -> bool {
        self.0 & (1 << 6) != 0
    }
    pub const fn fd_advise(self) -> bool {
        self.0 & (1 << 7) != 0
    }
    pub const fn fd_allocate(self) -> bool {
        self.0 & (1 << 8) != 0
    }
    pub const fn path_create_directory(self) -> bool {
        self.0 & (1 << 9) != 0
    }
    pub const fn path_create_file(self) -> bool {
        self.0 & (1 << 10) != 0
    }
    pub const fn path_link_source(self) -> bool {
        self.0 & (1 << 11) != 0
    }
    pub const fn path_link_target(self) -> bool {
        self.0 & (1 << 12) != 0
    }
    pub const fn path_open(self) -> bool {
        self.0 & (1 << 13) != 0
    }
    pub const fn fd_readdir(self) -> bool {
        self.0 & (1 << 14) != 0
    }
    pub const fn path_readlink(self) -> bool {
        self.0 & (1 << 15) != 0
    }
    pub const fn path_rename_source(self) -> bool {
        self.0 & (1 << 16) != 0
    }
    pub const fn path_rename_target(self) -> bool {
        self.0 & (1 << 17) != 0
    }
    pub const fn path_filestat_get(self) -> bool {
        self.0 & (1 << 18) != 0
    }
    pub const fn path_filestat_set_size(self) -> bool {
        self.0 & (1 << 19) != 0
    }
    pub const fn path_filestat_set_times(self) -> bool {
        self.0 & (1 << 20) != 0
    }
    pub const fn fd_filestat_get(self) -> bool {
        self.0 & (1 << 21) != 0
    }
    pub const fn fd_filestat_set_size(self) -> bool {
        self.0 & (1 << 22) != 0
    }
    pub const fn fd_filestat_set_times(self) -> bool {
        self.0 & (1 << 23) != 0
    }
    pub const fn path_symlink(self) -> bool {
        self.0 & (1 << 24) != 0
    }
    pub const fn path_remove_directory(self) -> bool {
        self.0 & (1 << 25) != 0
    }
    pub const fn path_unlink_file(self) -> bool {
        self.0 & (1 << 26) != 0
    }
    pub const fn poll_fd_readwrite(self) -> bool {
        self.0 & (1 << 27) != 0
    }
    pub const fn sock_shutdown(self) -> bool {
        self.0 & (1 << 28) != 0
    }
    pub const fn sock_accept(self) -> bool {
        self.0 & (1 << 29) != 0
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdFlags(pub u16);

impl FdFlags {
    pub const fn append(self) -> bool {
        self.0 & (1 << 0) != 0
    }
    pub const fn dsync(self) -> bool {
        self.0 & (1 << 1) != 0
    }
    pub const fn nonblock(self) -> bool {
        self.0 & (1 << 2) != 0
    }
    pub const fn rsync(self) -> bool {
        self.0 & (1 << 3) != 0
    }
    pub const fn sync(self) -> bool {
        self.0 & (1 << 4) != 0
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whence {
    Set = 0,
    Cur = 1,
    End = 2,
}

// 8 bytes: alignment 4
/// offset 0: buf (u32, pointer)
/// offset 4: buf_len (u32)
pub const IOVEC_SIZE: u32 = 8;

/// 8 bytes, alignment 4 (same layout as iovec)
pub const CIOVEC_SIZE: u32 = 8;

/// 8 bytes, alignment 4
/// offset 0: tag (u8, 0 = dir)
/// offset 4: u.dir.pr_name_len (u32)
pub const PRESTAT_SIZE: u32 = 8;

/// 24 bytes, alignment 8
/// offset 0:  fs_filetype (u8)
/// offset 2:  fs_flags (u16)
/// offset 8:  fs_rights_base (u64)
/// offset 16: fs_rights_inheriting (u64)
pub const FDSTAT_SIZE: u32 = 24;

/// 64 bytes, alignment 8
/// offset 0:  dev (u64)
/// offset 8:  ino (u64)
/// offset 16: filetype (u8)
/// offset 24: nlink (u64)
/// offset 32: size (u64)
/// offset 40: atim (u64)
/// offset 48: mtim (u64)
/// offset 56: ctim (u64)
pub const FILESTAT_SIZE: u32 = 64;

/// 24 bytes, alignment 8
/// offset 0:  d_next (u64)
/// offset 8:  d_ino (u64)
/// offset 16: d_namlen (u32)
/// offset 20: d_type (u8)
/// (followed by d_namlen bytes of name, not included in DIRENT_SIZE)
pub const DIRENT_SIZE: u32 = 24;

/// 32 bytes, alignment 8
/// offset 0:  userdata (u64)
/// offset 8:  error (u16)
/// offset 10: type (u8)
/// offset 16: fd_readwrite.nbytes (u64)
/// offset 24: fd_readwrite.flags (u16)
pub const EVENT_SIZE: u32 = 32;

/// 48 bytes, alignment 8
///
/// offset 0:  userdata (u64)
/// offset 8:  u.tag (u8, 0=clock, 1=fd_read, 2=fd_write)
/// offset 16: u.clock.id (u32)
/// offset 24: u.clock.timeout (u64)
/// offset 32: u.clock.precision (u64)
/// offset 40: u.clock.flags (u16)
/// or
/// offset 16: u.fd_read/fd_write.file_descriptor (u32)
pub const SUBSCRIPTION_SIZE: u32 = 48;

#[derive(Debug)]
pub enum FdKind {
    Stdin,
    Stdout,
    Stderr,
    File {
        host_path: PathBuf,
        file: Option<std::fs::File>,
    },
    Directory {
        host_path: PathBuf,
    },
    Preopen {
        host_path: PathBuf,
        guest_path: String,
    },
}

#[derive(Debug)]
pub struct FdEntry {
    pub file_type: FileType,
    pub flags: FdFlags,
    pub rights_base: Rights,
    pub rights_inheriting: Rights,
    pub offset: u64,
    pub kind: FdKind,
}

#[derive(Debug)]
pub struct FdTable {
    pub(crate) entries: Vec<Option<FdEntry>>,
}

impl Default for FdTable {
    fn default() -> Self {
        Self {
            entries: vec![
                Some(FdEntry {
                    file_type: FileType::CharacterDevice,
                    flags: FdFlags(0),
                    rights_base: Rights(Rights::FD_READ),
                    rights_inheriting: Rights(0),
                    offset: 0,
                    kind: FdKind::Stdin,
                }),
                Some(FdEntry {
                    file_type: FileType::CharacterDevice,
                    flags: FdFlags(0),
                    rights_base: Rights(Rights::FD_WRITE),
                    rights_inheriting: Rights(0),
                    offset: 0,
                    kind: FdKind::Stdout,
                }),
                Some(FdEntry {
                    file_type: FileType::CharacterDevice,
                    flags: FdFlags(0),
                    rights_base: Rights(Rights::FD_WRITE),
                    rights_inheriting: Rights(0),
                    offset: 0,
                    kind: FdKind::Stderr,
                }),
            ],
        }
    }
}

impl FdTable {
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, fd: u32) -> Result<&FdEntry, Errno> {
        self.entries
            .get(fd as usize)
            .and_then(|e| e.as_ref())
            .ok_or(Errno::BadF)
    }

    pub fn get_mut(&mut self, fd: u32) -> Result<&mut FdEntry, Errno> {
        self.entries
            .get_mut(fd as usize)
            .and_then(|e| e.as_mut())
            .ok_or(Errno::BadF)
    }

    pub fn insert(&mut self, entry: FdEntry) -> u32 {
        for (i, slot) in self.entries.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(entry);
                return i as u32;
            }
        }
        let fd = self.entries.len() as u32;
        self.entries.push(Some(entry));
        fd
    }

    pub fn remove(&mut self, fd: u32) -> Result<FdEntry, Errno> {
        let slot = self.entries.get_mut(fd as usize).ok_or(Errno::BadF)?;
        slot.take().ok_or(Errno::BadF)
    }

    pub fn renumber(&mut self, from: u32, to: u32) -> Result<(), Errno> {
        let entry = self.remove(from)?;
        if let Some(slot) = self.entries.get_mut(to as usize) {
            *slot = Some(entry);
        } else {
            self.entries.resize_with(to as usize + 1, || None);
            self.entries[to as usize] = Some(entry);
        }
        Ok(())
    }
}
