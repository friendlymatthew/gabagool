use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gabagool::{
    CompositeType, ExecutionState, ExternalValue, FunctionInstance, ImportDescription, Instance,
    Module, RawValue, Store,
};

use crate::{Errno, FdEntry, FdFlags, FdKind, FdTable, FileType, Rights};

pub enum DispatchResult {
    Values(Vec<RawValue>),
    Exit(u32),
}

#[derive(Debug, Default)]
pub struct WasiCtx {
    pub fd_table: FdTable,
    pub args: Vec<String>,
    pub environ: Vec<(String, String)>,
    pub exit_code: Option<u32>,
    pub stdin_buf: Vec<u8>,
    pub stdout_buf: Vec<u8>,
    pub stderr_buf: Vec<u8>,
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
            "fd_write" => self.fd_write(store, args),
            "fd_prestat_get" => self.fd_prestat_get(store, args),
            "fd_prestat_dir_name" => self.fd_prestat_dir_name(store, args),
            "args_sizes_get" => self.args_sizes_get(store, args),
            "args_get" => self.args_get(store, args),
            "environ_sizes_get" => self.environ_sizes_get(store, args),
            "environ_get" => self.environ_get(store, args),
            "clock_time_get" => self.clock_time_get(store, args),
            "clock_res_get" => self.clock_res_get(store, args),
            "random_get" => self.random_get(store, args),
            "sched_yield" => Errno::Success,
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

    fn fd_write(&mut self, store: &mut Store, args: &[RawValue]) -> Errno {
        let fd = args[0].as_i32() as u32;
        let iovs = args[1].as_i32() as u32;
        let iovs_len = args[2].as_i32() as u32;
        let nwritten_ptr = args[3].as_i32() as u32;

        let entry = match self.fd_table.get(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !entry.rights_base.fd_write() {
            return Errno::NotCapable;
        }

        let mem = &store.memories[0].data;
        let mut total_written: u32 = 0;
        let mut bufs = Vec::with_capacity(iovs_len as usize);

        for i in 0..iovs_len {
            let base = (iovs + i * 8) as usize;
            let buf_ptr = u32::from_le_bytes(mem[base..base + 4].try_into().unwrap()) as usize;
            let buf_len = u32::from_le_bytes(mem[base + 4..base + 8].try_into().unwrap()) as usize;

            bufs.push(mem[buf_ptr..buf_ptr + buf_len].to_vec());

            total_written += buf_len as u32;
        }

        match &self.fd_table.get(fd).unwrap().kind {
            FdKind::Stdout => {
                for buf in &bufs {
                    self.stdout_buf.extend_from_slice(buf);
                    let _ = std::io::stdout().lock().write_all(buf);
                }
            }
            FdKind::Stderr => {
                for buf in &bufs {
                    self.stderr_buf.extend_from_slice(buf);
                    let _ = std::io::stderr().lock().write_all(buf);
                }
            }
            _ => return Errno::BadF,
        }

        let mem = &mut store.memories[0].data;
        let ptr = nwritten_ptr as usize;
        mem[ptr..ptr + 4].copy_from_slice(&total_written.to_le_bytes());

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

        let mem = &mut store.memories[0].data;

        let ptr = prestat_ptr as usize;
        mem[ptr] = 0;
        mem[ptr + 1..ptr + 4].fill(0);
        mem[ptr + 4..ptr + 8].copy_from_slice(&(guest_path.len() as u32).to_le_bytes());

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
        mem[ptr..ptr + len].copy_from_slice(&bytes[..len]);

        Errno::Success
    }

    fn args_sizes_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let argc_ptr = args[0].as_i32() as usize;
        let buf_size_ptr = args[1].as_i32() as usize;

        let argc = self.args.len() as u32;
        let buf_size = self.args.iter().map(|a| a.len() as u32 + 1).sum::<u32>();

        let mem = &mut store.memories[0].data;
        mem[argc_ptr..argc_ptr + 4].copy_from_slice(&argc.to_le_bytes());
        mem[buf_size_ptr..buf_size_ptr + 4].copy_from_slice(&buf_size.to_le_bytes());

        Errno::Success
    }

    fn args_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let argv_ptr = args[0].as_i32() as usize;
        let argv_buf_ptr = args[1].as_i32() as usize;

        let mem = &mut store.memories[0].data;
        let mut buf_offset = argv_buf_ptr;

        for (i, arg) in self.args.iter().enumerate() {
            let bytes = arg.as_bytes();
            mem[argv_ptr + i * 4..argv_ptr + i * 4 + 4]
                .copy_from_slice(&(buf_offset as u32).to_le_bytes());
            mem[buf_offset..buf_offset + bytes.len()].copy_from_slice(bytes);

            mem[buf_offset + bytes.len()] = 0;
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
        mem[count_ptr..count_ptr + 4].copy_from_slice(&count.to_le_bytes());
        mem[buf_size_ptr..buf_size_ptr + 4].copy_from_slice(&buf_size.to_le_bytes());

        Errno::Success
    }

    fn environ_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let environ_ptr = args[0].as_i32() as usize;
        let environ_buf_ptr = args[1].as_i32() as usize;

        let mem = &mut store.memories[0].data;
        let mut buf_offset = environ_buf_ptr;

        for (i, (key, val)) in self.environ.iter().enumerate() {
            mem[environ_ptr + i * 4..environ_ptr + i * 4 + 4]
                .copy_from_slice(&(buf_offset as u32).to_le_bytes());

            let entry = format!("{key}={val}");
            let bytes = entry.as_bytes();

            mem[buf_offset..buf_offset + bytes.len()].copy_from_slice(bytes);
            mem[buf_offset + bytes.len()] = 0;

            buf_offset += bytes.len() + 1;
        }

        Errno::Success
    }

    fn clock_time_get(&self, store: &mut Store, args: &[RawValue]) -> Errno {
        let clock_id = args[0].as_i32() as u32;
        let _precision = args[1].as_i64() as u64;
        let timestamp_ptr = args[2].as_i32() as usize;

        let nanos = match clock_id {
            0 | 1 => {
                let duration = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default();
                duration.as_nanos() as u64
            }
            _ => return Errno::Inval,
        };

        let mem = &mut store.memories[0].data;
        mem[timestamp_ptr..timestamp_ptr + 8].copy_from_slice(&nanos.to_le_bytes());

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
        mem[resolution_ptr..resolution_ptr + 8].copy_from_slice(&res.to_le_bytes());

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

        for byte in &mut mem[buf_ptr..buf_ptr + buf_len] {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *byte = seed as u8;
        }

        Errno::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn howdy() {
        let wasm = std::fs::read("../programs-wasi/howdy.wasm").unwrap();
        let module = Module::new(&wasm).unwrap();
        let mut store = Store::new();
        let mut wasi = WasiCtx::new();

        let imports = wasi.imports(&mut store, &module);
        let instance = store.instantiate(&module, imports).unwrap();

        let exit_code = wasi.run(&mut store, instance).unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(&wasi.stdout_buf, b"howdy world\n");
    }
}
