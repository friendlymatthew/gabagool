use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
    path::PathBuf,
    process,
    thread,
};

use gabagool::{
    wasi::{self, WasiCtx},
    ExecutionState, Linker, Module, Store,
};

fn main() {
    // Loading CPython needs more native stack in debug builds than the
    // platform's default main thread provides. Construct and use the runtime
    // on this thread; its host callbacks do not need to be Send.
    let outcome = thread::Builder::new()
        .name("python-runner".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| run().map_err(|error| error.to_string()))
        .map_err(|error| format!("cannot start Python runner: {error}"))
        .and_then(|worker| {
            worker.join().map_err(|_| "Python runner panicked".to_owned())
        })
        .and_then(|result| result);

    match outcome {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("error: {error}");
            process::exit(1);
        }
    }
}

// Usage: python [--wasm PATH] [PYTHON_ARGS...]
fn run() -> Result<i32, Box<dyn Error>> {
    let mut arguments = env::args().skip(1).peekable();
    let mut wasm_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/python.wasm");

    if arguments.peek().is_some_and(|arg| arg == "--wasm") {
        arguments.next();
        wasm_path = arguments.next().ok_or("--wasm requires a path")?.into();
    }

    let mut python_args = arguments.collect::<Vec<_>>();
    if python_args.is_empty() {
        python_args = vec!["-c".into(), "print('hello')".into()];
    }

    let bytes = fs::read(&wasm_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot read {}: {error}; run sh examples/python/download.sh first",
                wasm_path.display(),
            ),
        )
    })?;

    let module = Module::try_new(&bytes)?;
    let mut store = Store::new();

    let mut linker = Linker::new();
    wasi::add_wasip1_to_linker(&mut linker)?;

    let mut ctx = WasiCtx::default();
    ctx.args_mut().push("python".into());
    ctx.args_mut().extend(python_args);

    let instance = linker.instantiate(&mut store, &module)?;
    let outcome = linker.invoke_no_args(&mut store, &mut ctx, instance, "_start");

    let mut stdout = io::stdout().lock();
    stdout.write_all(ctx.stdout().unwrap_or_default())?;
    stdout.flush()?;

    let mut stderr = io::stderr().lock();
    stderr.write_all(ctx.stderr().unwrap_or_default())?;
    stderr.flush()?;

    match outcome? {
        ExecutionState::Completed(_) => Ok(0),
        ExecutionState::Exited(code) => Ok(i32::try_from(code).unwrap_or(1)),
        ExecutionState::FuelExhausted => Err("Python execution exhausted its fuel".into()),
        ExecutionState::Suspended {
            module_name,
            func_name,
            ..
        } => Err(format!("unhandled host call: {module_name}.{func_name}").into()),
    }
}
