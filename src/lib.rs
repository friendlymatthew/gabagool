#![warn(clippy::nursery)]

pub mod compiler;
pub mod component;
mod error;
pub mod ir;
pub mod leb128;
mod linker;
mod module;
pub mod parser;
pub mod runtime;
pub mod snapshot;

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

pub mod value_stack {
    pub use crate::runtime::ValueStack;
}
