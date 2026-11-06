use gabagool::wasi::traits::{RandomError, WasiRandom};

#[derive(Debug)]
pub struct OsRandom;

impl WasiRandom for OsRandom {
    fn fill_bytes(&mut self, buffer: &mut [u8]) -> Result<(), RandomError> {
        getrandom::fill(buffer).map_err(|_| RandomError::Unavailable)
    }
}
