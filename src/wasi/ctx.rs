use crate::prelude::*;

use super::WasiOutput;

#[derive(Debug, Default)]
pub struct WasiCtx<O = Vec<u8>> {
    stdout: O,
    args: Vec<String>,
    environment: Vec<String>,
}

impl<O: WasiOutput> WasiCtx<O> {
    pub const fn new(stdout: O) -> Self {
        Self {
            stdout,
            args: vec![],
            environment: vec![],
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

    pub const fn args_mut(&mut self) -> &mut Vec<String> {
        &mut self.args
    }

    pub fn environment(&self) -> &[String] {
        &self.environment
    }

    pub const fn environment_mut(&mut self) -> &mut Vec<String> {
        &mut self.environment
    }
}
