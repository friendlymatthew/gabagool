#![warn(clippy::nursery)]

mod binary_grammar;
pub mod compiler;
pub mod component;
pub mod debugger;
mod error;
mod execution_grammar;
mod exponential_decay;
pub mod ir;
pub mod leb128;
pub mod parser;
pub mod snapshot;
mod store;
pub mod value_stack;

#[cfg(feature = "jit")]
mod jit;

pub use binary_grammar::*;
pub use component::*;
pub use error::*;
pub use execution_grammar::*;
pub use store::*;
