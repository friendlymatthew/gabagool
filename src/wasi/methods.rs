use crate::{
    prelude::*,
    wasi::{
        ctx::{FDFLAGS_ALL, OFLAGS_ALL},
        traits::{ClockError, ClockId},
        FdEntry, FileType, OpenOptions, FDFLAGS_DSYNC, FDFLAGS_RSYNC, FDFLAGS_SYNC, FSTFLAGS_ATIM,
        FSTFLAGS_ATIM_NOW, FSTFLAGS_MTIM, FSTFLAGS_MTIM_NOW, LOOKUPFLAGS_SYMLINK_FOLLOW,
        OFLAGS_CREAT, OFLAGS_DIRECTORY, OFLAGS_EXCL, OFLAGS_TRUNC, RIGHTS_FD_DATASYNC,
        RIGHTS_FD_SYNC, RIGHTS_PATH_CREATE_FILE, RIGHTS_PATH_FILESTAT_SET_SIZE,
    },
    Caller, ExternalValue, GuestMemory, HostOutcome, Instance, RawValue, Store,
};

use {PathError, SeekError, SeekFrom, WasiCtx, WasiIOError, WasiResource};

const SUCCESS: i32 = 0;
const AGAIN: i32 = 6;
const BADF: i32 = 8;
const FAULT: i32 = 21;
const INTR: i32 = 27;
const IO: i32 = 29;
const OVERFLOW: i32 = 61;
const NOTCAPABLE: i32 = 76;
const INVAL: i32 = 28;
const NOTSUP: i32 = 58;
const NAMETOOLONG: i32 = 37;
const NOTDIR: i32 = 54;
const ISDIR: i32 = 31;
const SPIPE: i32 = 70;
const ACCES: i32 = 2;
const EXIST: i32 = 20;
const ILSEQ: i32 = 25;
const NOENT: i32 = 44;
const XDEV: i32 = 75;
const NOTEMPTY: i32 = 55;
const MFILE: i32 = 33;

macro_rules! wasi_some {
    ($expr:expr, $errno:expr $(,)?) => {
        match $expr {
            Some(value) => value,
            None => return $crate::wasi::methods::errno($errno),
        }
    };
}

macro_rules! wasi_try {
    ($expr:expr, $errno:expr $(,)?) => {
        match $expr {
            Ok(value) => value,
            Err(_) => return $crate::wasi::methods::errno($errno),
        }
    };
}

pub(super) fn path_symlink<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let target_ptr = args[0].as_pointer();
    let target_len = args[1].as_i32() as u32 as usize;
    let fd = args[2].as_i32() as u32;
    let path_ptr = args[3].as_pointer();
    let path_len = args[4].as_i32() as u32 as usize;

    let (store, instance, ctx) = caller.parts_mut();

    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_symlink() {
        return errno(NOTCAPABLE);
    }

    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let path = match guest_path(memory, path_ptr, path_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    let bytes = wasi_try!(memory.read_bytes(target_ptr, target_len), FAULT);

    let target = wasi_try!(core::str::from_utf8(bytes), ILSEQ);

    if target.is_empty() {
        return errno(NOENT);
    }

    if bytes.contains(&0) {
        return errno(INVAL);
    }

    errno(match entry.resource.symlink(target, path) {
        Ok(()) => SUCCESS,
        Err(error) => path_errno(error),
    })
}

