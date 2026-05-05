use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::str;
use std::time::{SystemTime, UNIX_EPOCH};

use gabagool::{
    CompositeType, ExecutionState, ExternalValue, FunctionInstance, GuestMemory, ImportDescription,
    Instance, Module, RawValue, Store,
};

struct MemCursor<'a> {
    mem: &'a mut GuestMemory,
    pos: usize,
}

impl<'a> MemCursor<'a> {
    const fn new(mem: &'a mut GuestMemory, pos: usize) -> Self {
        Self { mem, pos }
    }

    const fn align(&mut self, n: usize) {
        self.pos = (self.pos + n - 1) & !(n - 1);
    }

    fn read_u8(&mut self) -> u8 {
        let v = self.mem.read_u8(self.pos);
        self.pos += 1;

        v
    }

    fn read_u16(&mut self) -> u16 {
        let v = self.mem.read_u16(self.pos);
        self.pos += 2;

        v
    }

    fn read_u32(&mut self) -> u32 {
        let v = self.mem.read_u32(self.pos);
        self.pos += 4;

        v
    }

    fn read_u64(&mut self) -> u64 {
        let v = self.mem.read_u64(self.pos);
        self.pos += 8;

        v
    }

    fn write_u8(&mut self, val: u8) {
        self.mem.write_u8(self.pos, val);
        self.pos += 1;
    }

    fn write_u16(&mut self, val: u16) {
        self.mem.write_u16(self.pos, val);
        self.pos += 2;
    }

    fn write_u32(&mut self, val: u32) {
        self.mem.write_u32(self.pos, val);
        self.pos += 4;
    }

    fn write_u64(&mut self, val: u64) {
        self.mem.write_u64(self.pos, val);
        self.pos += 8;
    }

    fn write_bytes(&mut self, data: &[u8]) {
        self.mem.write_bytes(self.pos, data);
        self.pos += data.len();
    }

    fn zero(&mut self, n: usize) {
        self.mem.fill(self.pos, n, 0);
        self.pos += n;
    }
}

use crate::{
    Errno, FdEntry, FdFlags, FdKind, FdTable, FileType, Rights, EVENT_SIZE, SUBSCRIPTION_SIZE,
};

pub enum DispatchResult {
    Values(Vec<RawValue>),
    Exit(u32),
}

#[derive(Debug)]
pub struct WasiCtx {
    pub fd_table: FdTable,
    pub args: Vec<String>,
    pub environ: Vec<(String, String)>,
    pub exit_code: Option<u32>,
    pub stdin_buf: Vec<u8>,
    pub stdout_buf: Vec<u8>,
    pub stderr_buf: Vec<u8>,
    pub clock_nanos: u64,
}

impl Default for WasiCtx {
    fn default() -> Self {
        Self {
            fd_table: FdTable::default(),
            args: Vec::new(),
            environ: Vec::new(),
            exit_code: None,
            stdin_buf: Vec::new(),
            stdout_buf: Vec::new(),
            stderr_buf: Vec::new(),
            clock_nanos: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        }
    }
}

impl WasiCtx {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_args(mut self, args: &[&str]) -> Self {
        self.args = args.iter().map(|s| s.to_string()).collect();

        self
    }

    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.environ.push((key.to_string(), value.to_string()));

