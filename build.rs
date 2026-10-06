fn main() {
    #[cfg(feature = "core-tests")]
    core_tests::generate();

    #[cfg(not(feature = "core-tests"))]
    {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        std::fs::write(
            std::path::Path::new(&out_dir).join("core_tests_generated.rs"),
            "",
        )
        .unwrap();
    }

    #[cfg(feature = "component-tests")]
    component_tests::generate();

    #[cfg(not(feature = "component-tests"))]
    {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        std::fs::write(
            std::path::Path::new(&out_dir).join("component_tests_generated.rs"),
            "",
        )
        .unwrap();
    }

    #[cfg(feature = "jit")]
    jit::generate();

    #[cfg(not(feature = "jit"))]
    {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        std::fs::write(
            std::path::Path::new(&out_dir).join("stencils_generated.rs"),
            "",
        )
        .unwrap();
    }
}

#[cfg(feature = "component-tests")]
mod component_tests {
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
}

#[cfg(feature = "core-tests")]
mod core_tests {
    use std::env;
    use std::fs;
    use std::path::Path;

    use wast::core::{NanPattern, WastArgCore, WastRetCore};
    use wast::parser::ParseBuffer;
    use wast::{Wast, WastArg, WastDirective, WastExecute, WastRet};

    pub fn generate() {
        println!("cargo::rerun-if-changed=tests/spec");

        let out_dir = env::var("OUT_DIR").unwrap();
        let wasm_dir = Path::new(&out_dir).join("wasm");
        fs::create_dir_all(&wasm_dir).unwrap();

        let spec_dir = Path::new("tests/spec");
        if !spec_dir.exists() {
            fs::write(Path::new(&out_dir).join("core_tests_generated.rs"), "").unwrap();
            return;
        }

        let mut all_tests = String::new();

        let entries = fs::read_dir(spec_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "wast"));

