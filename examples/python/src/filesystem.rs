use std::{
    io::{self, Read, Seek},
    path::Path,
    time::UNIX_EPOCH,
};

use cap_fs_ext::{
    FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt, OpenOptionsSyncExt,
};
use cap_std::fs::{Dir, File, Metadata};
use gabagool::wasi::{
    DirectoryEntry, FdEntry, FileType, Filestat, MemoryInput, MemoryResource, OpenOptions,
    PathError, SeekError, SeekFrom, WasiCtx, WasiIOError, WasiInput, WasiOutput, WasiResource,
    FDFLAGS_NONBLOCK, RIGHTS_FD_ADVISE, RIGHTS_FD_DATASYNC, RIGHTS_FD_FDSTAT_SET_FLAGS,
    RIGHTS_FD_FILESTAT_GET, RIGHTS_FD_READ, RIGHTS_FD_READDIR, RIGHTS_FD_SEEK, RIGHTS_FD_SYNC,
    RIGHTS_FD_TELL, RIGHTS_PATH_FILESTAT_GET, RIGHTS_PATH_OPEN,
};

const DIRECTORY_RIGHTS: u64 =
    RIGHTS_PATH_OPEN | RIGHTS_PATH_FILESTAT_GET | RIGHTS_FD_FILESTAT_GET | RIGHTS_FD_READDIR;
const FILE_RIGHTS: u64 = RIGHTS_FD_READ
    | RIGHTS_FD_SEEK
    | RIGHTS_FD_TELL
    | RIGHTS_FD_FILESTAT_GET
    | RIGHTS_FD_ADVISE
    | RIGHTS_FD_FDSTAT_SET_FLAGS
    | RIGHTS_FD_DATASYNC
    | RIGHTS_FD_SYNC;

#[derive(Debug)]
pub struct Directory {
    handle: Dir,
    entries: Option<Vec<(Vec<u8>, Filestat)>>,
}

impl Directory {
    fn new(handle: Dir) -> Self {
        Self {
            handle,
            entries: None,
        }
    }
}

#[derive(Debug)]
pub struct ReadOnlyFile(File);

impl WasiInput for ReadOnlyFile {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, WasiIOError> {
        self.0.read(buffer).map_err(io_error)
    }
}

#[derive(Debug)]
pub enum Resource {
    Stream(MemoryResource),
    Directory(Directory),
    File(ReadOnlyFile),
}

impl WasiResource for Resource {
    fn file_type(&self) -> FileType {
        match self {
            Self::Stream(stream) => stream.file_type(),
            Self::Directory(_) => FileType::Directory,
            Self::File(_) => FileType::RegularFile,
        }
    }

    fn input_mut(&mut self) -> Option<&mut dyn WasiInput> {
        match self {
            Self::Stream(stream) => stream.input_mut(),
            Self::File(file) => Some(file),
            Self::Directory(_) => None,
        }
    }

    fn output_mut(&mut self) -> Option<&mut dyn WasiOutput> {
        match self {
            Self::Stream(stream) => stream.output_mut(),
            _ => None,
        }
    }

    fn filestat(&mut self) -> Result<Filestat, WasiIOError> {
        let metadata = match self {
            Self::Stream(stream) => return stream.filestat(),
            Self::File(file) => file.0.metadata(),
            Self::Directory(dir) => dir.handle.dir_metadata(),
        }
        .map_err(io_error)?;
        Ok(filestat(&metadata))
    }

    fn path_filestat(&mut self, path: &str, follow: bool) -> Result<Filestat, PathError> {
        let Self::Directory(dir) = self else {
            return Err(PathError::NotDirectory);
        };
        let metadata = if follow {
            dir.handle.metadata(path)
        } else {
            dir.handle.symlink_metadata(path)
        }
        .map_err(path_error)?;
        Ok(filestat(&metadata))
    }

