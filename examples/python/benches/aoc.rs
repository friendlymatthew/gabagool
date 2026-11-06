use std::{
    env, fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::Duration,
};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode};

fn execute(executable: &std::ffi::OsStr, script: &Path) -> Output {
    let output = Command::new(executable)
        .arg(script)
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHASHSEED")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("cannot launch {executable:?}: {error}"));

    assert!(
        output.status.success(),
        "{executable:?} {script:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    output
}

fn aoc(c: &mut Criterion) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(
        root.join("runtime/python.wasm").is_file(),
        "download CPython first: sh examples/python/download.sh"
    );

    let python = env::var_os("AOC_PYTHON").unwrap_or_else(|| "python3".into());

    let runner = std::ffi::OsStr::new(env!("CARGO_BIN_EXE_python"));

    let mut scripts = fs::read_dir(root.join("aoc"))
        .expect("cannot read aoc directory")
        .map(|entry| entry.expect("cannot read aoc entry").path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("day") && name.contains('_') && name.ends_with(".py")
                    })
        })
        .collect::<Vec<_>>();

    scripts.sort();

    assert!(!scripts.is_empty(), "no AoC solutions found");

    for script in scripts {
        let name = script.file_stem().unwrap().to_str().unwrap();
        let guest = Path::new("/aoc").join(script.file_name().unwrap());

        let mut group = c.benchmark_group(format!("aoc/{name}"));
        group.sampling_mode(SamplingMode::Flat);

        let native_id = BenchmarkId::from_parameter("native");
        let guest_id = BenchmarkId::from_parameter("gabagool");
        let expected = execute(&python, &script).stdout;

        assert_eq!(
            expected,
            execute(runner, &guest).stdout,
            "output mismatch: {name}"
        );

        group.bench_function(native_id, |b| {
            b.iter(|| execute(&python, &script));
        });

        group.bench_function(guest_id, |b| {
            b.iter(|| execute(runner, &guest));
        });

        group.finish();
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5));
    targets = aoc
}

criterion_main!(benches);