        for entry in entries {
            let path = entry.path();
            let file_stem = path.file_stem().unwrap().to_str().unwrap();
            let safe_name = file_stem.replace('-', "_");

            let Ok(contents) = fs::read_to_string(&path) else {
                println!("cargo::warning=skipping {}: failed to read", path.display());
                continue;
            };
            let Ok(buf) = ParseBuffer::new(&contents) else {
                println!("cargo::warning=skipping {}: failed to lex", path.display());
                continue;
            };
            let Ok(wast) = wast::parser::parse::<Wast>(&buf) else {
                println!(
                    "cargo::warning=skipping {}: failed to parse",
                    path.display()
                );
                continue;
            };

            let mut module_idx: i32 = -1;
            let mut modules = Vec::new();
            // Track (register ...) directives: maps registered name -> module index
            let mut registered: Vec<(String, i32)> = Vec::new();
            let mut malformed_idx: u32 = 0;
            let mut unlinkable_idx: u32 = 0;
            let mut trap_module_idx: u32 = 0;

            for directive in wast.directives {
                match directive {
                    WastDirective::Module(mut wat) => {
                        module_idx += 1;
                        if let Ok(bytes) = wat.encode() {
                            let wasm_path =
                                wasm_dir.join(format!("{}_{}.wasm", safe_name, module_idx));
                            fs::write(&wasm_path, bytes).unwrap();
                        }
                        modules.push((module_idx, Vec::new()));
                    }

                    WastDirective::Register { name, .. } => {
                        if module_idx >= 0 {
                            registered.push((name.to_string(), module_idx));
                        }
                    }

                    WastDirective::AssertReturn { exec, results, .. } => {
                        if module_idx < 0 {
                            continue;
                        }
                        let WastExecute::Invoke(ref invoke) = exec else {
                            continue;
                        };
                        if invoke.module.is_some() {
                            continue;
                        }

                        let Some(args_code) = render_args(&invoke.args) else {
                            continue;
                        };
                        let Some(expected_code) = render_expected(&results) else {
                            continue;
                        };

                        let steps = &mut modules.last_mut().unwrap().1;
                        let step_idx = steps.len();
                        steps.push(format!(
                            "    spec_step_assert_return(&mut store, instance, \"{}\", &[{}], &[{}], {}, &mut failures);",
                            invoke.name, args_code, expected_code, step_idx
                        ));
                    }

                    WastDirective::AssertTrap { exec, .. } => match exec {
                        WastExecute::Invoke(ref invoke) => {
                            if module_idx < 0 {
                                continue;
                            }
                            if invoke.module.is_some() {
                                continue;
                            }

                            let Some(args_code) = render_args(&invoke.args) else {
                                continue;
                            };

                            let steps = &mut modules.last_mut().unwrap().1;
                            let step_idx = steps.len();
                            steps.push(format!(
                                    "    spec_step_assert_trap(&mut store, instance, \"{}\", &[{}], {}, &mut failures);",
                                    invoke.name, args_code, step_idx
                                ));
                        }
                        WastExecute::Wat(mut wat) => {
                            let Ok(bytes) = wat.encode() else {
                                trap_module_idx += 1;
                                continue;
                            };
                            let wasm_path = wasm_dir.join(format!(
                                "trap_module_{}_{}.wasm",
                                safe_name, trap_module_idx
                            ));
                            fs::write(&wasm_path, bytes).unwrap();
                            let test_name =
                                format!("trap_module_{}_{}", safe_name, trap_module_idx);
                            all_tests.push_str(&format!(
                                    concat!(
                                        "#[test]\n",
                                        "fn {test_name}() {{\n",
                                        "    let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/trap_module_{file}_{idx}.wasm\"));\n",
                                        "    let module = Module::new(wasm_bytes).unwrap();\n",
                                        "    let mut store = Store::new();\n",
                                        "    let imports = setup_spectest_imports(&mut store, &module);\n",
                                        "    let result = store.instantiate(&module, imports);\n",
                                        "    assert!(result.is_err(), \"expected module instantiation to trap, but it succeeded\");\n",
                                        "}}\n",
                                    ),
                                    test_name = test_name,
                                    file = safe_name,
                                    idx = trap_module_idx,
                                ));
                            trap_module_idx += 1;
                        }
                        _ => {}
                    },

                    WastDirective::AssertExhaustion {
                        call: ref invoke, ..
                    } => {
                        if module_idx < 0 {
                            continue;
                        }
                        if invoke.module.is_some() {
                            continue;
                        }

                        let Some(args_code) = render_args(&invoke.args) else {
                            continue;
                        };

                        let steps = &mut modules.last_mut().unwrap().1;
                        let step_idx = steps.len();
                        steps.push(format!(
                            "    spec_step_assert_trap(&mut store, instance, \"{}\", &[{}], {}, &mut failures);",
                            invoke.name, args_code, step_idx
                        ));
                    }

                    WastDirective::Invoke(ref invoke) => {
                        if module_idx < 0 {
                            continue;
                        }
                        if invoke.module.is_some() {
                            continue;
                        }

                        let Some(args_code) = render_args(&invoke.args) else {
                            continue;
                        };

                        let steps = &mut modules.last_mut().unwrap().1;
                        steps.push(format!(
                            "    spec_step_invoke(&mut store, instance, \"{}\", &[{}]);",
                            invoke.name, args_code
                        ));
                    }

                    WastDirective::AssertMalformed { mut module, .. } => {
                        let Ok(bytes) = module.encode() else {
                            malformed_idx += 1;
                            continue;
                        };
                        let wasm_path = wasm_dir
                            .join(format!("malformed_{}_{}.wasm", safe_name, malformed_idx));
                        fs::write(&wasm_path, bytes).unwrap();
                        let test_name = format!("malformed_{}_{}", safe_name, malformed_idx);
                        all_tests.push_str(&format!(
                            concat!(
                                "#[test]\n",
                                "fn {test_name}() {{\n",
                                "    let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/malformed_{file}_{idx}.wasm\"));\n",
                                "    let result = Parser::new(wasm_bytes).parse();\n",
                                "    assert!(result.is_err(), \"expected malformed module to fail parsing, but it succeeded\");\n",
                                "}}\n",
                            ),
                            test_name = test_name,
                            file = safe_name,
                            idx = malformed_idx,
                        ));
                        malformed_idx += 1;
                    }

                    WastDirective::AssertUnlinkable { mut module, .. } => {
                        let Ok(bytes) = module.encode() else {
                            unlinkable_idx += 1;
                            continue;
                        };
                        let wasm_path = wasm_dir
                            .join(format!("unlinkable_{}_{}.wasm", safe_name, unlinkable_idx));
                        fs::write(&wasm_path, bytes).unwrap();
                        let test_name = format!("unlinkable_{}_{}", safe_name, unlinkable_idx);

                        let prereq_registered: Vec<(String, i32)> = registered.clone();

                        let mut prereq_indices: Vec<i32> = Vec::new();
                        for (_, dep_idx) in &prereq_registered {
                            if !prereq_indices.contains(dep_idx) {
                                prereq_indices.push(*dep_idx);
                            }
                        }
                        prereq_indices.sort();

                        let mut setup = String::new();
                        for pidx in &prereq_indices {
                            setup.push_str(&format!(
                                concat!(
                                    "    let prereq_wasm_{pidx}: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{file}_{pidx}.wasm\"));\n",
                                    "    let prereq_module_{pidx} = Module::new(prereq_wasm_{pidx}).unwrap();\n",
                                    "    let prereq_imports_{pidx} = setup_spectest_imports(&mut store, &prereq_module_{pidx});\n",
                                    "    let prereq_instance_{pidx} = store.instantiate(&prereq_module_{pidx}, prereq_imports_{pidx}).unwrap();\n",
                                    "    let prereq_exports_{pidx}: Vec<ExportInstance> = store.exports(prereq_instance_{pidx}).to_vec();\n",
                                ),
                                pidx = pidx,
                                file = safe_name,
                            ));
                        }

                        if !prereq_registered.is_empty() {
                            setup.push_str(
                                "    let registered_exports: Vec<(&str, &[ExportInstance])> = vec![",
                            );
                            for (name, dep_idx) in &prereq_registered {
                                setup.push_str(&format!(
                                    "(\"{}\", &prereq_exports_{}), ",
                                    name, dep_idx
                                ));
                            }
                            setup.push_str("];\n");
                            setup.push_str("    let resolve_result = try_resolve_imports_with_registered(&module, &registered_exports);\n");
                        } else {
                            setup.push_str("    let resolve_result = try_resolve_spectest_imports(&mut store, &module);\n");
                        }

                        all_tests.push_str(&format!(
                            concat!(
                                "#[test]\n",
                                "fn {test_name}() {{\n",
                                "    let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/unlinkable_{file}_{idx}.wasm\"));\n",
                                "    let module = Module::new(wasm_bytes).unwrap();\n",
                                "    let mut store = Store::new();\n",
                                "{setup}",
                                "    let result = resolve_result.and_then(|imports| store.instantiate(&module, imports));\n",
                                "    assert!(result.is_err(), \"expected unlinkable module to fail instantiation, but it succeeded\");\n",
                                "}}\n",
                            ),
                            test_name = test_name,
                            file = safe_name,
                            idx = unlinkable_idx,
                            setup = setup,
                        ));
                        unlinkable_idx += 1;
                    }

                    WastDirective::AssertException { exec, .. } => {
                        if let WastExecute::Invoke(ref invoke) = exec {
                            if module_idx < 0 || invoke.module.is_some() {
                                continue;
                            }

                            let Some(args_code) = render_args(&invoke.args) else {
                                continue;
                            };

                            let steps = &mut modules.last_mut().unwrap().1;
                            let step_idx = steps.len();
                            steps.push(format!(
                                "    spec_step_assert_exception(&mut store, instance, \"{}\", &[{}], {}, &mut failures);",
                                invoke.name, args_code, step_idx
                            ));
                        }
                    }

                    _ => {}
                }
            }

            // Build a map: module_idx -> list of registered modules that precede it
            let mut registered_before: std::collections::BTreeMap<i32, Vec<(String, i32)>> =
                std::collections::BTreeMap::new();
            for &(midx, ref _steps) in &modules {
                let deps: Vec<(String, i32)> = registered
                    .iter()
                    .filter(|(_, ridx)| *ridx < midx)
                    .cloned()
                    .collect();
                if !deps.is_empty() {
                    registered_before.insert(midx, deps);
                }
            }

            for (midx, steps) in &modules {
                if steps.is_empty() {
                    continue;
                }
                let test_name = format!("{}_{}", safe_name, midx);
                let steps_code = steps.join("\n");

                // Generate prerequisite setup code for registered modules
                let deps = registered_before.get(midx);
                let has_deps = deps.is_some_and(|d| !d.is_empty());

                let setup_code = if has_deps {
                    let deps = deps.unwrap();
                    // Collect unique prerequisite module indices (in order)
                    let mut prereq_indices: Vec<i32> = Vec::new();
                    for (_, dep_idx) in deps {
                        if !prereq_indices.contains(dep_idx) {
                            prereq_indices.push(*dep_idx);
                        }
                    }
                    prereq_indices.sort();

                    let mut setup = String::new();
                    // Instantiate each prerequisite module
                    for pidx in &prereq_indices {
                        setup.push_str(&format!(
                            concat!(
                                "    let prereq_wasm_{pidx}: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{file}_{pidx}.wasm\"));\n",
                                "    let prereq_module_{pidx} = Module::new(prereq_wasm_{pidx}).unwrap();\n",
                                "    let prereq_imports_{pidx} = setup_spectest_imports(&mut store, &prereq_module_{pidx});\n",
                                "    let prereq_instance_{pidx} = store.instantiate(&prereq_module_{pidx}, prereq_imports_{pidx}).unwrap();\n",
                                "    let prereq_exports_{pidx}: Vec<ExportInstance> = store.exports(prereq_instance_{pidx}).to_vec();\n",
                            ),
                            pidx = pidx,
                            file = safe_name,
                        ));
                    }

                    // Build the registered_exports vec
                    setup.push_str(
                        "    let registered_exports: Vec<(&str, &[ExportInstance])> = vec![",
                    );
                    for (name, dep_idx) in deps {
                        setup.push_str(&format!("(\"{}\", &prereq_exports_{}), ", name, dep_idx));
                    }
                    setup.push_str("];\n");

                    // Resolve imports using registered modules
                    setup.push_str("    let imports = resolve_imports_with_registered(&mut store, &module, &registered_exports);\n");
                    setup
                } else {
                    "    let imports = setup_spectest_imports(&mut store, &module);\n".to_string()
                };

                let uses_failures = steps.iter().any(|s| s.contains("failures"));

                if uses_failures {
                    all_tests.push_str(&format!(
                        concat!(
                            "#[test]\n",
                            "fn {test_name}() {{\n",
                            "    let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{file}_{midx}.wasm\"));\n",
                            "    let module = Module::new(wasm_bytes).unwrap();\n",
                            "    let mut store = Store::new();\n",
                            "{setup}",
                            "    let instance = store.instantiate(&module, imports).unwrap();\n",
                            "    let mut failures = Vec::new();\n",
                            "{steps}\n",
                            "    if !failures.is_empty() {{\n",
                            "        panic!(\"{{}} assertion(s) failed in {test_name}:\\n{{}}\", failures.len(), failures.join(\"\\n\"));\n",
                            "    }}\n",
                            "}}\n",
                        ),
                        test_name = test_name,
                        file = safe_name,
                        midx = midx,
                        setup = setup_code,
                        steps = steps_code,
                    ));
                } else {
                    all_tests.push_str(&format!(
                        concat!(
                            "#[test]\n",
                            "fn {test_name}() {{\n",
                            "    let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{file}_{midx}.wasm\"));\n",
                            "    let module = Module::new(wasm_bytes).unwrap();\n",
                            "    let mut store = Store::new();\n",
                            "{setup}",
                            "    let instance = store.instantiate(&module, imports).unwrap();\n",
                            "{steps}\n",
                            "}}\n",
                        ),
                        test_name = test_name,
                        file = safe_name,
                        midx = midx,
                        setup = setup_code,
                        steps = steps_code,
                    ));
                }
            }
        }

