use std::env;
use std::fs;
use std::path::Path;

pub fn generate() {
    println!("cargo::rerun-if-changed=tests/components");

    let out_dir = env::var("OUT_DIR").unwrap();
    let components_dir = Path::new("tests/components");

    if !components_dir.exists() {
        fs::write(Path::new(&out_dir).join("component_tests_generated.rs"), "").unwrap();
        return;
    }

    let mut all_tests = String::new();

    let entries = fs::read_dir(components_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "wasm"));

    for entry in entries {
        let path = entry.path();
        let file_stem = path.file_stem().unwrap().to_str().unwrap();
        let safe_name = file_stem.replace('-', "_");

        all_tests.push_str(&format!(
            concat!(
                "#[test]\n",
                "fn {name}() {{\n",
                "    let wasm_bytes = std::fs::read(\"{path}\").unwrap();\n",
                "    let result = gabagool::parser::Parser::new(&wasm_bytes).parse();\n",
                "    assert!(result.is_ok(), \"failed to parse component {name}: {{:?}}\", result.err());\n",
                "}}\n",
            ),
            name = safe_name,
            path = path.display(),
        ));
    }

    fs::write(
        Path::new(&out_dir).join("component_tests_generated.rs"),
        all_tests,
    )
    .unwrap();
}
