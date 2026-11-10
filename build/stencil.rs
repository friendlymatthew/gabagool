use std::{env, path::PathBuf, process::Command};

pub fn generate() {
    let target = env::var("TARGET").expect("Cargo did not provide TARGET");
    let clang_target = match target.as_str() {
        "aarch64-apple-darwin" => "arm64-apple-macos",
        other => other,
    };

    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    let source = root.join("src/jit/stencils/stencils.c");
    let object = out_dir.join("stencils.o");

    let status = Command::new("clang")
        .arg(format!("--target={clang_target}"))
        .args([
            // enables optimized stencil code
            // todo: look into -O3 ?
            "-O2",
            // prevents function inlining
            "-fno-inline",
            // compiles into object files without linking
            "-c",
        ])
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .status()
        .expect("failed to start clang");

    assert!(
        status.success(),
        "stencil compilation failed for target {target} (Clang target {clang_target})"
    );

    println!("cargo::rerun-if-changed=src/jit/stencils/stencils.c");
    println!("cargo::rerun-if-changed=src/jit/stencils/shared.h");
}
