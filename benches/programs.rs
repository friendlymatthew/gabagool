use criterion::{criterion_group, criterion_main, Criterion};
use gabagool::{Module, RawValue, Store};
use std::fs;
use std::path::PathBuf;

extern crate wasmi;

fn programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-programs")
}

fn load_and_instantiate(wasm_bytes: &[u8]) -> (Store, gabagool::Instance) {
    let module = Module::try_new(wasm_bytes).unwrap();
    let mut store = Store::new();
    let instance = store.instantiate(&module, vec![]).unwrap();
    (store, instance)
}

fn bench_fibonacci(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("fibonacci.wasm")).unwrap();

    c.bench_function("fib(30)", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke(instance, "fib", vec![RawValue::from(30i32)])
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 832040);
        });
    });

    // c.bench_function("fib(30) wasmi", |b| {
    //     let engine = wasmi::Engine::default();
    //     let module = wasmi::Module::try_new(&engine, &wasm[..]).unwrap();
    //     b.iter(|| {
    //         let mut store = wasmi::Store::new(&engine, ());
    //         let linker = wasmi::Linker::new(&engine);
    //         let instance = linker
    //             .instantiate(&mut store, &module)
    //             .unwrap()
    //             .start(&mut store)
    //             .unwrap();
    //         let fib = instance.get_typed_func::<i32, i32>(&store,
    // "fib").unwrap();         let result = fib.call(&mut store,
    // 30).unwrap();         assert_eq!(result, 832040);
    //     });
    // });
}

fn bench_matrix(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("matrix.wasm")).unwrap();

    c.bench_function("matrix_multiply_64x64", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "matrix_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 626828219);
        });
    });
}

fn bench_sieve(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("sieve.wasm")).unwrap();

    c.bench_function("sieve_100k", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "count_primes")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 9592);
        });
    });
}

fn bench_sort(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("sort.wasm")).unwrap();

    c.bench_function("quicksort_4096", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "sort_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 67582043);
        });
    });
}

fn bench_ackermann(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("ackermann.wasm")).unwrap();

    c.bench_function("ackermann(3,5)", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "ackermann_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 253);
        });
    });
}

fn bench_mandelbrot(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("mandelbrot.wasm")).unwrap();

    c.bench_function("mandelbrot_128x128", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "mandelbrot_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 429384);
        });
    });
}

fn bench_nbody(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("nbody.wasm")).unwrap();

    c.bench_function("nbody_100k_steps", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "nbody_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), -169079859);
        });
    });
}

fn bench_sha256(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("sha256.wasm")).unwrap();

    c.bench_function("sha256_1kb_x1000", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "sha256_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), -1206794323);
        });
    });
}

fn bench_switch_dispatch(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("switch_dispatch.wasm")).unwrap();

    c.bench_function("switch_dispatch_1m", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "switch_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 1);
        });
    });
}

fn bench_indirect_call(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("indirect_call.wasm")).unwrap();

    c.bench_function("indirect_call_1m", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "indirect_call_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 1);
        });
    });
}

fn bench_call_chain(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("call_chain.wasm")).unwrap();

    c.bench_function("call_chain_100k", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "call_chain_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 706982704);
        });
    });
}

fn bench_binary_search(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("binary_search.wasm")).unwrap();

    c.bench_function("binary_search_100k", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "binary_search_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 33298);
        });
    });
}

fn bench_linked_list(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("linked_list.wasm")).unwrap();

    c.bench_function("linked_list_16k_x200", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "linked_list_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 1072103424);
        });
    });
}

fn bench_bulk_memory(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("bulk_memory.wasm")).unwrap();

    c.bench_function("bulk_memory_64kb_x500", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "bulk_memory_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 8364412);
        });
    });
}

fn bench_matrix_chain(c: &mut Criterion) {
    let wasm = fs::read(programs_dir().join("matrix_chain.wasm")).unwrap();

    c.bench_function("matrix_chain_dp_200", |b| {
        b.iter(|| {
            let (mut store, instance) = load_and_instantiate(&wasm);
            let result = store
                .invoke_no_args(instance, "matrix_chain_bench")
                .unwrap()
                .into_completed()
                .unwrap();
            assert_eq!(result[0].as_i32(), 6885669);
        });
    });
}

criterion_group!(
    benches,
    bench_fibonacci,
    bench_matrix,
    bench_sieve,
    bench_sort,
    bench_ackermann,
    bench_mandelbrot,
    bench_nbody,
    bench_sha256,
    bench_switch_dispatch,
    bench_indirect_call,
    bench_call_chain,
    bench_binary_search,
    bench_linked_list,
    bench_bulk_memory,
    bench_matrix_chain,
);
criterion_main!(benches);