pub(super) fn path_readlink<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let path_ptr = args[1].as_pointer();
    let path_len = args[2].as_i32() as u32 as usize;
    let buf_ptr = args[3].as_pointer();
    let buf_len = args[4].as_i32() as u32 as usize;
    let used_ptr = args[5].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();

    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_readlink() {
        return errno(NOTCAPABLE);
    }

    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.read_bytes(buf_ptr, buf_len), FAULT);
    wasi_try!(memory.read_bytes(used_ptr, 4), FAULT);

    let path = match guest_path(memory, path_ptr, path_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    let target = match entry.resource.read_link(path) {
        Ok(target) => target,
        Err(error) => return errno(path_errno(error)),
    };

    let copied = target.len().min(buf_len);

    wasi_try!(memory.write_bytes(buf_ptr, &target[..copied]), FAULT);
    wasi_try!(memory.write(used_ptr, (copied as u32).to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn path_open<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let lookup = args[1].as_i32() as u32;
    let path_ptr = args[2].as_pointer();
    let path_len = args[3].as_i32() as u32 as usize;
    let oflags = args[4].as_i32() as u32;
    let rights_base = args[5].as_i64() as u64;
    let rights_inheriting = args[6].as_i64() as u64;
    let fdflags = args[7].as_i32() as u32;
    let opened_ptr = args[8].as_pointer();

    if lookup & !LOOKUPFLAGS_SYMLINK_FOLLOW != 0
        || oflags & !u32::from(OFLAGS_ALL) != 0
        || fdflags & !u32::from(FDFLAGS_ALL) != 0
    {
        return errno(INVAL);
    }
    let oflags = oflags as u16;
    let fdflags = fdflags as u16;

    let (store, instance, ctx) = caller.parts_mut();

    let parent = wasi_some!(ctx.descriptors().get(&fd), BADF);

    if !parent.can_open()
        || (rights_base | rights_inheriting) & !parent.rights_inheriting != 0
        || oflags & OFLAGS_CREAT != 0 && parent.rights_base & RIGHTS_PATH_CREATE_FILE == 0
        || oflags & OFLAGS_TRUNC != 0 && parent.rights_base & RIGHTS_PATH_FILESTAT_SET_SIZE == 0
        || fdflags & FDFLAGS_DSYNC != 0
            && parent.rights_base & (RIGHTS_FD_DATASYNC | RIGHTS_FD_SYNC) == 0
        || fdflags & (FDFLAGS_RSYNC | FDFLAGS_SYNC) != 0 && parent.rights_base & RIGHTS_FD_SYNC == 0
    {
        return errno(NOTCAPABLE);
    }

    if !matches!(parent.resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.read_bytes(opened_ptr, 4), FAULT);

    let path = match guest_path(memory, path_ptr, path_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    let mut new_fd = 3u32;

    for &existing in ctx.descriptors().keys().filter(|&&key| key >= 3) {
        if existing != new_fd {
            break;
        }
        new_fd += 1;
        if new_fd > i32::MAX as u32 {
            return errno(MFILE);
        }
    }

    let options = OpenOptions {
        follow_symlink: lookup != 0,
        create: oflags & OFLAGS_CREAT != 0,
        directory: oflags & OFLAGS_DIRECTORY != 0,
        exclusive: oflags & OFLAGS_EXCL != 0,
        truncate: oflags & OFLAGS_TRUNC != 0,
        fdflags,
        rights_base,
        rights_inheriting,
    };

    let parent = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    let resource = match parent.resource.open(path, options) {
        Ok(resource) => resource,
        Err(error) => return errno(path_errno(error)),
    };

    if options.directory && !matches!(resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }

    ctx.descriptors_mut().insert(
        new_fd,
        FdEntry {
            resource,
            flags: fdflags,
            rights_base,
            rights_inheriting,
            preopen_name: None,
        },
    );

    wasi_try!(memory.write(opened_ptr, new_fd.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn path_link<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let old_fd = args[0].as_i32() as u32;
    let flags = args[1].as_i32() as u32;
    let old_ptr = args[2].as_pointer();
    let old_len = args[3].as_i32() as u32 as usize;
    let new_fd = args[4].as_i32() as u32;
    let new_ptr = args[5].as_pointer();
    let new_len = args[6].as_i32() as u32 as usize;

    if flags & !LOOKUPFLAGS_SYMLINK_FOLLOW != 0 {
        return errno(INVAL);
    }

    let (store, instance, ctx) = caller.parts_mut();

    let source = wasi_some!(ctx.descriptors().get(&old_fd), BADF);
    let target = wasi_some!(ctx.descriptors().get(&new_fd), BADF);

    if !source.can_link_source() || !target.can_link_target() {
        return errno(NOTCAPABLE);
    }

    if !matches!(source.resource.file_type(), FileType::Directory)
        || !matches!(target.resource.file_type(), FileType::Directory)
    {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let old_path = match guest_path(memory, old_ptr, old_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    let new_path = match guest_path(memory, new_ptr, new_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    errno(
        match source
            .resource
            .link(old_path, flags != 0, &target.resource, new_path)
        {
            Ok(()) => SUCCESS,
            Err(error) => path_errno(error),
        },
    )
}

pub(super) fn path_rename<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let old_fd = args[0].as_i32() as u32;
    let old_ptr = args[1].as_pointer();
    let old_len = args[2].as_i32() as u32 as usize;
    let new_fd = args[3].as_i32() as u32;
    let new_ptr = args[4].as_pointer();
    let new_len = args[5].as_i32() as u32 as usize;

    let (store, instance, ctx) = caller.parts_mut();

    let source = wasi_some!(ctx.descriptors().get(&old_fd), BADF);
    let target = wasi_some!(ctx.descriptors().get(&new_fd), BADF);

    if !source.can_rename_source() || !target.can_rename_target() {
        return errno(NOTCAPABLE);
    }

    if !matches!(source.resource.file_type(), FileType::Directory)
        || !matches!(target.resource.file_type(), FileType::Directory)
    {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let old_path = match guest_path(memory, old_ptr, old_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    let new_path = match guest_path(memory, new_ptr, new_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    errno(
        match source.resource.rename(old_path, &target.resource, new_path) {
            Ok(()) => SUCCESS,
            Err(error) => path_errno(error),
        },
    )
}

fn valid_timestamp_flags(flags: u32) -> bool {
    let atim = u32::from(FSTFLAGS_ATIM | FSTFLAGS_ATIM_NOW);
    let mtim = u32::from(FSTFLAGS_MTIM | FSTFLAGS_MTIM_NOW);
    flags & !(atim | mtim) == 0 && flags & atim != atim && flags & mtim != mtim
}

fn timestamp_values<R>(
    ctx: &mut WasiCtx<R>,
    atim: u64,
    mtim: u64,
    flags: u32,
) -> core::result::Result<(Option<u64>, Option<u64>), i32> {
    let now = if flags & u32::from(FSTFLAGS_ATIM_NOW | FSTFLAGS_MTIM_NOW) != 0 {
        ctx.clock_mut()
            .now_nanos(ClockId::Realtime, 0)
            .map_err(|error| match error {
                ClockError::Unsupported => INVAL,
                ClockError::Unavailable => IO,
                ClockError::Overflow => OVERFLOW,
            })?
    } else {
        0
    };

    let access = if flags & u32::from(FSTFLAGS_ATIM_NOW) != 0 {
        Some(now)
    } else if flags & u32::from(FSTFLAGS_ATIM) != 0 {
        Some(atim)
    } else {
        None
    };

    let modified = if flags & u32::from(FSTFLAGS_MTIM_NOW) != 0 {
        Some(now)
    } else if flags & u32::from(FSTFLAGS_MTIM) != 0 {
        Some(mtim)
    } else {
        None
    };

    Ok((access, modified))
}

pub(super) fn path_filestat_set_times<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let lookup_flags = args[1].as_i32() as u32;
    let path_ptr = args[2].as_pointer();
    let path_len = args[3].as_i32() as u32 as usize;
    let atim = args[4].as_i64() as u64;
    let mtim = args[5].as_i64() as u64;
    let flags = args[6].as_i32() as u32;

    if lookup_flags & !LOOKUPFLAGS_SYMLINK_FOLLOW != 0 || !valid_timestamp_flags(flags) {
        return errno(INVAL);
    }

    let (store, instance, ctx) = caller.parts_mut();

    let entry = wasi_some!(ctx.descriptors().get(&fd), BADF);

    if !entry.can_path_filestat_set_times() {
        return errno(NOTCAPABLE);
    }

    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let path = match guest_path(memory, path_ptr, path_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    let (access, modified) = match timestamp_values(ctx, atim, mtim, flags) {
        Ok(times) => times,
        Err(code) => return errno(code),
    };

    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    errno(
        match entry
            .resource
            .path_set_times(path, lookup_flags != 0, access, modified)
        {
            Ok(()) => SUCCESS,
            Err(error) => path_errno(error),
        },
    )
}

pub(super) fn path_filestat_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let flags = args[1].as_i32() as u32;
    let path_ptr = args[2].as_pointer();
    let path_len = args[3].as_i32() as u32 as usize;
    let stat_ptr = args[4].as_pointer();

    if flags & !LOOKUPFLAGS_SYMLINK_FOLLOW != 0 {
        return errno(INVAL);
    }

    let (store, instance, ctx) = caller.parts_mut();

    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_path_filestat_get() {
        return errno(NOTCAPABLE);
    }

    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.read_bytes(stat_ptr, 64), FAULT);

    let path = match guest_path(memory, path_ptr, path_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    let stat = match entry.resource.path_filestat(path, flags != 0) {
        Ok(stat) => stat,
        Err(error) => return errno(path_errno(error)),
    };

    wasi_try!(memory.write(stat_ptr, stat.to_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn path_create_directory<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let path_ptr = args[1].as_pointer();
    let path_len = args[2].as_i32() as u32 as usize;

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_create_directory() {
        return errno(NOTCAPABLE);
    }

    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);
    let path = match guest_path(memory, path_ptr, path_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    errno(match entry.resource.create_directory(path) {
        Ok(()) => SUCCESS,
        Err(error) => path_errno(error),
    })
}

pub(super) fn path_remove_directory<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let path_ptr = args[1].as_pointer();
    let path_len = args[2].as_i32() as u32 as usize;

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_remove_directory() {
        return errno(NOTCAPABLE);
    }

    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);
    let path = match guest_path(memory, path_ptr, path_len) {
        Ok(path) => path,
        Err(code) => return errno(code),
    };

    errno(match entry.resource.remove_directory(path) {
        Ok(()) => SUCCESS,
        Err(error) => path_errno(error),
    })
}

fn guest_path(memory: &GuestMemory, ptr: usize, len: usize) -> core::result::Result<&str, i32> {
    let bytes = memory.read_bytes(ptr, len).map_err(|_| FAULT)?;
    let path = core::str::from_utf8(bytes).map_err(|_| ILSEQ)?;
    if path.is_empty() {
        return Err(NOENT);
    }
    if bytes.contains(&0) {
        return Err(INVAL);
    }
    if path.starts_with('/') {
        return Err(NOTCAPABLE);
    }
    // Backend path resolution must additionally enforce capability boundaries.
    Ok(path)
}

const fn path_errno(error: PathError) -> i32 {
    match error {
        PathError::Io => IO,
        PathError::Unsupported => NOTSUP,
        PathError::NotFound => NOENT,
        PathError::AlreadyExists => EXIST,
        PathError::NotDirectory => NOTDIR,
        PathError::PermissionDenied => ACCES,
        PathError::NotCapable => NOTCAPABLE,
        PathError::InvalidInput => INVAL,
        PathError::CrossDevice => XDEV,
        PathError::NotEmpty => NOTEMPTY,
        PathError::IsDirectory => ISDIR,
    }
}

pub(super) fn fd_tell<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let position_ptr = args[1].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_tell() {
        return errno(NOTCAPABLE);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.read_bytes(position_ptr, 8), FAULT);

    let position = match entry.resource.tell() {
        Ok(position) => position,
        Err(error) => {
            return errno(match error {
                SeekError::Unsupported => SPIPE,
                SeekError::InvalidOffset => INVAL,
                SeekError::Overflow => OVERFLOW,
                SeekError::Io => IO,
            })
        }
    };

    wasi_try!(memory.write(position_ptr, position.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_seek<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let offset = args[1].as_i64();
    let whence = args[2].as_i32() as u32;
    let position_ptr = args[3].as_pointer();

    let from = match whence {
        0 if offset >= 0 => SeekFrom::Start(offset as u64),
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        _ => return errno(INVAL),
    };

    let (store, instance, ctx) = caller.parts_mut();

    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    let query_only = whence == 1 && offset == 0;

    if !(entry.can_seek() || query_only && entry.can_tell()) {
        return errno(NOTCAPABLE);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.read_bytes(position_ptr, 8), FAULT);

    let result = if query_only {
        entry.resource.tell()
    } else {
        entry.resource.seek(from)
    };

    let position = match result {
        Ok(position) => position,
        Err(error) => {
            return errno(match error {
                SeekError::Unsupported => SPIPE,
                SeekError::InvalidOffset => INVAL,
                SeekError::Overflow => OVERFLOW,
                SeekError::Io => IO,
            })
        }
    };

    wasi_try!(memory.write(position_ptr, position.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_readdir<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let buf_ptr = args[1].as_pointer();
    let buf_len = args[2].as_i32() as u32 as usize;
    let mut cookie = args[3].as_i64() as u64;
    let used_ptr = args[4].as_pointer();
    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_readdir() {
        return errno(NOTCAPABLE);
    }
    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(NOTDIR);
    }
    let memory = wasi_some!(guest_memory(store, instance), FAULT);
    wasi_try!(memory.read_bytes(used_ptr, 4), FAULT);
    let buffer = wasi_try!(memory.read_bytes_mut(buf_ptr, buf_len), FAULT);
    let mut used = 0usize;

    while used < buffer.len() {
        let item = match entry.resource.directory_entry(cookie) {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(error) => {
                return errno(match error {
                    WasiIOError::Io => IO,
                    WasiIOError::Interrupted => INTR,
                    WasiIOError::WouldBlock => AGAIN,
                    WasiIOError::Unsupported => NOTSUP,
                })
            }
        };
        let name_len = wasi_try!(u32::try_from(item.name.len()), OVERFLOW);
        let header = item.header(name_len);
        let header_len = header.len().min(buffer.len() - used);
        buffer[used..used + header_len].copy_from_slice(&header[..header_len]);
        used += header_len;
        if header_len < header.len() {
            break;
        }
        let copied = item.name.len().min(buffer.len() - used);
        buffer[used..used + copied].copy_from_slice(&item.name[..copied]);
        used += copied;
        if copied < item.name.len() {
            break;
        }
        cookie = item.next_cookie;
    }

    wasi_try!(memory.write(used_ptr, (used as u32).to_le_bytes()), FAULT);
    errno(SUCCESS)
}

pub(super) fn fd_prestat_dir_name<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let path_ptr = args[1].as_pointer();
    let path_len = args[2].as_i32() as u32 as usize;

    let (store, instance, ctx) = caller.parts_mut();

    let entry = wasi_some!(ctx.descriptors().get(&fd), BADF);
    let name = wasi_some!(entry.preopen_name.as_ref(), BADF);

    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(BADF);
    }

    if name.len() > path_len {
        return errno(NAMETOOLONG);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.write_bytes(path_ptr, name), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_prestat_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let addr = args[1].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors().get(&fd), BADF);
    let name = wasi_some!(entry.preopen_name.as_ref(), BADF);

    if !matches!(entry.resource.file_type(), FileType::Directory) {
        return errno(BADF);
    }

    let name_len = wasi_try!(u32::try_from(name.len()), OVERFLOW);

    let mut bytes = [0u8; 8];
    bytes[4..8].copy_from_slice(&name_len.to_le_bytes());

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.write(addr, bytes), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_pread<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let iovs_ptr = args[1].as_pointer();
    let iovs_len = args[2].as_i32() as u32 as usize;
    let offset = args[3].as_i64() as u64;
    let nread_ptr = args[4].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_pread() {
        return errno(NOTCAPABLE);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let buffers = match decode_iovecs(memory, iovs_ptr, iovs_len) {
        Ok(buffers) => buffers,
        Err(code) => return errno(code),
    };

    wasi_try!(memory.read_bytes(nread_ptr, 4), FAULT);

    let capacity: u64 = buffers.iter().map(|&(_, len)| u64::from(len)).sum();
    wasi_some!(offset.checked_add(capacity), OVERFLOW);

    let mut total_read = 0u32;
    for (ptr, len) in buffers {
        let buffer = wasi_try!(memory.read_bytes_mut(ptr as usize, len as usize), FAULT);

        match entry
            .resource
            .read_at(buffer, offset + u64::from(total_read))
        {
            Ok(n) if n <= buffer.len() => {
                total_read += n as u32;
                if n < buffer.len() {
                    break;
                }
            }
            result => {
                if total_read != 0 {
                    break;
                }
                return errno(match result {
                    Err(WasiIOError::Interrupted) => INTR,
                    Err(WasiIOError::WouldBlock) => AGAIN,
                    Err(WasiIOError::Unsupported) => NOTSUP,
                    _ => IO,
                });
            }
        }
    }

    wasi_try!(memory.write(nread_ptr, total_read.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_pwrite<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let iovs_ptr = args[1].as_pointer();
    let iovs_len = args[2].as_i32() as u32 as usize;
    let offset = args[3].as_i64() as u64;
    let nwritten_ptr = args[4].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_pwrite() {
        return errno(NOTCAPABLE);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let buffers = match decode_iovecs(memory, iovs_ptr, iovs_len) {
        Ok(buffers) => buffers,
        Err(code) => return errno(code),
    };

    wasi_try!(memory.read_bytes(nwritten_ptr, 4), FAULT);

    let capacity: u64 = buffers.iter().map(|&(_, len)| u64::from(len)).sum();
    wasi_some!(offset.checked_add(capacity), OVERFLOW);

    let mut total_written = 0u32;
    for (ptr, len) in buffers {
        let buffer = wasi_try!(memory.read_bytes(ptr as usize, len as usize), FAULT);

        match entry
            .resource
            .write_at(buffer, offset + u64::from(total_written))
        {
            Ok(n) if n <= buffer.len() => {
                total_written += n as u32;
                if n < buffer.len() {
                    break;
                }
            }
            result => {
                if total_written != 0 {
                    break;
                }
                return errno(match result {
                    Err(WasiIOError::Interrupted) => INTR,
                    Err(WasiIOError::WouldBlock) => AGAIN,
                    Err(WasiIOError::Unsupported) => NOTSUP,
                    _ => IO,
                });
            }
        }
    }

    wasi_try!(
        memory.write(nwritten_ptr, total_written.to_le_bytes()),
        FAULT
    );

    errno(SUCCESS)
}

pub(super) fn fd_filestat_set_times<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let atim = args[1].as_i64() as u64;
    let mtim = args[2].as_i64() as u64;
    let flags = args[3].as_i32() as u32;

    if !valid_timestamp_flags(flags) {
        return errno(INVAL);
    }

    let ctx = caller.data_mut();
    let entry = wasi_some!(ctx.descriptors().get(&fd), BADF);
    if !entry.can_filestat_set_times() {
        return errno(NOTCAPABLE);
    }

    let (access, modified) = match timestamp_values(ctx, atim, mtim, flags) {
        Ok(times) => times,
        Err(code) => return errno(code),
    };

    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    errno(match entry.resource.set_times(access, modified) {
        Ok(()) => SUCCESS,
        Err(WasiIOError::Io) => IO,
        Err(WasiIOError::Interrupted) => INTR,
        Err(WasiIOError::WouldBlock) => AGAIN,
        Err(WasiIOError::Unsupported) => NOTSUP,
    })
}

pub(super) fn fd_filestat_set_size<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let size = args[1].as_i64() as u64;
    let entry = wasi_some!(caller.data_mut().descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_filestat_set_size() {
        return errno(NOTCAPABLE);
    }

    errno(match entry.resource.set_size(size) {
        Ok(()) => SUCCESS,
        Err(WasiIOError::Io) => IO,
        Err(WasiIOError::Interrupted) => INTR,
        Err(WasiIOError::WouldBlock) => AGAIN,
        Err(WasiIOError::Unsupported) => NOTSUP,
    })
}

pub(super) fn fd_filestat_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let addr = args[1].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_filestat_get() {
        return errno(NOTCAPABLE);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.read_bytes(addr, 64), FAULT);

    let stat = match entry.resource.filestat() {
        Ok(stat) => stat,
        Err(error) => {
            return errno(match error {
                WasiIOError::Io => IO,
                WasiIOError::Interrupted => INTR,
                WasiIOError::WouldBlock => AGAIN,
                WasiIOError::Unsupported => NOTSUP,
            })
        }
    };

    wasi_try!(memory.write(addr, stat.to_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_fdstat_set_flags<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let flags = args[1].as_i32() as u32;

    if flags & !u32::from(FDFLAGS_ALL) != 0 {
        return errno(INVAL);
    }
    let flags = flags as u16;

    let entry = wasi_some!(caller.data_mut().descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_set_flags() {
        return errno(NOTCAPABLE);
    }

    match entry.resource.set_fdflags(flags) {
        Ok(()) => {
            entry.flags = flags;
            errno(SUCCESS)
        }
        Err(error) => errno(match error {
            WasiIOError::Io => IO,
            WasiIOError::Interrupted => INTR,
            WasiIOError::WouldBlock => AGAIN,
            WasiIOError::Unsupported => NOTSUP,
        }),
    }
}

pub(super) fn fd_sync<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;

    let entry = wasi_some!(caller.data_mut().descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_sync() {
        return errno(NOTCAPABLE);
    }

    errno(match entry.resource.sync() {
        Ok(()) => SUCCESS,
        Err(WasiIOError::Io) => IO,
        Err(WasiIOError::Interrupted) => INTR,
        Err(WasiIOError::WouldBlock) => AGAIN,
        Err(WasiIOError::Unsupported) => NOTSUP,
    })
}

pub(super) fn fd_datasync<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let entry = wasi_some!(caller.data_mut().descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_datasync() {
        return errno(NOTCAPABLE);
    }

    errno(match entry.resource.datasync() {
        Ok(()) => SUCCESS,
        Err(WasiIOError::Io) => IO,
        Err(WasiIOError::Interrupted) => INTR,
        Err(WasiIOError::WouldBlock) => AGAIN,
        Err(WasiIOError::Unsupported) => NOTSUP,
    })
}

pub(super) fn fd_advise<R: WasiResource>(
    caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let _offset = args[1].as_i64() as u64;
    let _len = args[2].as_i64() as u64;
    let advice = args[3].as_i32() as u32;

    if advice > 5 {
        return errno(INVAL);
    }

    let entry = wasi_some!(caller.data().descriptors().get(&fd), BADF);
    if !entry.can_advise() {
        return errno(NOTCAPABLE);
    }

    errno(SUCCESS)
}

pub(super) fn fd_fdstat_set_rights<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let base_rights = args[1].as_i64() as u64;
    let inherited_rights = args[2].as_i64() as u64;

    let ctx = caller.data_mut();

    let d = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if (base_rights & !d.rights_base) != 0 || (inherited_rights & !d.rights_inheriting) != 0 {
        return errno(NOTCAPABLE);
    }

    d.rights_base = base_rights;
    d.rights_inheriting = inherited_rights;

    errno(SUCCESS)
}

pub(super) fn clock_res_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let clock_id = args[0].as_i32() as u32;
    let addr = args[1].as_pointer();

    let clock_id = wasi_try!(ClockId::try_from(clock_id), INVAL);

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let nanos = match ctx.clock_mut().resolution_ns(clock_id) {
        Ok(nanos) => nanos.get(),
        Err(error) => {
            return errno(match error {
                ClockError::Unsupported => INVAL,
                ClockError::Unavailable => IO,
                ClockError::Overflow => OVERFLOW,
            })
        }
    };

    wasi_try!(memory.write(addr, nanos.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn clock_time_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let clock_id = args[0].as_i32() as u32;
    let precision_nanos = args[1].as_i64() as u64;
    let guest_addr = args[2].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let clock_id = wasi_try!(ClockId::try_from(clock_id), INVAL);

    let nanos = match ctx.clock_mut().now_nanos(clock_id, precision_nanos) {
        Ok(nanos) => nanos,
        Err(error) => {
            return errno(match error {
                ClockError::Unsupported => INVAL,
                ClockError::Unavailable => IO,
                ClockError::Overflow => OVERFLOW,
            })
        }
    };

    wasi_try!(memory.write(guest_addr, nanos.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn random_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let addr = args[0].as_pointer();
    let len = args[1].as_i32() as u32;

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let buf = wasi_try!(memory.read_bytes_mut(addr, len as usize), FAULT);

    let rand = ctx.random_mut();
    wasi_try!(rand.fill_bytes(buf), IO);

    errno(SUCCESS)
}

pub(super) fn fd_close<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;

    let ctx = caller.data_mut();

    match ctx.descriptors_mut().remove(&fd) {
        Some(_) => errno(SUCCESS),
        None => errno(BADF),
    }
}

pub fn fd_read<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let iovs_ptr = args[1].as_pointer();
    let iovs_len = args[2].as_i32() as u32 as usize;
    let nread_ptr = args[3].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();

    let file_entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !file_entry.can_read() {
        return errno(NOTCAPABLE);
    }

    let input = wasi_some!(file_entry.resource.input_mut(), BADF);
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let buffers = match decode_iovecs(memory, iovs_ptr, iovs_len) {
        Ok(buffers) => buffers,
        Err(code) => return errno(code),
    };

    // first check before consuming input
    wasi_try!(memory.read_bytes(nread_ptr, 4), FAULT);

    let mut total_read = 0u32;

    for (ptr, len) in buffers {
        let buffer = wasi_try!(memory.read_bytes_mut(ptr as usize, len as usize), FAULT);

        match input.read(buffer) {
            Ok(n) if n <= buffer.len() => {
                total_read += n as u32;

                if n < buffer.len() {
                    break;
                }
            }
            result => {
                if total_read != 0 {
                    break;
                }

                return errno(match result {
                    Err(WasiIOError::Interrupted) => INTR,
                    Err(WasiIOError::WouldBlock) => AGAIN,
                    _ => IO,
                });
            }
        }
    }

    wasi_try!(memory.write(nread_ptr, total_read.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_fdstat_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let addr = args[1].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors().get(&fd), BADF);

    let mut buf = [0u8; 24];
    buf[0] = entry.resource.file_type() as u8;
    buf[2..4].copy_from_slice(&entry.flags.to_le_bytes());
    buf[8..16].copy_from_slice(&entry.rights_base.to_le_bytes());
    buf[16..24].copy_from_slice(&entry.rights_inheriting.to_le_bytes());

    let memory = wasi_some!(guest_memory(store, instance), FAULT);
    wasi_try!(memory.write_bytes(addr, &buf), FAULT);

    errno(SUCCESS)
}

pub(super) fn environ_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    string_list_get(
        memory,
        ctx.environment(),
        args[0].as_pointer(),
        args[1].as_i32() as u32,
    )
}

pub(super) fn environ_sizes_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    string_list_sizes_get(
        memory,
        ctx.environment(),
        args[0].as_pointer(),
        args[1].as_pointer(),
    )
}

pub(super) fn args_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    string_list_get(
        memory,
        ctx.args(),
        args[0].as_pointer(),
        args[1].as_i32() as u32,
    )
}

pub(super) fn args_sizes_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    string_list_sizes_get(
        memory,
        ctx.args(),
        args[0].as_pointer(),
        args[1].as_pointer(),
    )
}

pub(super) const fn proc_exit<R: WasiResource>(
    _: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    HostOutcome::Exit(args[0].as_i32() as u32)
}

fn string_list_buffer_len(strings: &[String]) -> Option<u32> {
    strings.iter().try_fold(0u32, |total, string| {
        let len = u32::try_from(string.len()).ok()?;
        total.checked_add(len)?.checked_add(1)
    })
}

fn string_list_sizes_get(
    memory: &mut GuestMemory,
    strings: &[String],
    count_ptr: usize,
    buffer_size_ptr: usize,
) -> HostOutcome {
    let count = wasi_try!(u32::try_from(strings.len()), OVERFLOW);
    let buffer_len = wasi_some!(string_list_buffer_len(strings), OVERFLOW);

    wasi_try!(memory.write(count_ptr, count.to_le_bytes()), FAULT);
    wasi_try!(
        memory.write(buffer_size_ptr, buffer_len.to_le_bytes()),
        FAULT
    );

    errno(SUCCESS)
}

fn string_list_get(
    memory: &mut GuestMemory,
    strings: &[String],
    table_ptr: usize,
    mut buffer_ptr: u32,
) -> HostOutcome {
    let table_len = wasi_some!(strings.len().checked_mul(4), OVERFLOW);
    let buffer_len = wasi_some!(string_list_buffer_len(strings), OVERFLOW);

    wasi_try!(memory.read_bytes(table_ptr, table_len), FAULT);
    wasi_try!(
        memory.read_bytes(buffer_ptr as usize, buffer_len as usize),
        FAULT
    );

    wasi_some!(buffer_ptr.checked_add(buffer_len), OVERFLOW);

    for (index, string) in strings.iter().enumerate() {
        wasi_try!(
            memory.write(table_ptr + index * 4, buffer_ptr.to_le_bytes()),
            FAULT
        );
        wasi_try!(
            memory.write_bytes(buffer_ptr as usize, string.as_bytes()),
            FAULT
        );
        let nul_ptr = buffer_ptr + string.len() as u32;
        wasi_try!(memory.write(nul_ptr as usize, [0]), FAULT);
        buffer_ptr = nul_ptr + 1;
    }

    errno(SUCCESS)
}

pub(super) fn fd_write<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let iovs_ptr = args[1].as_pointer();
    let iovs_len = args[2].as_pointer();
    let nwritten_ptr = args[3].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_write() {
        return errno(NOTCAPABLE);
    }
    let output = wasi_some!(entry.resource.output_mut(), BADF);

    let buffers = match decode_iovecs(memory, iovs_ptr, iovs_len) {
        Ok(buffers) => buffers,
        Err(code) => return errno(code),
    };
    wasi_try!(memory.read_bytes(nwritten_ptr, 4), FAULT);

    let mut written = 0u32;
    for (ptr, len) in buffers {
        let bytes = wasi_try!(memory.read_bytes(ptr as usize, len as usize), FAULT);
        match output.write(bytes) {
            Ok(bytes_written) if bytes_written <= bytes.len() => {
                written += bytes_written as u32;
                if bytes_written < bytes.len() {
                    break;
                }
            }
            result => {
                if written != 0 {
                    break;
                }

                return errno(match result {
                    Err(WasiIOError::Interrupted) => INTR,
                    Err(WasiIOError::WouldBlock) => AGAIN,
                    _ => IO,
                });
            }
        }
    }

    wasi_try!(memory.write(nwritten_ptr, written.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

fn decode_iovecs(
    memory: &GuestMemory,
    table_ptr: usize,
    count: usize,
) -> core::result::Result<Vec<(u32, u32)>, i32> {
    let table_len = count.checked_mul(8).ok_or(FAULT)?;
    let table = memory.read_bytes(table_ptr, table_len).map_err(|_| FAULT)?;
    let mut buffers = Vec::new();
    let mut total = 0u32;

    let (chunks, _) = table.as_chunks::<8>();
    for &[p1, p2, p3, p4, l1, l2, l3, l4] in chunks {
        let ptr = u32::from_le_bytes([p1, p2, p3, p4]);
        let len = u32::from_le_bytes([l1, l2, l3, l4]);
        if len == 0 {
            continue;
        }
        total = total.checked_add(len).ok_or(OVERFLOW)?;
        memory
            .read_bytes(ptr as usize, len as usize)
            .map_err(|_| FAULT)?;
        buffers.push((ptr, len));
    }

    Ok(buffers)
}

fn guest_memory(store: &mut Store, instance: Instance) -> Option<&mut GuestMemory> {
    store
        .exports(instance)
        .iter()
        .find_map(|export| {
            if export.name == "memory" {
                if let ExternalValue::Memory { addr } = export.value {
                    return Some(addr);
                }
            }
            None
        })
        .map(|addr| &mut store.memories[addr].data)
}

fn errno(code: i32) -> HostOutcome {
    HostOutcome::Return(vec![RawValue::from(code)])
}
