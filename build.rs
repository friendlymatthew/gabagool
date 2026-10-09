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
    use std::{env, fs, path::Path};

    use wast::core::{NanPattern, WastArgCore, WastRetCore};
    use wast::lexer::Lexer;
    use wast::parser::ParseBuffer;
    use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastRet, Wat};

    #[derive(Debug, Clone, Copy)]
    enum SkipReason {
        Suspension,
        Threads,
        UnsupportedExpectedAlternative,
        UnsupportedGcInstruction,
        UnsupportedModuleInstances,
        UnsupportedSimdInstruction,
        UnsupportedSimdValue,
        UnsupportedTextFormat,
    }

    impl SkipReason {
        const fn description(self) -> &'static str {
            match self {
                Self::Suspension => "suspension is not supported",
                Self::Threads => "threads are not supported",
                Self::UnsupportedExpectedAlternative => {
                    "alternative expected values are not supported by the runner"
                }
                Self::UnsupportedGcInstruction => "GC instructions are not supported",
                Self::UnsupportedModuleInstances => {
                    "module definitions and instances are not supported by the runner"
                }
                Self::UnsupportedSimdInstruction => "SIMD instruction is not supported",
                Self::UnsupportedSimdValue => "SIMD arguments and results are not supported",
                Self::UnsupportedTextFormat => {
                    "text-format malformed and invalid modules are not supported"
                }
            }
        }
    }

    pub fn generate() {
        println!("cargo::rerun-if-changed=tests/spec");

        let out_dir = env::var("OUT_DIR").unwrap();
        let wasm_dir = Path::new(&out_dir).join("wasm");
        fs::create_dir_all(&wasm_dir).unwrap();

        let spec_dir = Path::new("tests/spec");
        assert!(
            spec_dir.exists(),
            "core spec fixtures are missing; run `uv run download-core-tests.py`"
        );

        let mut entries = fs::read_dir(spec_dir)
            .unwrap()
            .map(|entry| {
                entry.unwrap_or_else(|error| panic!("failed to read a core spec entry: {error}"))
            })
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wast"))
            .collect::<Vec<_>>();
        assert!(!entries.is_empty(), "no core spec fixtures were found");
        entries.sort_by_key(|entry| entry.path());

        let mut generated = String::new();

        for entry in entries {
            let path = entry.path();
            let file_stem = path.file_stem().unwrap().to_str().unwrap();
            let safe_name = file_stem.replace('-', "_");
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let mut lexer = Lexer::new(&contents);
            lexer.allow_confusing_unicode(true);
            let buffer = ParseBuffer::new_with_lexer(lexer)
                .unwrap_or_else(|error| panic!("failed to lex {}: {error}", path.display()));
            let wast = wast::parser::parse::<Wast>(&buffer)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));

            generated.push_str(&format!(
                "#[test]\nfn r#{safe_name}() {{\n    run_script({file_stem:?}, |script| {{\n"
            ));

            let unsupported_file = unsupported_feature(&safe_name);
            let mut module_idx = 0u32;
            let mut malformed_idx = 0u32;
            let mut invalid_idx = 0u32;
            let mut unlinkable_idx = 0u32;
            let mut trap_module_idx = 0u32;
            let mut return_module_idx = 0u32;

            for directive in wast.directives {
                let (line, column) = directive.span().linecol_in(&contents);
                let location = format!(
                    "{file_stem}.wast:{}:{}",
                    line.saturating_add(1),
                    column.saturating_add(1)
                );
                let is_assertion = is_assertion_directive(&directive);

                if let Some(reason) = unsupported_file {
                    push_skip(&mut generated, &location, is_assertion, reason, false);
                    continue;
                }

                match directive {
                    WastDirective::Module(mut wat) => {
                        let id = quote_wat_id(&wat);
                        let bytes = wat.encode().unwrap_or_else(|error| {
                            panic!("failed to encode {}: {error}", path.display())
                        });
                        let filename = format!("{safe_name}_module_{module_idx}.wasm");
                        fs::write(wasm_dir.join(&filename), bytes).unwrap();
                        let id = render_target(id.as_deref());
                        generated.push_str(&format!(
                            "        script.directive({location:?}, |context| context.instantiate_module(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{filename}\")), {id}));\n"
                        ));
                        module_idx += 1;
                    }
                    WastDirective::Register { name, module, .. } => {
                        let target = render_target(module.as_ref().map(|id| id.name()));
                        generated.push_str(&format!(
                            "        script.directive({location:?}, |context| context.register({name:?}, {target}));\n"
                        ));
                    }
                    WastDirective::Invoke(invoke) => {
                        let target = render_target(invoke.module.as_ref().map(|id| id.name()));
                        match render_args(&invoke.args) {
                            Ok(args) => generated.push_str(&format!(
                                "        script.directive({location:?}, |context| spec_invoke(context, {target}, {:?}, &[{args}]).map(|_| ()));\n",
                                invoke.name
                            )),
                            Err(reason) => push_skip(
                                &mut generated,
                                &location,
                                false,
                                reason,
                                true,
                            ),
                        }
                    }
                    WastDirective::AssertReturn { exec, results, .. } => match exec {
                        WastExecute::Invoke(invoke) => {
                            let target =
                                render_target(invoke.module.as_ref().map(|id| id.name()));
                            match (render_args(&invoke.args), render_expected(&results)) {
                                (Ok(args), Ok(expected)) => generated.push_str(&format!(
                                    "        script.assertion({location:?}, |context| spec_assert_return(context, {target}, {:?}, &[{args}], &[{expected}]));\n",
                                    invoke.name
                                )),
                                (Ok(_), Err(reason)) => push_skip(
                                    &mut generated,
                                    &location,
                                    true,
                                    reason,
                                    true,
                                ),
                                (Err(reason), _) => push_skip(
                                    &mut generated,
                                    &location,
                                    true,
                                    reason,
                                    true,
                                ),
                            }
                        }
                        WastExecute::Get { module, global, .. } => {
                            let target = render_target(module.as_ref().map(|id| id.name()));
                            match render_expected(&results) {
                                Ok(expected) => generated.push_str(&format!(
                                    "        script.assertion({location:?}, |context| spec_assert_get(context, {target}, {global:?}, &[{expected}]));\n"
                                )),
                                Err(reason) => push_skip(
                                    &mut generated,
                                    &location,
                                    true,
                                    reason,
                                    false,
                                ),
                            }
                        }
                        WastExecute::Wat(mut wat) => {
                            let bytes = wat.encode().unwrap_or_else(|error| {
                                panic!("failed to encode {}: {error}", path.display())
                            });
                            let filename =
                                format!("{safe_name}_return_module_{return_module_idx}.wasm");
                            fs::write(wasm_dir.join(&filename), bytes).unwrap();
                            generated.push_str(&format!(
                                "        script.assertion({location:?}, |context| spec_assert_module_return(context, include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{filename}\"))));\n"
                            ));
                            return_module_idx += 1;
                        }
                    },
                    WastDirective::AssertTrap { exec, .. } => match exec {
                        WastExecute::Invoke(invoke) => {
                            let target =
                                render_target(invoke.module.as_ref().map(|id| id.name()));
                            match render_args(&invoke.args) {
                                Ok(args) => generated.push_str(&format!(
                                    "        script.assertion({location:?}, |context| spec_assert_trap(context, {target}, {:?}, &[{args}]));\n",
                                    invoke.name
                                )),
                                Err(reason) => push_skip(
                                    &mut generated,
                                    &location,
                                    true,
                                    reason,
                                    true,
                                ),
                            }
                        }
                        WastExecute::Wat(mut wat) => {
                            let bytes = wat.encode().unwrap_or_else(|error| {
                                panic!("failed to encode {}: {error}", path.display())
                            });
                            let filename =
                                format!("{safe_name}_trap_module_{trap_module_idx}.wasm");
                            fs::write(wasm_dir.join(&filename), bytes).unwrap();
                            generated.push_str(&format!(
                                "        script.assertion({location:?}, |context| spec_assert_module_trap(context, include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{filename}\"))));\n"
                            ));
                            trap_module_idx += 1;
                        }
                        WastExecute::Get { .. } => generated.push_str(&format!(
                            "        script.assertion({location:?}, |_| Err(\"global.get cannot trap\".to_string()));\n"
                        )),
                    },
                    WastDirective::AssertExhaustion { call, .. } => {
                        let target = render_target(call.module.as_ref().map(|id| id.name()));
                        match render_args(&call.args) {
                            Ok(args) => generated.push_str(&format!(
                                "        script.assertion({location:?}, |context| spec_assert_exhaustion(context, {target}, {:?}, &[{args}]));\n",
                                call.name
                            )),
                            Err(reason) => push_skip(
                                &mut generated,
                                &location,
                                true,
                                reason,
                                true,
                            ),
                        }
                    }
                    WastDirective::AssertException { exec, .. } => match exec {
                        WastExecute::Invoke(invoke) => {
                            let target =
                                render_target(invoke.module.as_ref().map(|id| id.name()));
                            match render_args(&invoke.args) {
                                Ok(args) => generated.push_str(&format!(
                                    "        script.assertion({location:?}, |context| spec_assert_exception(context, {target}, {:?}, &[{args}]));\n",
                                    invoke.name
                                )),
                                Err(reason) => push_skip(
                                    &mut generated,
                                    &location,
                                    true,
                                    reason,
                                    true,
                                ),
                            }
                        }
                        WastExecute::Get { .. } | WastExecute::Wat(_) => generated.push_str(
                            &format!("        script.assertion({location:?}, |_| Err(\"expected an exception from an unsupported execution form\".to_string()));\n"),
                        ),
                    },
                    WastDirective::AssertMalformed { mut module, .. } => {
                        if matches!(&module, QuoteWat::QuoteModule(..)) {
                            push_skip(
                                &mut generated,
                                &location,
                                true,
                                SkipReason::UnsupportedTextFormat,
                                false,
                            );
                        } else {
                            let bytes = module.encode().unwrap_or_else(|error| {
                                panic!("failed to encode {}: {error}", path.display())
                            });
                            let filename = format!("{safe_name}_malformed_{malformed_idx}.wasm");
                            fs::write(wasm_dir.join(&filename), bytes).unwrap();
                            generated.push_str(&format!(
                                "        script.assertion({location:?}, |_| spec_assert_malformed(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{filename}\"))));\n"
                            ));
                        }
                        malformed_idx += 1;
                    }
                    WastDirective::AssertInvalid { mut module, .. } => {
                        if matches!(&module, QuoteWat::QuoteModule(..)) {
                            push_skip(
                                &mut generated,
                                &location,
                                true,
                                SkipReason::UnsupportedTextFormat,
                                false,
                            );
                        } else if safe_name == "simd" {
                            push_skip(
                                &mut generated,
                                &location,
                                true,
                                SkipReason::UnsupportedSimdInstruction,
                                false,
                            );
                        } else {
                            let bytes = module.encode().unwrap_or_else(|error| {
                                panic!("failed to encode {}: {error}", path.display())
                            });
                            let filename = format!("{safe_name}_invalid_{invalid_idx}.wasm");
                            fs::write(wasm_dir.join(&filename), bytes).unwrap();
                            generated.push_str(&format!(
                                "        script.assertion({location:?}, |_| spec_assert_invalid(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{filename}\"))));\n"
                            ));
                        }
                        invalid_idx += 1;
                    }
                    WastDirective::AssertUnlinkable { mut module, .. } => {
                        let bytes = module.encode().unwrap_or_else(|error| {
                            panic!("failed to encode {}: {error}", path.display())
                        });
                        let filename = format!("{safe_name}_unlinkable_{unlinkable_idx}.wasm");
                        fs::write(wasm_dir.join(&filename), bytes).unwrap();
                        generated.push_str(&format!(
                            "        script.assertion({location:?}, |context| spec_assert_unlinkable(context, include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{filename}\"))));\n"
                        ));
                        unlinkable_idx += 1;
                    }
                    WastDirective::ModuleDefinition(_) | WastDirective::ModuleInstance { .. } => {
                        push_skip(
                            &mut generated,
                            &location,
                            false,
                            SkipReason::UnsupportedModuleInstances,
                            true,
                        );
                    }
                    WastDirective::AssertSuspension { .. } => push_skip(
                        &mut generated,
                        &location,
                        true,
                        SkipReason::Suspension,
                        true,
                    ),
                    WastDirective::Thread(_) | WastDirective::Wait { .. } => push_skip(
                        &mut generated,
                        &location,
                        false,
                        SkipReason::Threads,
                        true,
                    ),
                }
            }

            generated.push_str("    });\n}\n\n");
        }

        fs::write(
            Path::new(&out_dir).join("core_tests_generated.rs"),
            generated,
        )
        .unwrap();
    }

    fn push_skip(
        generated: &mut String,
        location: &str,
        is_assertion: bool,
        reason: SkipReason,
        blocks_script: bool,
    ) {
        generated.push_str(&format!(
            "        script.skip({location:?}, {is_assertion}, {:?}, {blocks_script});\n",
            reason.description()
        ));
    }

    fn quote_wat_id(wat: &QuoteWat<'_>) -> Option<String> {
        match wat {
            QuoteWat::Wat(Wat::Module(module)) => module.id.as_ref().map(|id| id.name().to_owned()),
            QuoteWat::Wat(Wat::Component(_))
            | QuoteWat::QuoteModule(..)
            | QuoteWat::QuoteComponent(..) => None,
        }
    }

    fn render_target(target: Option<&str>) -> String {
        target.map_or_else(|| "None".to_string(), |name| format!("Some({name:?})"))
    }

    fn unsupported_feature(test_name: &str) -> Option<SkipReason> {
        const GC_PREFIXES: &[&str] = &[
            "array_",
            "br_on_cast_",
            "br_on_cast_fail_",
            "extern_",
            "i31_",
            "ref_cast_",
            "ref_eq_",
            "ref_test_",
            "struct_",
        ];
        if GC_PREFIXES.iter().any(|prefix| {
            test_name.starts_with(prefix)
                || prefix
                    .strip_suffix('_')
                    .is_some_and(|name| name == test_name)
        }) || test_name == "type_subtyping"
        {
            return Some(SkipReason::UnsupportedGcInstruction);
        }

        (test_name == "simd"
            || test_name.starts_with("simd_")
            || test_name.starts_with("relaxed_")
            || test_name.contains("_relaxed_"))
        .then_some(SkipReason::UnsupportedSimdInstruction)
    }

    const fn is_assertion_directive(directive: &WastDirective<'_>) -> bool {
        matches!(
            directive,
            WastDirective::AssertMalformed { .. }
                | WastDirective::AssertInvalid { .. }
                | WastDirective::AssertUnlinkable { .. }
                | WastDirective::AssertTrap { .. }
                | WastDirective::AssertReturn { .. }
                | WastDirective::AssertExhaustion { .. }
                | WastDirective::AssertException { .. }
                | WastDirective::AssertSuspension { .. }
        )
    }

    fn render_i32(value: i32) -> String {
        if value == i32::MIN {
            "i32::MIN".to_string()
        } else {
            format!("{value}i32")
        }
    }

    fn render_i64(value: i64) -> String {
        if value == i64::MIN {
            "i64::MIN".to_string()
        } else {
            format!("{value}i64")
        }
    }

    fn render_args(args: &[WastArg<'_>]) -> Result<String, SkipReason> {
        args.iter()
            .map(|arg| match arg {
                WastArg::Core(WastArgCore::I32(value)) => {
                    Ok(format!("RawValue::from({})", render_i32(*value)))
                }
                WastArg::Core(WastArgCore::I64(value)) => {
                    Ok(format!("RawValue::from({})", render_i64(*value)))
                }
                WastArg::Core(WastArgCore::F32(value)) => {
                    Ok(format!("RawValue::from(f32::from_bits({}))", value.bits))
                }
                WastArg::Core(WastArgCore::F64(value)) => {
                    Ok(format!("RawValue::from(f64::from_bits({}))", value.bits))
                }
                WastArg::Core(WastArgCore::RefNull(_)) => {
                    Ok("RawValue::from_ref(Ref::Null)".to_string())
                }
                WastArg::Core(WastArgCore::RefExtern(value))
                | WastArg::Core(WastArgCore::RefHost(value)) => Ok(format!(
                    "RawValue::from_ref(Ref::RefExtern(usize::try_from({value}u32).unwrap()))"
                )),
                WastArg::Core(WastArgCore::V128(_)) => Err(SkipReason::UnsupportedSimdValue),
                _ => panic!("unclassified core spec argument: {arg:?}"),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|values| values.join(", "))
    }

    fn render_expected(results: &[WastRet<'_>]) -> Result<String, SkipReason> {
        results
            .iter()
            .map(|result| match result {
                WastRet::Core(WastRetCore::I32(value)) => {
                    Ok(format!("ExpectedValue::I32({})", render_i32(*value)))
                }
                WastRet::Core(WastRetCore::I64(value)) => {
                    Ok(format!("ExpectedValue::I64({})", render_i64(*value)))
                }
                WastRet::Core(WastRetCore::F32(pattern)) => Ok(match pattern {
                    NanPattern::CanonicalNan => {
                        "ExpectedValue::F32(NanPat::CanonicalNan)".to_string()
                    }
                    NanPattern::ArithmeticNan => {
                        "ExpectedValue::F32(NanPat::ArithmeticNan)".to_string()
                    }
                    NanPattern::Value(value) => {
                        format!("ExpectedValue::F32(NanPat::Value({}))", value.bits)
                    }
                }),
                WastRet::Core(WastRetCore::F64(pattern)) => Ok(match pattern {
                    NanPattern::CanonicalNan => {
                        "ExpectedValue::F64(NanPat::CanonicalNan)".to_string()
                    }
                    NanPattern::ArithmeticNan => {
                        "ExpectedValue::F64(NanPat::ArithmeticNan)".to_string()
                    }
                    NanPattern::Value(value) => {
                        format!("ExpectedValue::F64(NanPat::Value({}))", value.bits)
                    }
                }),
                WastRet::Core(WastRetCore::RefNull(_)) => {
                    Ok("ExpectedValue::Ref(ExpectedRef::Null)".to_string())
                }
                WastRet::Core(WastRetCore::RefExtern(Some(value))) => Ok(format!(
                    "ExpectedValue::Ref(ExpectedRef::Extern(Some({value})))"
                )),
                WastRet::Core(WastRetCore::RefExtern(None)) => {
                    Ok("ExpectedValue::Ref(ExpectedRef::Extern(None))".to_string())
                }
                WastRet::Core(WastRetCore::RefHost(value)) => Ok(format!(
                    "ExpectedValue::Ref(ExpectedRef::Extern(Some({value})))"
                )),
                WastRet::Core(WastRetCore::RefFunc(_)) => {
                    Ok("ExpectedValue::Ref(ExpectedRef::Func)".to_string())
                }
                WastRet::Core(
                    WastRetCore::RefAny
                    | WastRetCore::RefEq
                    | WastRetCore::RefStruct
                    | WastRetCore::RefArray
                    | WastRetCore::RefI31
                    | WastRetCore::RefI31Shared,
                ) => Err(SkipReason::UnsupportedGcInstruction),
                WastRet::Core(WastRetCore::V128(_)) => Err(SkipReason::UnsupportedSimdValue),
                WastRet::Core(WastRetCore::Either(_)) => {
                    Err(SkipReason::UnsupportedExpectedAlternative)
                }
                _ => panic!("unclassified core spec expected value: {result:?}"),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|values| values.join(", "))
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