    fn open(&mut self, path: &str, options: OpenOptions) -> Result<Self, PathError> {
        let Self::Directory(dir) = self else {
            return Err(PathError::NotDirectory);
        };

        if options.create
            || options.truncate
            || (options.rights_base | options.rights_inheriting) & !(FILE_RIGHTS | DIRECTORY_RIGHTS)
                != 0
        {
            return Err(PathError::NotCapable);
        }

        if options.fdflags & !FDFLAGS_NONBLOCK != 0 {
            return Err(PathError::Unsupported);
        }

        let mut host_options = cap_std::fs::OpenOptions::new();

        host_options
            .read(true)
            .maybe_dir(true)
            .nonblock(true)
            .follow(if options.follow_symlink {
                FollowSymlinks::Yes
            } else {
                FollowSymlinks::No
            });

        let file = dir
            .handle
            .open_with(path, &host_options)
            .map_err(path_error)?;

        let metadata = file.metadata().map_err(path_error)?;

        if metadata.is_dir() {
            Ok(Self::Directory(Directory::new(Dir::from_std_file(
                file.into_std(),
            ))))
        } else if options.directory {
            Err(PathError::NotDirectory)
        } else if metadata.is_file() {
            Ok(Self::File(ReadOnlyFile(file)))
        } else {
            Err(PathError::Unsupported)
        }
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> Result<usize, WasiIOError> {
        let Self::File(file) = self else {
            return Err(WasiIOError::Unsupported);
        };

        #[cfg(not(windows))]
        {
            use cap_std::fs::FileExt;
            file.0.read_at(buffer, offset).map_err(io_error)
        }

        #[cfg(windows)]
        {
            use cap_std::fs::FileExt;
            file.0.seek_read(buffer, offset).map_err(io_error)
        }
    }

    fn seek(&mut self, from: SeekFrom) -> Result<u64, SeekError> {
        let Self::File(file) = self else {
            return Err(SeekError::Unsupported);
        };

        let from = match from {
            SeekFrom::Start(n) => io::SeekFrom::Start(n),
            SeekFrom::Current(n) => io::SeekFrom::Current(n),
            SeekFrom::End(n) => io::SeekFrom::End(n),
        };

        file.0.seek(from).map_err(|error| match error.kind() {
            io::ErrorKind::InvalidInput => SeekError::InvalidOffset,
            _ => SeekError::Io,
        })
    }

    fn set_fdflags(&mut self, flags: u16) -> Result<(), WasiIOError> {
        if matches!(self, Self::File(_)) && flags & !FDFLAGS_NONBLOCK == 0 {
            Ok(())
        } else {
            Err(WasiIOError::Unsupported)
        }
    }

    fn datasync(&mut self) -> Result<(), WasiIOError> {
        if matches!(self, Self::File(_)) {
            Ok(())
        } else {
            Err(WasiIOError::Unsupported)
        }
    }

    fn sync(&mut self) -> Result<(), WasiIOError> {
        self.datasync()
    }

    fn directory_entry(&mut self, cookie: u64) -> Result<Option<DirectoryEntry>, WasiIOError> {
        let Self::Directory(dir) = self else {
            return Err(WasiIOError::Unsupported);
        };

        if cookie == 0 || dir.entries.is_none() {
            let mut entries = Vec::new();

            for entry in dir.handle.entries().map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                let name = entry.file_name().as_encoded_bytes().to_vec();
                entries.push((name, filestat(&entry.metadata().map_err(io_error)?)));
            }

            dir.entries = Some(entries);
        }

        let index = usize::try_from(cookie).map_err(|_| WasiIOError::Io)?;

        Ok(dir
            .entries
            .as_ref()
            .unwrap()
            .get(index)
            .map(|(name, stat)| DirectoryEntry {
                next_cookie: cookie + 1,
                ino: stat.ino,
                filetype: stat.filetype,
                name: name.clone(),
            }))
    }
}

fn filestat(metadata: &Metadata) -> Filestat {
    let nanos = |time: io::Result<cap_std::time::SystemTime>| {
        time.ok()
            .and_then(|time| time.into_std().duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
            .unwrap_or(0)
    };

    let mut stat = Filestat {
        dev: 0,
        ino: 0,
        nlink: 1,
        filetype: if metadata.is_dir() {
            FileType::Directory
        } else if metadata.is_file() {
            FileType::RegularFile
        } else if metadata.file_type().is_symlink() {
            FileType::SymbolicLink
        } else {
            FileType::Unknown
        },
        size: metadata.len(),
        atim: nanos(metadata.accessed()),
        mtim: nanos(metadata.modified()),
        ctim: 0,
    };

    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        stat.dev = metadata.dev();
        stat.ino = metadata.ino();
        stat.nlink = metadata.nlink();
        stat.ctim = u64::try_from(metadata.ctime())
            .ok()
            .and_then(|s| s.checked_mul(1_000_000_000))
            .and_then(|n| n.checked_add(u64::try_from(metadata.ctime_nsec()).ok()?))
            .unwrap_or(0);
    }

    stat
}

fn io_error(error: io::Error) -> WasiIOError {
    match error.kind() {
        io::ErrorKind::Interrupted => WasiIOError::Interrupted,
        io::ErrorKind::WouldBlock => WasiIOError::WouldBlock,
        io::ErrorKind::Unsupported => WasiIOError::Unsupported,
        _ => WasiIOError::Io,
    }
}

fn path_error(error: io::Error) -> PathError {
    match error.kind() {
        io::ErrorKind::NotFound => PathError::NotFound,
        io::ErrorKind::PermissionDenied => PathError::NotCapable,
        io::ErrorKind::AlreadyExists => PathError::AlreadyExists,
        io::ErrorKind::NotADirectory => PathError::NotDirectory,
        io::ErrorKind::IsADirectory => PathError::IsDirectory,
        io::ErrorKind::InvalidInput => PathError::InvalidInput,
        io::ErrorKind::Unsupported => PathError::Unsupported,
        _ => PathError::Io,
    }
}

pub fn context(stdlib: &Path) -> io::Result<WasiCtx<Resource>> {
    let dir = Dir::open_ambient_dir(stdlib, cap_std::ambient_authority())?;

    let mut ctx = WasiCtx::new(
        Resource::Stream(MemoryResource::Input(MemoryInput::new(Vec::new()))),
        Resource::Stream(MemoryResource::Output(Vec::new())),
        Resource::Stream(MemoryResource::Output(Vec::new())),
    );

    ctx.descriptors_mut().insert(
        3,
        FdEntry {
            resource: Resource::Directory(Directory::new(dir)),
            flags: 0,
            rights_base: DIRECTORY_RIGHTS,
            rights_inheriting: DIRECTORY_RIGHTS | FILE_RIGHTS,
            preopen_name: Some(b"/python/lib".to_vec()),
        },
    );

    ctx.environment_mut().extend([
        "PYTHONHOME=/python".into(),
        "PYTHONDONTWRITEBYTECODE=1".into(),
    ]);

    Ok(ctx)
}

pub fn captured_output(ctx: &WasiCtx<Resource>, fd: u32) -> &[u8] {
    match ctx.descriptors().get(&fd).map(|entry| &entry.resource) {
        Some(Resource::Stream(MemoryResource::Output(bytes))) => bytes,
        _ => &[],
    }
}
