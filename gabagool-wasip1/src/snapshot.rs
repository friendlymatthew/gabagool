use std::path::{Path, PathBuf};

use gabagool::snapshot::Snapshot;

use crate::{FdEntry, FdFlags, FdKind, FdTable, FileType, Rights, WasiCtx};

impl Snapshot for FileType {
    fn encode(&self, buf: &mut Vec<u8>) {
        (*self as u8).encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Unknown,
            1 => Self::BlockDevice,
            2 => Self::CharacterDevice,
            3 => Self::Directory,
            4 => Self::RegularFile,
            5 => Self::SocketDgram,
            6 => Self::SocketStream,
            7 => Self::SymbolicLink,
            d => panic!("invalid FileType discriminant: {d}"),
        }
    }
}

impl Snapshot for FdFlags {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.0.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self(u16::decode(buf))
    }
}

impl Snapshot for Rights {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.0.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self(u64::decode(buf))
    }
}

fn encode_path(path: &Path, buf: &mut Vec<u8>) {
    path.to_str().unwrap_or("").to_string().encode(buf);
}

fn decode_path(buf: &mut &[u8]) -> PathBuf {
    PathBuf::from(String::decode(buf))
}

impl Snapshot for FdKind {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Stdin => 0u8.encode(buf),
            Self::Stdout => 1u8.encode(buf),
            Self::Stderr => 2u8.encode(buf),
            Self::File { host_path, .. } => {
                3u8.encode(buf);
                encode_path(host_path, buf);
            }
            Self::Directory { host_path } => {
                4u8.encode(buf);
                encode_path(host_path, buf);
            }
            Self::Preopen {
                host_path,
                guest_path,
            } => {
                5u8.encode(buf);
                encode_path(host_path, buf);
                guest_path.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Stdin,
            1 => Self::Stdout,
            2 => Self::Stderr,
            3 => {
                let host_path = decode_path(buf);
                Self::File {
                    host_path,
                    file: None,
                }
            }
            4 => Self::Directory {
                host_path: decode_path(buf),
            },
            5 => {
                let host_path = decode_path(buf);
                let guest_path = String::decode(buf);
                Self::Preopen {
                    host_path,
                    guest_path,
                }
            }
            d => panic!("invalid FdKind discriminant: {d}"),
        }
    }
}

impl Snapshot for FdEntry {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.file_type.encode(buf);
        self.flags.encode(buf);
        self.rights_base.encode(buf);
        self.rights_inheriting.encode(buf);
        self.offset.encode(buf);
        self.kind.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            file_type: FileType::decode(buf),
            flags: FdFlags::decode(buf),
            rights_base: Rights::decode(buf),
            rights_inheriting: Rights::decode(buf),
            offset: u64::decode(buf),
            kind: FdKind::decode(buf),
        }
    }
}

impl Snapshot for FdTable {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.entries.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            entries: Vec::decode(buf),
        }
    }
}

impl Snapshot for WasiCtx {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.fd_table.encode(buf);
        self.args.encode(buf);
        (self.environ.len() as u32).encode(buf);
        for (k, v) in &self.environ {
            k.encode(buf);
            v.encode(buf);
        }
        self.exit_code.encode(buf);
        self.stdin_buf.encode(buf);
        self.stdout_buf.encode(buf);
        self.stderr_buf.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let fd_table = FdTable::decode(buf);
        let args = Vec::<String>::decode(buf);
        let environ_len = u32::decode(buf) as usize;
        let mut environ = Vec::with_capacity(environ_len);
        for _ in 0..environ_len {
            let k = String::decode(buf);
            let v = String::decode(buf);
            environ.push((k, v));
        }
        let exit_code = Option::decode(buf);
        let stdin_buf = Vec::<u8>::decode(buf);
        let stdout_buf = Vec::<u8>::decode(buf);
        let stderr_buf = Vec::<u8>::decode(buf);

        let mut ctx = Self {
            fd_table,
            args,
            environ,
            exit_code,
            stdin_buf,
            stdout_buf,
            stderr_buf,
        };

        ctx.reopen_files();
        ctx
    }
}

impl WasiCtx {
    pub fn reopen_files(&mut self) {
        for i in 0..self.fd_table.len() {
            let entry = match self.fd_table.get_mut(i as u32) {
                Ok(e) => e,
                Err(_) => continue,
            };

            if let FdKind::File {
                host_path,
                ref mut file,
            } = &mut entry.kind
            {
                if file.is_none() {
                    let mut open_opts = std::fs::OpenOptions::new();
                    if entry.rights_base.fd_read() {
                        open_opts.read(true);
                    }
                    if entry.rights_base.fd_write() {
                        open_opts.write(true);
                    }
                    if let Ok(mut f) = open_opts.open(host_path) {
                        use std::io::{Seek, SeekFrom};
                        let _ = f.seek(SeekFrom::Start(entry.offset));
                        *file = Some(f);
                    }
                }
            }
        }
    }
}