        fs::write(
            Path::new(&out_dir).join("core_tests_generated.rs"),
            all_tests,
        )
        .unwrap();
    }

    fn render_i32(v: i32) -> String {
        if v == i32::MIN {
            "i32::MIN".to_string()
        } else {
            format!("{}i32", v)
        }
    }

    fn render_i64(v: i64) -> String {
        if v == i64::MIN {
            "i64::MIN".to_string()
        } else {
            format!("{}i64", v)
        }
    }

    fn render_args(args: &[WastArg]) -> Option<String> {
        let rendered: Option<Vec<String>> = args
            .iter()
            .map(|arg| match arg {
                WastArg::Core(WastArgCore::I32(v)) => {
                    Some(format!("RawValue::from({})", render_i32(*v)))
                }
                WastArg::Core(WastArgCore::I64(v)) => {
                    Some(format!("RawValue::from({})", render_i64(*v)))
                }
                WastArg::Core(WastArgCore::F32(v)) => {
                    Some(format!("RawValue::from(f32::from_bits({}))", v.bits))
                }
                WastArg::Core(WastArgCore::F64(v)) => {
                    Some(format!("RawValue::from(f64::from_bits({}))", v.bits))
                }
                WastArg::Core(WastArgCore::RefNull(_)) => {
                    Some("RawValue::from_ref(Ref::Null)".to_string())
                }
                WastArg::Core(WastArgCore::RefExtern(n)) => Some(format!(
                    "RawValue::from_ref(Ref::RefExtern({} as usize))",
                    n
                )),
                WastArg::Core(WastArgCore::RefHost(n)) => Some(format!(
                    "RawValue::from_ref(Ref::RefExtern({} as usize))",
                    n
                )),
                _ => None,
            })
            .collect();
        rendered.map(|v| v.join(", "))
    }

    fn render_expected(results: &[WastRet]) -> Option<String> {
        let rendered: Option<Vec<String>> = results
            .iter()
            .map(|ret| match ret {
                WastRet::Core(WastRetCore::I32(v)) => {
                    Some(format!("ExpectedValue::I32({})", render_i32(*v)))
                }
                WastRet::Core(WastRetCore::I64(v)) => {
                    Some(format!("ExpectedValue::I64({})", render_i64(*v)))
                }
                WastRet::Core(WastRetCore::F32(np)) => Some(match np {
                    NanPattern::CanonicalNan => {
                        "ExpectedValue::F32(NanPat::CanonicalNan)".to_string()
                    }
                    NanPattern::ArithmeticNan => {
                        "ExpectedValue::F32(NanPat::ArithmeticNan)".to_string()
                    }
                    NanPattern::Value(v) => {
                        format!("ExpectedValue::F32(NanPat::Value({}))", v.bits)
                    }
                }),
                WastRet::Core(WastRetCore::F64(np)) => Some(match np {
                    NanPattern::CanonicalNan => {
                        "ExpectedValue::F64(NanPat::CanonicalNan)".to_string()
                    }
                    NanPattern::ArithmeticNan => {
                        "ExpectedValue::F64(NanPat::ArithmeticNan)".to_string()
                    }
                    NanPattern::Value(v) => {
                        format!("ExpectedValue::F64(NanPat::Value({}))", v.bits)
                    }
                }),
                WastRet::Core(WastRetCore::RefNull(_)) => {
                    Some("ExpectedValue::Ref(ExpectedRef::Null)".to_string())
                }
                WastRet::Core(WastRetCore::RefExtern(Some(n))) => Some(format!(
                    "ExpectedValue::Ref(ExpectedRef::Extern(Some({})))",
                    n
                )),
                WastRet::Core(WastRetCore::RefExtern(None)) => {
                    Some("ExpectedValue::Ref(ExpectedRef::Extern(None))".to_string())
                }
                WastRet::Core(WastRetCore::RefHost(n)) => Some(format!(
                    "ExpectedValue::Ref(ExpectedRef::Extern(Some({})))",
                    n
                )),
                WastRet::Core(WastRetCore::RefFunc(_)) => {
                    Some("ExpectedValue::Ref(ExpectedRef::Func)".to_string())
                }
                WastRet::Core(
                    WastRetCore::RefAny
                    | WastRetCore::RefEq
                    | WastRetCore::RefStruct
                    | WastRetCore::RefArray,
                ) => Some("ExpectedValue::Ref(ExpectedRef::NonNull)".to_string()),
                WastRet::Core(WastRetCore::RefI31 | WastRetCore::RefI31Shared) => {
                    Some("ExpectedValue::Ref(ExpectedRef::I31)".to_string())
                }
                WastRet::Core(WastRetCore::Either(_)) => None,
                _ => None,
            })
            .collect();
        rendered.map(|v| v.join(", "))
    }
}

