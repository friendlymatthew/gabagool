use crate::prelude::*;

use super::WasiOutput;

#[derive(Debug, Default)]
pub struct WasiCtx<O = Vec<u8>> {
    stdout: O,
    args: Vec<String>,
}

impl<O: WasiOutput> WasiCtx<O> {
    pub const fn new(stdout: O) -> Self {
        Self {
            stdout,
            args: vec![],
        }
    }

    pub const fn stdout(&self) -> &O {
        &self.stdout
    }

    pub const fn stdout_mut(&mut self) -> &mut O {
        &mut self.stdout
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn args_mut(&mut self) -> &mut Vec<String> {
        &mut self.args
    }
}