        self
    }

    pub fn preopen(mut self, guest_path: &str, host_path: &str) -> Self {
        self.fd_table.insert(FdEntry {
            file_type: FileType::Directory,
            flags: FdFlags(0),
            rights_base: Rights(
                Rights::PATH_OPEN
                    | Rights::PATH_CREATE_DIRECTORY
                    | Rights::PATH_CREATE_FILE
                    | Rights::PATH_LINK_SOURCE
                    | Rights::PATH_LINK_TARGET
                    | Rights::PATH_READLINK
                    | Rights::PATH_RENAME_SOURCE
                    | Rights::PATH_RENAME_TARGET
                    | Rights::PATH_FILESTAT_GET
                    | Rights::PATH_FILESTAT_SET_SIZE
                    | Rights::PATH_FILESTAT_SET_TIMES
                    | Rights::PATH_SYMLINK
                    | Rights::PATH_REMOVE_DIRECTORY
                    | Rights::PATH_UNLINK_FILE
                    | Rights::FD_READDIR
                    | Rights::FD_FILESTAT_GET,
            ),
            rights_inheriting: Rights(
                Rights::FD_READ
                    | Rights::FD_WRITE
                    | Rights::FD_SEEK
                    | Rights::FD_TELL
                    | Rights::FD_SYNC
                    | Rights::FD_DATASYNC
                    | Rights::FD_FDSTAT_SET_FLAGS
                    | Rights::FD_ALLOCATE
                    | Rights::FD_FILESTAT_GET
                    | Rights::FD_FILESTAT_SET_SIZE
                    | Rights::FD_FILESTAT_SET_TIMES
                    | Rights::FD_ADVISE,
            ),
            offset: 0,
            kind: FdKind::Preopen {
                host_path: PathBuf::from(host_path),
                guest_path: guest_path.to_string(),
            },
        });

        self
    }

    pub fn imports(&self, store: &mut Store, module: &Module) -> Vec<ExternalValue> {
        let mut externals = Vec::new();

        for import in module.import_declarations() {
            if import.module != "wasi_snapshot_preview1" {
                continue;
            }

            match &import.description {
                ImportDescription::Func(type_idx) => {
                    let function_type = match &module.types()[*type_idx as usize].composite_type {
                        CompositeType::Func(ft) => ft.clone(),
                        _ => panic!("expected function"),
                    };
                    let addr = store.functions.len();

                    store.functions.push(FunctionInstance::Host {
                        function_type,
                        module_name: import.module.clone(),
                        function_name: import.name.clone(),
                    });

                    externals.push(ExternalValue::Function { addr });
                }
                _ => panic!("expected function"),
            }
        }

        externals
    }

    pub fn dispatch(
        &mut self,
        store: &mut Store,
        func_name: &str,
        args: &[RawValue],
    ) -> DispatchResult {
        let errno = match func_name {
            "fd_read" => self.fd_read(store, args),
            "fd_pread" => self.fd_pread(store, args),
            "fd_write" => self.fd_write(store, args),
            "fd_pwrite" => self.fd_pwrite(store, args),
            "fd_close" => self.fd_close(args),
            "fd_seek" => self.fd_seek(store, args),
            "fd_tell" => self.fd_tell(store, args),
            "fd_sync" => self.fd_sync(args),
            "fd_datasync" => self.fd_sync(args),
            "fd_fdstat_get" => self.fd_fdstat_get(store, args),
            "fd_fdstat_set_flags" => self.fd_fdstat_set_flags(args),
            "fd_filestat_get" => self.fd_filestat_get(store, args),
            "fd_prestat_get" => self.fd_prestat_get(store, args),
            "fd_prestat_dir_name" => self.fd_prestat_dir_name(store, args),
            "fd_readdir" => self.fd_readdir(store, args),
            "fd_renumber" => {
                let from = args[0].as_i32() as u32;
                let to = args[1].as_i32() as u32;
                match self.fd_table.renumber(from, to) {
                    Ok(()) => Errno::Success,
                    Err(e) => e,
                }
            }
            "fd_fdstat_set_rights" => self.fd_fdstat_set_rights(args),
            "fd_filestat_set_size" => self.fd_filestat_set_size(args),
            "fd_filestat_set_times" => self.fd_filestat_set_times(args),
            "fd_advise" => Errno::Success,
            "fd_allocate" => Errno::Success,
            "path_open" => self.path_open(store, args),
            "path_create_directory" => self.path_create_directory(store, args),
            "path_remove_directory" => self.path_remove_directory(store, args),
            "path_unlink_file" => self.path_unlink_file(store, args),
            "path_rename" => self.path_rename(store, args),
            "path_symlink" => self.path_symlink(store, args),
            "path_link" => self.path_link(store, args),
            "path_readlink" => self.path_readlink(store, args),
            "path_filestat_get" => self.path_filestat_get(store, args),
            "path_filestat_set_times" => self.path_filestat_set_times(store, args),
            "args_sizes_get" => self.args_sizes_get(store, args),
            "args_get" => self.args_get(store, args),
            "environ_sizes_get" => self.environ_sizes_get(store, args),
            "environ_get" => self.environ_get(store, args),
            "clock_time_get" => self.clock_time_get(store, args),
            "clock_res_get" => self.clock_res_get(store, args),
            "random_get" => self.random_get(store, args),
            "poll_oneoff" => self.poll_oneoff(store, args),
            "sched_yield" => Errno::Success,
            "proc_raise" => Errno::NoSys,
            "sock_accept" => Errno::NoSys,
            "sock_recv" => Errno::NoSys,
            "sock_send" => Errno::NoSys,
            "sock_shutdown" => Errno::NoSys,
            "proc_exit" => {
                self.exit_code = Some(args[0].as_i32() as u32);
                return DispatchResult::Exit(args[0].as_i32() as u32);
            }
            _ => Errno::NoSys,
        };

        DispatchResult::Values(vec![RawValue::from(errno as i32)])
    }

    pub fn run(&mut self, store: &mut Store, instance: Instance) -> gabagool::Result<u32> {
        let mut state = store.invoke::<[RawValue; 0]>(instance, "_start", [])?;

        loop {
            match state {
                ExecutionState::Completed(_) => return Ok(self.exit_code.unwrap_or(0)),
                ExecutionState::FuelExhausted => return Ok(self.exit_code.unwrap_or(0)),
                ExecutionState::Suspended {
                    func_name, args, ..
                } => match self.dispatch(store, &func_name, &args) {
                    DispatchResult::Values(results) => {
                        state = store.resume_with(&results)?;
                    }
                    DispatchResult::Exit(code) => return Ok(code),
                },
            }
        }
    }

    fn write_filestat(
        mem: &mut GuestMemory,
        ptr: usize,
        filetype: FileType,
        nlink: u64,
        size: u64,
    ) {
        let mut c = MemCursor::new(mem, ptr);
        c.write_u64(0);
        c.write_u64(0);
        c.write_u8(filetype as u8);
        c.align(8);
        c.write_u64(nlink);
        c.write_u64(size);
        c.write_u64(0);
        c.write_u64(0);
        c.write_u64(0);
    }

    fn dir_host_path(&self, fd: u32) -> Result<PathBuf, Errno> {
        let entry = self.fd_table.get(fd)?;

        match &entry.kind {
            FdKind::Preopen { host_path, .. } => Ok(host_path.clone()),
            FdKind::Directory { host_path } => Ok(host_path.clone()),
            _ => Err(Errno::NotDir),
        }
    }

    fn resolve_guest_path(
        &self,
        store: &Store,
        dirfd: u32,
        path_ptr: usize,
        path_len: usize,
    ) -> Result<PathBuf, Errno> {
        let mem = &store.memories[0].data;
        let guest_path =
            str::from_utf8(mem.read_bytes(path_ptr, path_len)).map_err(|_| Errno::Inval)?;

        let dir = self.dir_host_path(dirfd)?.join(guest_path);

        Ok(dir)
    }

    fn fd_read(&mut self, store: &mut Store, args: &[RawValue]) -> Errno {
        let [fd, iovs, iovs_len, nread_ptr] = args else {
            panic!("invalid argument");
        };

        self.fd_read_impl(
            store,
            fd.as_i32() as u32,
            iovs.as_i32() as u32,
            iovs_len.as_i32() as u32,
            nread_ptr.as_i32() as u32,
            None,
        )
    }

    fn fd_pread(&mut self, store: &mut Store, args: &[RawValue]) -> Errno {
        let [fd, iovs, iovs_len, offset, nread_ptr] = args else {
            panic!("invalid argument");
        };

        self.fd_read_impl(
            store,
            fd.as_i32() as u32,
            iovs.as_i32() as u32,
            iovs_len.as_i32() as u32,
            nread_ptr.as_i32() as u32,
            Some(offset.as_i64() as u64),
        )
    }

    fn fd_read_impl(
        &mut self,
        store: &mut Store,
        fd: u32,
        iovs: u32,
        iovs_len: u32,
        nread_ptr: u32,
        offset: Option<u64>,
    ) -> Errno {
        let entry = match self.fd_table.get(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !entry.rights_base.fd_read() {
            return Errno::NotCapable;
        }

        let mem = &mut store.memories[0].data;
        let mut iov_entries = Vec::with_capacity(iovs_len as usize);

        for i in 0..iovs_len {
            let mut c = MemCursor::new(mem, (iovs + i * 8) as usize);
            iov_entries.push((c.read_u32() as usize, c.read_u32() as usize));
        }

        let mut total_read = 0u32;

        match &mut self.fd_table.get_mut(fd).unwrap().kind {
            FdKind::Stdin => {
                if offset.is_some() {
                    return Errno::SPipe;
                }

                for (buf_ptr, buf_len) in &iov_entries {
                    let available = self.stdin_buf.len().min(*buf_len);
                    if available == 0 {
                        break;
                    }

                    let drained = self.stdin_buf.drain(..available).collect::<Vec<_>>();
                    store.memories[0].data.write_bytes(*buf_ptr, &drained);
                    total_read += available as u32;
                }
            }
            FdKind::File { file: Some(f), .. } => {
                let original_pos = offset.map(|pos| {
                    let orig = f.stream_position().unwrap_or(0);
                    let _ = f.seek(SeekFrom::Start(pos));

                    orig
                });

                for (buf_ptr, buf_len) in &iov_entries {
                    let mut tmp = vec![0u8; *buf_len];

                    match f.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => {
                            store.memories[0].data.write_bytes(*buf_ptr, &tmp[..n]);
                            total_read += n as u32;
                        }
                        Err(_) => return Errno::Io,
                    }
                }

                if let Some(orig) = original_pos {
                    let _ = f.seek(SeekFrom::Start(orig));
                }
            }
            _ => return Errno::BadF,
        }

        store.memories[0]
            .data
            .write_u32(nread_ptr as usize, total_read);

        Errno::Success
    }

    fn fd_write(&mut self, store: &mut Store, args: &[RawValue]) -> Errno {
        let [fd, iovs, iovs_len, nwritten_ptr] = args else {
            panic!("invalid argument");
        };

        self.fd_write_impl(
            store,
            fd.as_i32() as u32,
            iovs.as_i32() as u32,
            iovs_len.as_i32() as u32,
            nwritten_ptr.as_i32() as u32,
            None,
        )
    }

    fn fd_pwrite(&mut self, store: &mut Store, args: &[RawValue]) -> Errno {
        let [fd, iovs, iovs_len, offset, nwritten_ptr] = args else {
            panic!("invalid argument");
        };

        self.fd_write_impl(
            store,
            fd.as_i32() as u32,
            iovs.as_i32() as u32,
            iovs_len.as_i32() as u32,
            nwritten_ptr.as_i32() as u32,
            Some(offset.as_i64() as u64),
        )
    }

    fn fd_write_impl(
        &mut self,
        store: &mut Store,
        fd: u32,
        iovs: u32,
        iovs_len: u32,
        nwritten_ptr: u32,
        offset: Option<u64>,
    ) -> Errno {
        let entry = match self.fd_table.get(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !entry.rights_base.fd_write() {
            return Errno::NotCapable;
        }

        let mem = &mut store.memories[0].data;
        let mut total_written = 0u32;
        let mut bufs = Vec::with_capacity(iovs_len as usize);

        for i in 0..iovs_len {
            let mut c = MemCursor::new(mem, (iovs + i * 8) as usize);
            let buf_ptr = c.read_u32() as usize;
            let buf_len = c.read_u32() as usize;

            bufs.push(mem.read_bytes(buf_ptr, buf_len).to_vec());
            total_written += buf_len as u32;
        }

        match &mut self.fd_table.get_mut(fd).unwrap().kind {
            FdKind::Stdout => {
                if offset.is_some() {
                    return Errno::SPipe;
                }

                for buf in &bufs {
                    self.stdout_buf.extend_from_slice(buf);
                    let _ = std::io::stdout().lock().write_all(buf);
                }
            }
            FdKind::Stderr => {
                if offset.is_some() {
                    return Errno::SPipe;
                }

                for buf in &bufs {
                    self.stderr_buf.extend_from_slice(buf);
                    let _ = std::io::stderr().lock().write_all(buf);
                }
            }
            FdKind::File { file: Some(f), .. } => {
                let original_pos = offset.map(|pos| {
                    let orig = f.stream_position().unwrap_or(0);
                    let _ = f.seek(SeekFrom::Start(pos));

                    orig
                });

                for buf in &bufs {
                    if f.write_all(buf).is_err() {
                        return Errno::Io;
                    }
                }

                if let Some(orig) = original_pos {
                    let _ = f.seek(SeekFrom::Start(orig));
                }
            }
            _ => return Errno::BadF,
        }

        store.memories[0]
            .data
            .write_u32(nwritten_ptr as usize, total_written);

        Errno::Success
    }

    fn fd_readdir(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let [fd, buf, buf_len, cookie, bufused_ptr] = args else {
            panic!("invalid argument");
        };

        let fd = fd.as_i32() as u32;
        let buf = buf.as_i32() as usize;
        let buf_len = buf_len.as_i32() as usize;
        let cookie = cookie.as_i64() as u64;
        let bufused_ptr = bufused_ptr.as_i32() as usize;

        let host_path = match self.dir_host_path(fd) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let entries = match std::fs::read_dir(&host_path) {
            Ok(rd) => rd,
            Err(_) => return Errno::Io,
        };

        let mem = &mut store.memories[0].data;
        let mut offset = 0;

        for (i, entry) in entries.enumerate() {
            if (i as u64) < cookie {
                continue;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let name = entry.file_name();
            let name_bytes = name.as_encoded_bytes();
            let entry_size = 24 + name_bytes.len();

            let d_next = (i + 1) as u64;
            let d_type = if entry.path().is_dir() {
                FileType::Directory as u8
            } else if entry.path().is_symlink() {
                FileType::SymbolicLink as u8
            } else {
                FileType::RegularFile as u8
            };

            let remaining = buf_len - offset;
            if remaining == 0 {
                break;
            }

            if entry_size <= remaining {
                let mut c = MemCursor::new(mem, buf + offset);
                c.write_u64(d_next);
                c.write_u64(0);
                c.write_u32(name_bytes.len() as u32);
                c.write_u8(d_type);
                c.zero(3);
                c.write_bytes(name_bytes);
                offset += entry_size;
            } else {
                let mut tmp = vec![0u8; entry_size];
                tmp[0..8].copy_from_slice(&d_next.to_le_bytes());
                tmp[16..20].copy_from_slice(&(name_bytes.len() as u32).to_le_bytes());
                tmp[20] = d_type;
                tmp[24..].copy_from_slice(name_bytes);
                mem.write_bytes(buf + offset, &tmp[..remaining]);
                offset += remaining;
                break;
            }
        }

        mem.write_u32(bufused_ptr, offset as u32);

        Errno::Success
    }

    fn fd_prestat_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let prestat_ptr = args[1].as_i32() as u32;

        let entry = match self.fd_table.get(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        let guest_path = match &entry.kind {
            FdKind::Preopen { guest_path, .. } => guest_path,
            _ => return Errno::BadF,
        };

        let mut c = MemCursor::new(&mut store.memories[0].data, prestat_ptr as usize);
        c.write_u8(0);
        c.zero(3);
        c.write_u32(guest_path.len() as u32);

        Errno::Success
    }

    fn fd_prestat_dir_name(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let path_ptr = args[1].as_i32() as u32;
        let path_len = args[2].as_i32() as u32;

        let entry = match self.fd_table.get(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        let guest_path = match &entry.kind {
            FdKind::Preopen { guest_path, .. } => guest_path,
            _ => return Errno::BadF,
        };

        let bytes = guest_path.as_bytes();
        let len = bytes.len().min(path_len as usize);

        let mem = &mut store.memories[0].data;
        let ptr = path_ptr as usize;
        mem.write_bytes(ptr, &bytes[..len]);

        Errno::Success
    }

    fn args_sizes_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let argc_ptr = args[0].as_i32() as usize;
        let buf_size_ptr = args[1].as_i32() as usize;

        let argc = self.args.len() as u32;
        let buf_size = self.args.iter().map(|a| a.len() as u32 + 1).sum::<u32>();

        let mem = &mut store.memories[0].data;
        mem.write_u32(argc_ptr, argc);
        mem.write_u32(buf_size_ptr, buf_size);

        Errno::Success
    }

    fn args_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let argv_ptr = args[0].as_i32() as usize;
        let argv_buf_ptr = args[1].as_i32() as usize;

        let mem = &mut store.memories[0].data;
        let mut buf_offset = argv_buf_ptr;

        for (i, arg) in self.args.iter().enumerate() {
            let bytes = arg.as_bytes();

            mem.write_u32(argv_ptr + i * 4, buf_offset as u32);

            mem.write_bytes(buf_offset, bytes);

            mem.write_u8(buf_offset + bytes.len(), 0);
            buf_offset += bytes.len() + 1;
        }

        Errno::Success
    }

    fn environ_sizes_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let count_ptr = args[0].as_i32() as usize;
        let buf_size_ptr = args[1].as_i32() as usize;

        let count = self.environ.len() as u32;
        let buf_size = self
            .environ
            .iter()
            .map(|(k, v)| k.len() as u32 + 1 + v.len() as u32 + 1)
            .sum::<u32>();

        let mem = &mut store.memories[0].data;
        mem.write_u32(count_ptr, count);
        mem.write_u32(buf_size_ptr, buf_size);

        Errno::Success
    }

    fn environ_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let environ_ptr = args[0].as_i32() as usize;
        let environ_buf_ptr = args[1].as_i32() as usize;

        let mem = &mut store.memories[0].data;
        let mut buf_offset = environ_buf_ptr;

        for (i, (key, val)) in self.environ.iter().enumerate() {
            mem.write_u32(environ_ptr + i * 4, buf_offset as u32);

            let entry = format!("{key}={val}");
            let bytes = entry.as_bytes();

            mem.write_bytes(buf_offset, bytes);
            mem.write_u8(buf_offset + bytes.len(), 0);

            buf_offset += bytes.len() + 1;
        }

        Errno::Success
    }

    fn clock_time_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let clock_id = args[0].as_i32() as u32;
        let _precision = args[1].as_i64() as u64;
        let timestamp_ptr = args[2].as_i32() as usize;

        let nanos = match clock_id {
            0 | 1 => self.clock_nanos,
            _ => return Errno::Inval,
        };

        let mem = &mut store.memories[0].data;
        mem.write_u64(timestamp_ptr, nanos);

        Errno::Success
    }

    fn clock_res_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let clock_id = args[0].as_i32() as u32;
        let resolution_ptr = args[1].as_i32() as usize;

        let res = match clock_id {
            0 => 1_000u64,
            1 => 1,
            _ => return Errno::Inval,
        };

        let mem = &mut store.memories[0].data;
        mem.write_u64(resolution_ptr, res);

        Errno::Success
    }

    fn poll_oneoff(&mut self, store: &mut Store, args: &[RawValue]) -> Errno {
        let in_ptr = args[0].as_i32() as usize;
        let out_ptr = args[1].as_i32() as usize;
        let nsubscriptions = args[2].as_i32() as u32;
        let nevents_ptr = args[3].as_i32() as usize;

        let mem = &mut store.memories[0].data;
        let mut nevents = 0u32;

        for i in 0..nsubscriptions {
            let sub_base = in_ptr + (i * SUBSCRIPTION_SIZE) as usize;

            let mut r = MemCursor::new(mem, sub_base);

            let userdata = r.read_u64();
            let tag = r.read_u8();
            r.align(8);
            let _clock_id = r.read_u32();
            r.align(8);
            let timeout = r.read_u64();
            let _precision = r.read_u64();
            let flags = r.read_u16();

            let evt_base = out_ptr + (nevents * EVENT_SIZE) as usize;
            let mut w = MemCursor::new(mem, evt_base);

            match tag {
                0 => {
                    if flags & 1 == 0 {
                        self.clock_nanos += timeout;
                    } else if timeout > self.clock_nanos {
                        self.clock_nanos = timeout;
                    }

                    w.write_u64(userdata);
                    w.write_u16(0);
                    w.write_u8(0);
                    nevents += 1;
                }
                1 | 2 => {
                    w.write_u64(userdata);
                    w.write_u16(0);
                    w.write_u8(tag);
                    w.align(8);
                    w.write_u64(0);
                    w.write_u16(0);
                    nevents += 1;
                }
                _ => {
                    w.write_u64(userdata);
                    w.write_u16(Errno::Inval as u16);
                    w.write_u8(tag);
                    nevents += 1;
                }
            }
        }

        store.memories[0].data.write_u32(nevents_ptr, nevents);

        Errno::Success
    }

    fn random_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let buf_ptr = args[0].as_i32() as usize;
        let buf_len = args[1].as_i32() as usize;

        let mem = &mut store.memories[0].data;

        let mut seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut random_bytes = Vec::with_capacity(buf_len);
        for _ in 0..buf_len {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            random_bytes.push(seed as u8);
        }
        mem.write_bytes(buf_ptr, &random_bytes);

        Errno::Success
    }

    fn fd_close(&mut self, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        match self.fd_table.remove(fd) {
            Ok(_) => Errno::Success,
            Err(e) => e,
        }
    }

    fn fd_seek(&mut self, store: &mut Store, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let offset = args[1].as_i64();
        let whence = args[2].as_i32() as u8;
        let newoffset_ptr = args[3].as_i32() as usize;

        let entry = match self.fd_table.get_mut(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !entry.rights_base.fd_seek() {
            return Errno::NotCapable;
        }

        match &mut entry.kind {
            FdKind::File { file: Some(f), .. } => {
                let pos = match whence {
                    0 => SeekFrom::Start(offset as u64),
                    1 => SeekFrom::Current(offset),
                    2 => SeekFrom::End(offset),
                    _ => return Errno::Inval,
                };
                match f.seek(pos) {
                    Ok(new_pos) => {
                        entry.offset = new_pos;

                        let mem = &mut store.memories[0].data;
                        mem.write_u64(newoffset_ptr, new_pos);

                        Errno::Success
                    }
                    Err(_) => Errno::Io,
                }
            }
            _ => {
                let new_pos = match whence {
                    0 => offset as u64,
                    1 => (entry.offset as i64 + offset) as u64,
                    2 => return Errno::Inval,
                    _ => return Errno::Inval,
                };

                entry.offset = new_pos;

                let mem = &mut store.memories[0].data;
                mem.write_u64(newoffset_ptr, new_pos);

                Errno::Success
            }
        }
    }

    fn fd_tell(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let offset_ptr = args[1].as_i32() as usize;

        let entry = match self.fd_table.get(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !entry.rights_base.fd_tell() {
            return Errno::NotCapable;
        }

        let mem = &mut store.memories[0].data;
        mem.write_u64(offset_ptr, entry.offset);

        Errno::Success
    }

    fn fd_sync(&mut self, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;

        let entry = match self.fd_table.get_mut(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        match &mut entry.kind {
            FdKind::File { file: Some(f), .. } => {
                if f.sync_all().is_err() {
                    return Errno::Io;
                }
                Errno::Success
            }
            _ => Errno::Success,
        }
    }

    fn fd_fdstat_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let buf_ptr = args[1].as_i32() as usize;

        let entry = match self.fd_table.get(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        let mut c = MemCursor::new(&mut store.memories[0].data, buf_ptr);
        c.write_u8(entry.file_type as u8);
        c.zero(1);
        c.write_u16(entry.flags.0);
        c.zero(4);
        c.write_u64(entry.rights_base.0);
        c.write_u64(entry.rights_inheriting.0);

        Errno::Success
    }

    fn fd_fdstat_set_flags(&mut self, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let flags = args[1].as_i32() as u16;

        let entry = match self.fd_table.get_mut(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        entry.flags = FdFlags(flags);
        Errno::Success
    }

    fn fd_fdstat_set_rights(&mut self, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let rights_base = args[1].as_i64() as u64;
        let rights_inheriting = args[2].as_i64() as u64;

        let entry = match self.fd_table.get_mut(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if rights_base & !entry.rights_base.0 != 0 {
            return Errno::NotCapable;
        }

        if rights_inheriting & !entry.rights_inheriting.0 != 0 {
            return Errno::NotCapable;
        }

        entry.rights_base = Rights(rights_base);
        entry.rights_inheriting = Rights(rights_inheriting);
        Errno::Success
    }

    fn fd_filestat_set_size(&mut self, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let size = args[1].as_i64() as u64;

        let entry = match self.fd_table.get_mut(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        match &entry.kind {
            FdKind::File { file: Some(f), .. } => {
                if f.set_len(size).is_err() {
                    return Errno::Io;
                }
                Errno::Success
            }
            _ => Errno::BadF,
        }
    }

    fn fd_filestat_set_times(&self, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let _atim = args[1].as_i64() as u64;
        let _mtim = args[2].as_i64() as u64;
        let _fst_flags = args[3].as_i32() as u16;

        match self.fd_table.get(fd) {
            Ok(_) => Errno::Success,
            Err(e) => e,
        }
    }

    fn fd_filestat_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let buf_ptr = args[1].as_i32() as usize;

        let entry = match self.fd_table.get(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        let (nlink, size) = match &entry.kind {
            FdKind::File { file: Some(f), .. } => {
                let m = f.metadata().ok();
                (1, m.map_or(0, |m| m.len()))
            }
            FdKind::File { host_path, .. } => {
                let m = std::fs::metadata(host_path).ok();
                (1, m.map_or(0, |m| m.len()))
            }
            FdKind::Directory { host_path } | FdKind::Preopen { host_path, .. } => {
                let has = std::fs::metadata(host_path).is_ok();
                (if has { 1 } else { 0 }, 0)
            }
            _ => (0, 0),
        };

        Self::write_filestat(
            &mut store.memories[0].data,
            buf_ptr,
            entry.file_type,
            nlink,
            size,
        );
        Errno::Success
    }

    fn path_open(&mut self, store: &mut Store, args: &[RawValue]) -> Errno {
        let dirfd = args[0].as_i32() as u32;
        let _dirflags = args[1].as_i32() as u32;
        let path_ptr = args[2].as_i32() as usize;
        let path_len = args[3].as_i32() as usize;
        let oflags = args[4].as_i32() as u16;
        let rights_base = args[5].as_i64() as u64;
        let rights_inheriting = args[6].as_i64() as u64;
        let fdflags = args[7].as_i32() as u16;
        let fd_out_ptr = args[8].as_i32() as usize;

        let dir_entry = match self.fd_table.get(dirfd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !dir_entry.rights_base.path_open() {
            return Errno::NotCapable;
        }

        let host_dir = match &dir_entry.kind {
            FdKind::Preopen { host_path, .. } => host_path.clone(),
            FdKind::Directory { host_path } => host_path.clone(),
            _ => return Errno::NotDir,
        };

        let mem = &store.memories[0].data;

        let guest_path = match str::from_utf8(mem.read_bytes(path_ptr, path_len)) {
            Ok(s) => s,
            Err(_) => return Errno::Inval,
        };

        let host_path = host_dir.join(guest_path);

        let oflags_creat = oflags & 1 != 0;
        let oflags_directory = oflags & 2 != 0;
        let oflags_excl = oflags & 4 != 0;
        let oflags_trunc = oflags & 8 != 0;

        if oflags_directory {
            if !host_path.is_dir() {
                return Errno::NotDir;
            }

            let new_fd = self.fd_table.insert(FdEntry {
                file_type: FileType::Directory,
                flags: FdFlags(fdflags),
                rights_base: Rights(rights_base),
                rights_inheriting: Rights(rights_inheriting),
                offset: 0,
                kind: FdKind::Directory { host_path },
            });

            let mem = &mut store.memories[0].data;
            mem.write_u32(fd_out_ptr, new_fd);

            return Errno::Success;
        }

        let mut open_opts = std::fs::OpenOptions::new();

        let rights_base = Rights(rights_base);
        let fdflags = FdFlags(fdflags);

        if rights_base.fd_read() {
            open_opts.read(true);
        }
        if rights_base.fd_write() {
            open_opts.write(true);
        }
        if oflags_creat {
            open_opts.create(true);
        }
        if oflags_excl {
            open_opts.create_new(true);
        }
        if oflags_trunc {
            open_opts.truncate(true);
        }
        if fdflags.append() {
            open_opts.append(true);
        }

        let file = match open_opts.open(&host_path) {
            Ok(f) => f,
            Err(e) => {
                return match e.kind() {
                    std::io::ErrorKind::NotFound => Errno::NoEnt,
                    std::io::ErrorKind::PermissionDenied => Errno::Access,
                    std::io::ErrorKind::AlreadyExists => Errno::Exist,
                    _ => Errno::Io,
                };
            }
        };

        let new_fd = self.fd_table.insert(FdEntry {
            file_type: FileType::RegularFile,
            flags: fdflags,
            rights_base,
            rights_inheriting: Rights(rights_inheriting),
            offset: 0,
            kind: FdKind::File {
                host_path,
                file: Some(file),
            },
        });

        let mem = &mut store.memories[0].data;
        mem.write_u32(fd_out_ptr, new_fd);

        Errno::Success
    }

    fn path_create_directory(&self, store: &Store, args: &[RawValue]) -> Errno {
        let dirfd = args[0].as_i32() as u32;
        let path_ptr = args[1].as_i32() as usize;
        let path_len = args[2].as_i32() as usize;

        let host_path = match self.resolve_guest_path(store, dirfd, path_ptr, path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        std::fs::create_dir(&host_path).map_or_else(Into::into, |()| Errno::Success)
    }

    fn path_remove_directory(&self, store: &Store, args: &[RawValue]) -> Errno {
        let dirfd = args[0].as_i32() as u32;
        let path_ptr = args[1].as_i32() as usize;
        let path_len = args[2].as_i32() as usize;

        let host_path = match self.resolve_guest_path(store, dirfd, path_ptr, path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        std::fs::remove_dir(&host_path).map_or_else(Into::into, |()| Errno::Success)
    }

    fn path_unlink_file(&self, store: &Store, args: &[RawValue]) -> Errno {
        let dirfd = args[0].as_i32() as u32;
        let path_ptr = args[1].as_i32() as usize;
        let path_len = args[2].as_i32() as usize;

        let host_path = match self.resolve_guest_path(store, dirfd, path_ptr, path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        std::fs::remove_file(&host_path).map_or_else(Into::into, |()| Errno::Success)
    }

    fn path_rename(&self, store: &Store, args: &[RawValue]) -> Errno {
        let old_dirfd = args[0].as_i32() as u32;
        let old_path_ptr = args[1].as_i32() as usize;
        let old_path_len = args[2].as_i32() as usize;
        let new_dirfd = args[3].as_i32() as u32;
        let new_path_ptr = args[4].as_i32() as usize;
        let new_path_len = args[5].as_i32() as usize;

        let old = match self.resolve_guest_path(store, old_dirfd, old_path_ptr, old_path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let new = match self.resolve_guest_path(store, new_dirfd, new_path_ptr, new_path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        std::fs::rename(&old, &new).map_or_else(Into::into, |()| Errno::Success)
    }

    fn path_symlink(&self, store: &Store, args: &[RawValue]) -> Errno {
        let old_path_ptr = args[0].as_i32() as usize;
        let old_path_len = args[1].as_i32() as usize;
        let dirfd = args[2].as_i32() as u32;
        let new_path_ptr = args[3].as_i32() as usize;
        let new_path_len = args[4].as_i32() as usize;

        let mem = &store.memories[0].data;
        let old_path = match str::from_utf8(mem.read_bytes(old_path_ptr, old_path_len)) {
            Ok(s) => s.to_string(),
            Err(_) => return Errno::Inval,
        };

        let new_host = match self.resolve_guest_path(store, dirfd, new_path_ptr, new_path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&old_path, &new_host)
                .map_or_else(Into::into, |()| Errno::Success)
        }

        #[cfg(not(unix))]
        Errno::NoSys
    }

    fn path_link(&self, store: &Store, args: &[RawValue]) -> Errno {
        let old_dirfd = args[0].as_i32() as u32;
        let _old_flags = args[1].as_i32() as u32;
        let old_path_ptr = args[2].as_i32() as usize;
        let old_path_len = args[3].as_i32() as usize;
        let new_dirfd = args[4].as_i32() as u32;
        let new_path_ptr = args[5].as_i32() as usize;
        let new_path_len = args[6].as_i32() as usize;

        let old = match self.resolve_guest_path(store, old_dirfd, old_path_ptr, old_path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let new = match self.resolve_guest_path(store, new_dirfd, new_path_ptr, new_path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        std::fs::hard_link(&old, &new).map_or_else(Into::into, |()| Errno::Success)
    }

    fn path_readlink(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let dirfd = args[0].as_i32() as u32;
        let path_ptr = args[1].as_i32() as usize;
        let path_len = args[2].as_i32() as usize;
        let buf_ptr = args[3].as_i32() as usize;
        let buf_len = args[4].as_i32() as usize;
        let bufused_ptr = args[5].as_i32() as usize;

        let host_path = match self.resolve_guest_path(store, dirfd, path_ptr, path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let target = match std::fs::read_link(&host_path) {
            Ok(t) => t,
            Err(e) => return e.into(),
        };

        let target_bytes = target.as_os_str().as_encoded_bytes();
        let write_len = target_bytes.len().min(buf_len);

        let mem = &mut store.memories[0].data;
        mem.write_bytes(buf_ptr, &target_bytes[..write_len]);
        mem.write_u32(bufused_ptr, write_len as u32);

        Errno::Success
    }

    fn path_filestat_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let dirfd = args[0].as_i32() as u32;
        let _flags = args[1].as_i32() as u32;
        let path_ptr = args[2].as_i32() as usize;
        let path_len = args[3].as_i32() as usize;
        let buf_ptr = args[4].as_i32() as usize;

        let host_path = match self.resolve_guest_path(store, dirfd, path_ptr, path_len) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let metadata = match std::fs::metadata(&host_path) {
            Ok(m) => m,
            Err(e) => return e.into(),
        };

        let filetype = if metadata.is_dir() {
            FileType::Directory
        } else if metadata.is_symlink() {
            FileType::SymbolicLink
        } else {
            FileType::RegularFile
        };

        Self::write_filestat(
            &mut store.memories[0].data,
            buf_ptr,
            filetype,
            1,
            metadata.len(),
        );

        Errno::Success
    }

    fn path_filestat_set_times(&self, store: &Store, args: &[RawValue]) -> Errno {
        let dirfd = args[0].as_i32() as u32;
        let _flags = args[1].as_i32() as u32;
        let path_ptr = args[2].as_i32() as usize;
        let path_len = args[3].as_i32() as usize;
        let _atim = args[4].as_i64() as u64;
        let _mtim = args[5].as_i64() as u64;
        let _fst_flags = args[6].as_i32() as u16;

        match self.resolve_guest_path(store, dirfd, path_ptr, path_len) {
            Ok(_) => Errno::Success,
            Err(e) => e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn howdy() {
        let wasm = std::fs::read("../test-programs-wasip1/howdy.wasm").unwrap();
        let module = Module::new(&wasm).unwrap();
        let mut store = Store::new();
        let mut wasi = WasiCtx::new();

        let imports = wasi.imports(&mut store, &module);
        let instance = store.instantiate(&module, imports).unwrap();

        let exit_code = wasi.run(&mut store, instance).unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(&wasi.stdout_buf, b"howdy world\n");
    }

    #[test]
    fn file_io() {
        let dir = tempfile::tempdir().unwrap();
        let sandbox = dir.path().to_str().unwrap();

        let wasm = std::fs::read("../test-programs-wasip1/file_io.wasm").unwrap();
        let module = Module::new(&wasm).unwrap();
        let mut store = Store::new();
        let mut wasi = WasiCtx::new().preopen("/sandbox", sandbox);

        let imports = wasi.imports(&mut store, &module);
        let instance = store.instantiate(&module, imports).unwrap();
        let exit_code = wasi.run(&mut store, instance).unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(&wasi.stdout_buf, b"howdy from file\n");

        let on_disk = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
        assert_eq!(on_disk, "howdy from file");
    }

    #[test]
    fn sleep() {
        let wasm = std::fs::read("../test-programs-wasip1/sleep.wasm").unwrap();
        let module = Module::new(&wasm).unwrap();
        let mut store = Store::new();
        let mut wasi = WasiCtx::new();

        let before = wasi.clock_nanos;
        let imports = wasi.imports(&mut store, &module);
        let instance = store.instantiate(&module, imports).unwrap();
        let exit_code = wasi.run(&mut store, instance).unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(&wasi.stdout_buf, b"slept ok\n");
        assert!(wasi.clock_nanos >= before + 100_000_000);
    }

    #[test]
    fn snapshot_howdy() {
        use gabagool::snapshot::Snapshot;

        let wasm = std::fs::read("../test-programs-wasip1/howdy.wasm").unwrap();
        let module = Module::new(&wasm).unwrap();
        let mut store = Store::new();
        let mut wasi = WasiCtx::new();

        let imports = wasi.imports(&mut store, &module);
        let instance = store.instantiate(&module, imports).unwrap();
        let mut state = store
            .invoke::<[RawValue; 0]>(instance, "_start", [])
            .unwrap();

        let mut dispatched = 0;
        loop {
            match state {
                ExecutionState::Completed(_) => break,
                ExecutionState::FuelExhausted => break,
                ExecutionState::Suspended {
                    func_name, args, ..
                } => {
                    dispatched += 1;

                    if dispatched == 2 {
                        let store_snap = store.to_bytes();
                        let mut wasi_snap = Vec::new();
                        wasi.encode(&mut wasi_snap);

                        let mut restored = Store::from_bytes(&store_snap);
                        let mut wasi2 = WasiCtx::decode(&mut &wasi_snap[..]);

                        let results = wasi2.dispatch(&mut restored, &func_name, &args);
                        match results {
                            DispatchResult::Values(v) => {
                                state = restored.resume_with(&v).unwrap();
                            }
                            DispatchResult::Exit(code) => {
                                assert_eq!(code, 0);
                                return;
                            }
                        }

                        store = restored;
                        wasi = wasi2;
                        continue;
                    }

                    match wasi.dispatch(&mut store, &func_name, &args) {
                        DispatchResult::Values(v) => {
                            state = store.resume_with(&v).unwrap();
                        }
                        DispatchResult::Exit(_) => break,
                    }
                }
            }
        }

        assert_eq!(&wasi.stdout_buf, b"howdy world\n");
    }

    #[test]
    fn snapshot_file_io() {
        use gabagool::snapshot::Snapshot;

        let dir = tempfile::tempdir().unwrap();
        let sandbox = dir.path().to_str().unwrap();

        let wasm = std::fs::read("../test-programs-wasip1/file_io.wasm").unwrap();
        let module = Module::new(&wasm).unwrap();
        let mut store = Store::new();
        let mut wasi = WasiCtx::new().preopen("/sandbox", sandbox);

        let imports = wasi.imports(&mut store, &module);
        let instance = store.instantiate(&module, imports).unwrap();
        let mut state = store
            .invoke::<[RawValue; 0]>(instance, "_start", [])
            .unwrap();

        let mut dispatched = 0;
        loop {
            match state {
                ExecutionState::Completed(_) => break,
                ExecutionState::FuelExhausted => break,
                ExecutionState::Suspended {
                    func_name, args, ..
                } => {
                    dispatched += 1;

                    if dispatched == 5 {
                        let store_snap = store.to_bytes();
                        let mut wasi_snap = Vec::new();
                        wasi.encode(&mut wasi_snap);

                        let mut restored = Store::from_bytes(&store_snap);
                        let mut wasi2 = WasiCtx::decode(&mut &wasi_snap[..]);

                        let results = wasi2.dispatch(&mut restored, &func_name, &args);
                        match results {
                            DispatchResult::Values(v) => {
                                state = restored.resume_with(&v).unwrap();
                            }
                            DispatchResult::Exit(code) => {
                                assert_eq!(code, 0);
                                return;
                            }
                        }

                        store = restored;
                        wasi = wasi2;
                        continue;
                    }

                    match wasi.dispatch(&mut store, &func_name, &args) {
                        DispatchResult::Values(v) => {
                            state = store.resume_with(&v).unwrap();
                        }
                        DispatchResult::Exit(_) => break,
                    }
                }
            }
        }

        assert_eq!(&wasi.stdout_buf, b"howdy from file\n");
        let on_disk = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
        assert_eq!(on_disk, "howdy from file");
    }
}
