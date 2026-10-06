#[repr(C)]
#[derive(Debug, Default)]
pub struct OpImmediate {
    pub imm0: u64,
    pub imm1: u64,
}

pub type StencilFn = unsafe extern "C" fn(*mut StencilContext);

#[repr(C)]
pub struct StencilContext {
    pub stack: *mut u64,
    pub stack_pointer: u64,
    pub locals: *mut u64,
    pub mem_base: *mut u64,
    pub mem_len: u64,
    pub globals: *mut u64,
    pub imm_table: *const OpImmediate,
    pub fn_table: *const StencilFn,
    pub pc: u32,
    pub snapshot_flag: u8,
    pub exit_reason: u8,
    pub exit_value: u32,
}

pub enum Exit {
    Snapshot = 0,
    Return = 1,
}

impl From<u8> for Exit {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Snapshot,
            1 => Self::Return,
            _ => unimplemented!(),
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/stencils_generated.rs"));
