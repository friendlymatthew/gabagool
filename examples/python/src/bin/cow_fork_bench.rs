use gabagool::{Module, Store};
use gabagool_wasip1::WasiCtx;
use std::env;
use std::fs;
use std::time::Instant;

const SCRIPT_GUEST_PATH: &str = "/heap_demo.py";
const N_CHILDREN: usize = 8;

fn build_post_run_store() -> Store {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let wasm = fs::read(format!("{manifest_dir}/python.wasm")).unwrap();
    let module = Module::new(&wasm).unwrap();
    let mut store = Store::new_cow();

    let mut wasi = WasiCtx::new()
        .with_args(&["python", SCRIPT_GUEST_PATH])
        .preopen("/", manifest_dir);

    let imports = wasi.imports(&mut store, &module);
    let instance = store.instantiate(&module, imports).unwrap();
    wasi.run(&mut store, instance).unwrap();

    store
}

fn fmt_mb(bytes: usize) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

fn run() {
    eprintln!("booting cpython and running heap_demo.py...");
    let parent = build_post_run_store();
    let heap_bytes = parent.memories[0].data.len();
    eprintln!("heap size: {}\n", fmt_mb(heap_bytes));

    let t0 = Instant::now();
    let bytes = parent.to_bytes();
    let to_bytes_time = t0.elapsed();

    let t1 = Instant::now();
    let baseline_forks = (0..N_CHILDREN)
        .map(|_| Store::from_bytes(&bytes))
        .collect::<Vec<_>>();
    let from_bytes_time = t1.elapsed();

    let baseline_total = to_bytes_time + from_bytes_time;
    let serialized_size = bytes.len();
    drop(baseline_forks);
    drop(bytes);

    eprintln!("baseline (to_bytes + {N_CHILDREN}x from_bytes):");
    eprintln!("  serialized buffer: {}", fmt_mb(serialized_size));
    eprintln!("  to_bytes:          {to_bytes_time:?}");
    eprintln!(
        "  from_bytes x{N_CHILDREN}:    {from_bytes_time:?}  (avg {:?} per child)",
        from_bytes_time / N_CHILDREN as u32
    );
    eprintln!("  total:             {baseline_total:?}\n");

    let t2 = Instant::now();
    let snap = parent.snapshot();
    let snapshot_time = t2.elapsed();

    let t3 = Instant::now();
    let cow_forks = snap.fork(N_CHILDREN).unwrap();
    let fork_time = t3.elapsed();

    let cow_total = snapshot_time + fork_time;
    drop(cow_forks);

    eprintln!("cow (snapshot + fork({N_CHILDREN})):");
    eprintln!("  snapshot:          {snapshot_time:?}");
    eprintln!(
        "  fork({N_CHILDREN}):           {fork_time:?}  (avg {:?} per child)",
        fork_time / N_CHILDREN as u32
    );
    eprintln!("  total:             {cow_total:?}\n");

    let speedup = baseline_total.as_secs_f64() / cow_total.as_secs_f64();
    eprintln!("cow is {speedup:.0}x faster ({cow_total:?} vs {baseline_total:?})");
}

fn main() {
    let builder = std::thread::Builder::new().stack_size(64 * 1024 * 1024);
    builder.spawn(run).unwrap().join().unwrap();
}
