use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use addr2line::Context;
use gabagool::debugger::{Debugger, StepResult};
use gabagool::ir::CompilerMode;
use gabagool::{Module, RawValue, Store};
use gimli::{EndianSlice, LittleEndian, SectionId};
use wasmparser::{Parser, Payload};

const C_SOURCE: &str = "\
int fib(int n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
";
const EXPECTED_RECURSIVE_LINE: u32 = 5;

#[test]
fn clang_dwarf_locations_resolve_gabagool_offsets() -> Result<(), Box<dyn Error>> {
    let Some(fixture) = compile_c_fixture()? else {
        return Ok(());
    };

    let wasm = fs::read(&fixture.wasm_path)?;
    let context = dwarf_context(&wasm)?;
    let module = Module::new_with_mode(&wasm, CompilerMode::Debug)?;
    let mut store = Store::new();
    let instance = store.instantiate(&module, vec![])?;
    let mut debugger = Debugger::new(store, instance);

    debugger.start("fib", vec![RawValue::from(3_i32)])?;

    let mut saw_recursive_line = false;
    loop {
        if let Some(frame) = debugger.call_stack().last() {
            if let Some(location) = frame.instruction_location {
                let source = context.find_location(location.code_offset)?;
                if let Some(source) = source {
                    let file_matches = source
                        .file
                        .is_some_and(|file| PathBuf::from(file).ends_with("fib.c"));
                    if file_matches && source.line == Some(EXPECTED_RECURSIVE_LINE) {
                        saw_recursive_line = true;
                        break;
                    }
                }
            }
        }

        match debugger.step_forward()? {
            StepResult::Stepped => {}
            StepResult::Completed => break,
            other => panic!("unexpected debugger stop: {other:?}"),
        }
    }

    assert!(
        saw_recursive_line,
        "no gabagool instruction_location resolved to {}:{}",
        fixture.source_path.display(),
        EXPECTED_RECURSIVE_LINE,
    );

    Ok(())
}

struct CompiledFixture {
    source_path: PathBuf,
    wasm_path: PathBuf,
}

fn compile_c_fixture() -> Result<Option<CompiledFixture>, Box<dyn Error>> {
    let Some(clang) = find_clang() else {
        if require_wasm_clang() {
            return Err("no clang found".into());
        }

        eprintln!("skipping dwarf location test: no clang found");
        return Ok(None);
    };

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "gabagool-dwarf-locations-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;

    let source_path = dir.join("fib.c");
    let wasm_path = dir.join("fib.wasm");
    fs::write(&source_path, C_SOURCE)?;

    let output = Command::new(&clang)
        .arg("--target=wasm32")
        .arg("-O0")
        .arg("-g")
        .arg("-nostdlib")
        .arg("-Wl,--no-entry")
        .arg("-Wl,--export=fib")
        .arg("-o")
        .arg(&wasm_path)
        .arg(&source_path)
        .output()?;

    if !output.status.success() {
        if require_wasm_clang() {
            return Err(format!(
                "clang could not produce wasm\nclang: {}\nstatus: {}\nstderr:\n{}",
                PathBuf::from(&clang).display(),
                output.status,
                String::from_utf8_lossy(&output.stderr),
            )
            .into());
        }

        eprintln!(
            "skipping dwarf location test: clang could not produce wasm\nclang: {}\nstatus: {}\nstderr:\n{}",
            PathBuf::from(&clang).display(),
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        return Ok(None);
    }

    Ok(Some(CompiledFixture {
        source_path,
        wasm_path,
    }))
}

fn require_wasm_clang() -> bool {
    std::env::var_os("GABAGOOL_REQUIRE_WASM_CLANG").is_some()
}

fn find_clang() -> Option<OsString> {
    let mut candidates = Vec::new();
    if let Ok(clang) = std::env::var("CLANG") {
        candidates.push(OsString::from(clang));
    }
    candidates.push(OsString::from("/opt/homebrew/opt/llvm/bin/clang"));
    candidates.push(OsString::from("clang"));

    candidates.into_iter().find(|candidate| {
        Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    })
}

type DwarfContext = Context<EndianSlice<'static, LittleEndian>>;

fn dwarf_context(wasm: &[u8]) -> Result<DwarfContext, Box<dyn Error>> {
    let mut sections: HashMap<String, &'static [u8]> = HashMap::new();

    for payload in Parser::new(0).parse_all(wasm) {
        let Payload::CustomSection(section) = payload? else {
            continue;
        };
        if !section.name().starts_with(".debug_") {
            continue;
        }

        let data = section.data().to_vec().into_boxed_slice();
        sections.insert(section.name().to_owned(), Box::leak(data));
    }

    let load_section = |id: SectionId| -> Result<EndianSlice<'static, LittleEndian>, gimli::Error> {
        Ok(EndianSlice::new(
            sections.get(id.name()).copied().unwrap_or(&[]),
            LittleEndian,
        ))
    };

    let dwarf = gimli::Dwarf::load(load_section)?;
    Ok(Context::from_dwarf(dwarf)?)
}
