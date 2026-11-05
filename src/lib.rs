#![no_std]
#![warn(clippy::nursery)]

extern crate alloc;

pub mod compiler;
pub mod component;
mod error;
pub mod ir;
pub mod leb128;
mod linker;
mod module;
pub mod parser;
mod prelude;
pub mod runtime;
pub mod snapshot;
mod validator;

pub mod wasi;

#[cfg(feature = "jit")]
mod jit;

pub use component::*;
pub use error::*;
pub use linker::*;
pub use module::*;
pub use runtime::{
    CallFrame, DataInstance, ElementInstance, ExecutionState, ExportInstance, ExternalValue,
    FunctionInstance, GlobalInstance, GuestMemory, Instance, MemoryInstance, RawValue, Ref, Store,
    TableInstance, TagInstance,
};
pub use snapshot::StoreSnapshot;
