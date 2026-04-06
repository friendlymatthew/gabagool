use std::env;
use std::fs;
use std::path::Path;

use gabagool::{Module, Store};
use gabagool_wasip1::WasiCtx;

fn run() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let script = env::args().nth(1).unwrap_or_else(|| "hello.py".into());

    let script = {
        let abs = fs::canonicalize(&script).unwrap_or_else(|_| {
            eprintln!("python: can't find file '{script}'");
            std::process::exit(2);
        });
        let rel = abs
            .strip_prefix(Path::new(manifest_dir))
            .unwrap_or_else(|_| {
                eprintln!(
                    "python: file '{}' is outside the preopen directory '{manifest_dir}'",
                    abs.display()
                );
                std::process::exit(2);
            });
        format!("/{}", rel.display())
    };

    let wasm = fs::read(format!("{manifest_dir}/python.wasm")).unwrap();
    let module = Module::new(&wasm).unwrap();
    let mut store = Store::new();

    let mut wasi = WasiCtx::new()
        .with_args(&["python", &script])
        .preopen("/", manifest_dir);

    let imports = wasi.imports(&mut store, &module);
    let instance = store.instantiate(&module, imports).unwrap();
    let exit_code = wasi.run(&mut store, instance).unwrap();

    if exit_code != 0 {
        eprintln!("python exited with code {exit_code}");
    }
}

fn main() {
    let builder = std::thread::Builder::new().stack_size(64 * 1024 * 1024);
    builder.spawn(run).unwrap().join().unwrap();
}
