use crate::prelude::*;

use super::WasiOutput;

#[derive(Debug, Default)]
pub struct WasiCtx<O = Vec<u8>> {
    stdout: O,
}

impl<O: WasiOutput> WasiCtx<O> {
    pub const fn new(stdout: O) -> Self {
        Self { stdout }
    }

    pub const fn stdout(&self) -> &O {
        &self.stdout
    }

    pub const fn stdout_mut(&mut self) -> &mut O {
        &mut self.stdout
    }
}