#[cfg(feature = "jit")]
mod jit {
    use std::{env, fs, path::Path};

    use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};

    fn snake_to_pascal(name: &str) -> String {
        let name = name.strip_suffix('_').unwrap_or(name);

        name.split('_')
            .map(|part| {
                let mut chars = part.chars();

                match chars.next() {
                    Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            })
            .collect()
    }

    pub fn generate() {
        println!("cargo::rerun-if-changed=src/stencils/stencils.c");
        println!("cargo::rerun-if-changed=src/stencils/stencil_context.h");

        let out_dir = env::var("OUT_DIR").unwrap();

        let objects = cc::Build::new()
            .file("src/stencils/stencils.c")
            .include("src/stencils")
            .opt_level(3)
            .flag("-fno-stack-protector")
            .flag("-fno-asynchronous-unwind-tables")
            .flag("-fno-exceptions")
            .cargo_metadata(false)
            .compile_intermediates();

        let objects = objects.first().expect("expect .o file");

        let obj_data = fs::read(objects).expect("should exist");
        let obj_file = object::File::parse(&*obj_data).expect("should parse");

        let text_section = obj_file
            .sections()
            .find(|s| s.name() == Ok("__text") || s.name() == Ok(".text"))
            .expect("text section should exist");

        let text_data = text_section.data().unwrap();
        let text_addr = text_section.address();

        let mut sym_addrs = obj_file
            .symbols()
            .filter(|s| s.kind() == SymbolKind::Text && s.section_index().is_some())
            .filter_map(|s| Some((s.name().ok()?, s.address())))
            .collect::<Vec<_>>();

        sym_addrs.sort_by_key(|&(_, a)| a);

        let stencils = sym_addrs
            .iter()
            .enumerate()
            .filter_map(|(i, (name, _))| {
                let clean = name.strip_prefix('_').unwrap_or(name);
                let should_strip = clean.starts_with("ltmp")
                    || clean.starts_with("Ltmp")
                    || clean.starts_with('.');

                (!should_strip).then_some((clean, i))
            })
            .collect::<Vec<_>>();

        let mut generated = String::new();

        for &(stencil, sym_idx) in &stencils {
            let addr = sym_addrs[sym_idx].1;
            let next_addr = sym_addrs
                .get(sym_idx + 1)
                .map(|s| s.1)
                .unwrap_or(text_addr + text_data.len() as u64);

            let offset = (addr - text_addr) as usize;
            let size = (next_addr - addr) as usize;

            let bs = text_data.get(offset..offset + size).expect("valid bytes");

            generated.push_str(&format!(
                "pub const STENCIL_{}: &[u8] = &{:?};\n",
                stencil.to_uppercase(),
                bs,
            ));
        }

        generated.push_str(
            "\npub const fn stencil_for_op(op: &crate::ir::Op) -> Option<&'static [u8]> {\n",
        );
        generated.push_str("    match op {\n");

        for &(stencil, _) in &stencils {
            generated.push_str(&format!(
                "        crate::ir::Op::{} {{ .. }} => Some(STENCIL_{}),\n",
                snake_to_pascal(stencil),
                stencil.to_uppercase(),
            ));
        }

        generated.push_str("        _ => None,\n");
        generated.push_str("    }\n");
        generated.push_str("}\n");

        fs::write(Path::new(&out_dir).join("stencils_generated.rs"), generated).unwrap();
    }
}
