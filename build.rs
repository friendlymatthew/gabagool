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
    use std::collections::BTreeMap;
    use std::env;
    use std::fs;
    use std::path::Path;

    use wast::core::{NanPattern, WastArgCore, WastRetCore};
    use wast::lexer::Lexer;
    use wast::parser::ParseBuffer;
    use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastRet, Wat};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum SkipReason {
        DependentOnUnsupportedScriptState,
        GlobalGet,
        NamedModuleInvocation,
        NonCurrentRegisterTarget,
        NoCurrentModule,
        RegisteredModuleInstantiation,
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
        const ALL: [Self; 14] = [
            Self::DependentOnUnsupportedScriptState,
            Self::GlobalGet,
            Self::NamedModuleInvocation,
            Self::NonCurrentRegisterTarget,
            Self::NoCurrentModule,
            Self::RegisteredModuleInstantiation,
            Self::Suspension,
            Self::Threads,
            Self::UnsupportedExpectedAlternative,
            Self::UnsupportedGcInstruction,
            Self::UnsupportedModuleInstances,
            Self::UnsupportedSimdInstruction,
            Self::UnsupportedSimdValue,
            Self::UnsupportedTextFormat,
        ];

        const fn description(self) -> &'static str {
            match self {
                Self::DependentOnUnsupportedScriptState => {
                    "depends on earlier unsupported script state"
                }
                Self::GlobalGet => "global get assertions are not supported by the runner",
                Self::NamedModuleInvocation => {
                    "invocation of a non-current named module is not supported by the runner"
                }
                Self::NonCurrentRegisterTarget => {
                    "registering a non-current module is not supported by the runner"
                }
                Self::NoCurrentModule => "directive has no current module",
                Self::RegisteredModuleInstantiation => {
                    "module trap assertions with registered imports are not supported by the runner"
                }
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

    #[derive(Default)]
    struct GenerationReport {
        runnable_assertions: usize,
        skipped_assertions: BTreeMap<SkipReason, usize>,
        skipped_directives: Vec<(String, SkipReason, bool)>,
        current_location: Option<String>,
        current_is_assertion: bool,
    }

    impl GenerationReport {
        const fn schedule_assertion(&mut self) {
            self.runnable_assertions += 1;
        }

        fn set_location(&mut self, location: String, is_assertion: bool) {
            self.current_location = Some(location);
            self.current_is_assertion = is_assertion;
        }

        fn skip(&mut self, reason: SkipReason) {
            let location = self
                .current_location
                .clone()
                .expect("skipped directives must have a source location");
            self.skip_at(reason, location, self.current_is_assertion);
        }

        fn skip_at(&mut self, reason: SkipReason, location: String, is_assertion: bool) {
            if is_assertion {
                *self.skipped_assertions.entry(reason).or_default() += 1;
            }

            self.skipped_directives
                .push((location, reason, is_assertion));
        }

        fn ignore_scheduled(&mut self, reason: SkipReason, steps: &[GeneratedStep]) {
            let count = steps.iter().filter(|step| step.is_assertion).count();
            self.runnable_assertions = self
                .runnable_assertions
                .checked_sub(count)
                .expect("ignored assertions must already be scheduled");

            for step in steps {
                self.skip_at(reason, step.location.clone(), step.is_assertion);
            }
        }
    }

    struct GeneratedStep {
        code: String,
        is_assertion: bool,
        location: String,
    }

    impl GeneratedStep {
        fn assertion(code: String, location: String) -> Self {
            Self {
                code,
                is_assertion: true,
                location,
            }
        }

        fn directive(code: String, location: String) -> Self {
            Self {
                code,
                is_assertion: false,
                location,
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

        let mut all_tests = String::new();
        let mut test_calls = String::new();
        let mut report = GenerationReport::default();

        let mut entries = fs::read_dir(spec_dir)
            .unwrap()
            .map(|entry| {
                entry.unwrap_or_else(|error| panic!("failed to read a core spec entry: {error}"))
            })
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "wast"))
            .collect::<Vec<_>>();

        assert!(!entries.is_empty(), "no core spec fixtures were found");

        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let file_stem = path.file_stem().unwrap().to_str().unwrap();
            let safe_name = file_stem.replace('-', "_");
            let unsupported_file_reason = unsupported_feature(&format!("{safe_name}_"));

            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let mut lexer = Lexer::new(&contents);
            lexer.allow_confusing_unicode(true);
            let buf = ParseBuffer::new_with_lexer(lexer)
                .unwrap_or_else(|error| panic!("failed to lex {}: {error}", path.display()));
            let wast = wast::parser::parse::<Wast>(&buf)
                .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));

            let mut module_idx: i32 = -1;
            let mut modules = Vec::new();
            let mut registered: Vec<(String, i32)> = Vec::new();
            let mut current_module_name = None;
            let mut unsupported_script_state = None;
            let mut malformed_idx: u32 = 0;
            let mut invalid_idx: u32 = 0;
            let mut unlinkable_idx: u32 = 0;
            let mut trap_module_idx: u32 = 0;

            for directive in wast.directives {
                let (line, column) = directive.span().linecol_in(&contents);
                let directive_location = format!(
                    "{}:{}:{}",
                    path.display(),
                    line.saturating_add(1),
                    column.saturating_add(1)
                );
                let is_assertion = is_assertion_directive(&directive);
                report.set_location(directive_location.clone(), is_assertion);

                if let Some(reason) = unsupported_file_reason {
                    report.skip(reason);
                    continue;
                }

                if let Some(reason) = unsupported_script_state {
                    report.skip(reason);
                    continue;
                }

                match directive {
                    WastDirective::Module(mut wat) => {
                        module_idx += 1;
                        current_module_name = quote_wat_id(&wat);

                        let bytes = wat.encode().unwrap_or_else(|error| {
                            panic!("failed to encode {}: {error}", path.display())
                        });
                        let wasm_path = wasm_dir.join(format!("{}_{}.wasm", safe_name, module_idx));
                        fs::write(&wasm_path, bytes).unwrap();

                        modules.push((
                            module_idx,
                            vec![GeneratedStep::directive(
                                String::new(),
                                directive_location.clone(),
                            )],
                        ));
                    }

                    WastDirective::ModuleDefinition(_) | WastDirective::ModuleInstance { .. } => {
                        let reason = SkipReason::UnsupportedModuleInstances;
                        report.skip(reason);
                        unsupported_script_state =
                            Some(SkipReason::DependentOnUnsupportedScriptState);
                    }

                    WastDirective::Register { name, module, .. } => {
                        if module_idx < 0 {
                            report.skip(SkipReason::NoCurrentModule);
                            continue;
                        }

                        if module
                            .is_some_and(|id| current_module_name.as_deref() != Some(id.name()))
                        {
                            let reason = SkipReason::NonCurrentRegisterTarget;
                            report.skip(reason);
                            unsupported_script_state =
                                Some(SkipReason::DependentOnUnsupportedScriptState);
                            continue;
                        }

                        registered.push((name.to_string(), module_idx));
                    }

                    WastDirective::AssertReturn { exec, results, .. } => {
                        if module_idx < 0 {
                            report.skip(SkipReason::NoCurrentModule);
                            continue;
                        }

                        let invoke = match &exec {
                            WastExecute::Invoke(invoke) => invoke,
                            WastExecute::Get { .. } => {
                                report.skip(SkipReason::GlobalGet);
                                continue;
                            }
                            WastExecute::Wat(_) => {
                                panic!("assert_return with a module execution is unclassified")
                            }
                        };

                        if !targets_current_module(invoke.module.as_ref(), &current_module_name) {
                            let reason = SkipReason::NamedModuleInvocation;
                            report.skip(reason);
                            unsupported_script_state =
                                Some(SkipReason::DependentOnUnsupportedScriptState);
                            continue;
                        }

                        let args_code = match render_args(&invoke.args) {
                            Ok(code) => code,
                            Err(reason) => {
                                report.skip(reason);
                                continue;
                            }
                        };
                        let expected_code = match render_expected(&results) {
                            Ok(code) => code,
                            Err(reason) => {
                                report.skip(reason);
                                continue;
                            }
                        };

                        let steps = &mut modules.last_mut().unwrap().1;
                        let step_idx = steps.len();
                        steps.push(GeneratedStep::assertion(format!(
                            "        spec_step_assert_return(&mut store, _instance, {:?}, &[{}], &[{}], {}, _case);",
                            invoke.name, args_code, expected_code, step_idx
                        ), directive_location.clone()));
                        report.schedule_assertion();
                    }

                    WastDirective::AssertTrap { exec, .. } => match exec {
                        WastExecute::Invoke(ref invoke) => {
                            if module_idx < 0 {
                                report.skip(SkipReason::NoCurrentModule);
                                continue;
                            }

                            if !targets_current_module(invoke.module.as_ref(), &current_module_name)
                            {
                                let reason = SkipReason::NamedModuleInvocation;
                                report.skip(reason);
                                unsupported_script_state =
                                    Some(SkipReason::DependentOnUnsupportedScriptState);
                                continue;
                            }

                            let args_code = match render_args(&invoke.args) {
                                Ok(code) => code,
                                Err(reason) => {
                                    report.skip(reason);
                                    continue;
                                }
                            };

                            let steps = &mut modules.last_mut().unwrap().1;
                            let step_idx = steps.len();
                            steps.push(GeneratedStep::assertion(format!(
                                    "        spec_step_assert_trap(&mut store, _instance, {:?}, &[{}], {}, _case);",
                                    invoke.name, args_code, step_idx
                                ), directive_location.clone()));
                            report.schedule_assertion();
                        }
                        WastExecute::Wat(mut wat) => {
                            if !registered.is_empty() {
                                report.skip(SkipReason::RegisteredModuleInstantiation);
                                continue;
                            }

                            let bytes = wat.encode().unwrap_or_else(|error| {
                                panic!("failed to encode {}: {error}", path.display())
                            });
                            let wasm_path = wasm_dir.join(format!(
                                "trap_module_{}_{}.wasm",
                                safe_name, trap_module_idx
                            ));
                            fs::write(&wasm_path, bytes).unwrap();
                            let test_name =
                                format!("trap_module_{}_{}", safe_name, trap_module_idx);
                            all_tests.push_str(&format!(
                                    concat!(
                                        "fn {test_name}(report: &mut CoreTestReport) {{\n",
                                        "    report.run_case(\"{test_name}\", |case| {{\n",
                                        "        let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/trap_module_{file}_{idx}.wasm\"));\n",
                                        "        let module = Module::try_new(wasm_bytes).unwrap();\n",
                                        "        let mut store = Store::new();\n",
                                        "        let imports = try_resolve_imports_with_registered(&mut store, &module, &[]).unwrap();\n",
                                        "        let result = store.instantiate(&module, imports);\n",
                                        "        case.check(matches!(result, Err(gabagool::Error::Trap(_))), \"expected module instantiation to trap, but it did not\");\n",
                                        "    }});\n",
                                        "}}\n",
                                    ),
                                    test_name = test_name,
                                    file = safe_name,
                                    idx = trap_module_idx,
                                ));
                            test_calls.push_str(&format!("    {test_name}(&mut report);\n"));
                            trap_module_idx += 1;
                            report.schedule_assertion();
                        }
                        WastExecute::Get { .. } => {
                            report.skip(SkipReason::GlobalGet);
                        }
                    },

                    WastDirective::AssertExhaustion {
                        call: ref invoke, ..
                    } => {
                        if module_idx < 0 {
                            report.skip(SkipReason::NoCurrentModule);
                            continue;
                        }

                        if !targets_current_module(invoke.module.as_ref(), &current_module_name) {
                            let reason = SkipReason::NamedModuleInvocation;
                            report.skip(reason);
                            unsupported_script_state =
                                Some(SkipReason::DependentOnUnsupportedScriptState);
                            continue;
                        }

                        let args_code = match render_args(&invoke.args) {
                            Ok(code) => code,
                            Err(reason) => {
                                report.skip(reason);
                                continue;
                            }
                        };

                        let steps = &mut modules.last_mut().unwrap().1;
                        let step_idx = steps.len();
                        steps.push(GeneratedStep::assertion(format!(
                            "        spec_step_assert_exhaustion(&mut store, _instance, {:?}, &[{}], {}, _case);",
                            invoke.name, args_code, step_idx
                        ), directive_location.clone()));
                        report.schedule_assertion();
                    }

                    WastDirective::Invoke(ref invoke) => {
                        if module_idx < 0 {
                            report.skip(SkipReason::NoCurrentModule);
                            continue;
                        }

                        if !targets_current_module(invoke.module.as_ref(), &current_module_name) {
                            let reason = SkipReason::NamedModuleInvocation;
                            report.skip(reason);
                            unsupported_script_state =
                                Some(SkipReason::DependentOnUnsupportedScriptState);
                            continue;
                        }

                        let args_code = match render_args(&invoke.args) {
                            Ok(code) => code,
                            Err(reason) => {
                                report.skip(reason);
                                continue;
                            }
                        };

                        let steps = &mut modules.last_mut().unwrap().1;
                        steps.push(GeneratedStep::directive(
                            format!(
                                "        spec_step_invoke(&mut store, _instance, {:?}, &[{}]);",
                                invoke.name, args_code
                            ),
                            directive_location.clone(),
                        ));
                    }

                    WastDirective::AssertMalformed { mut module, .. } => {
                        if matches!(&module, QuoteWat::QuoteModule(..)) {
                            report.skip(SkipReason::UnsupportedTextFormat);
                            malformed_idx += 1;
                            continue;
                        }

                        let bytes = module.encode().unwrap_or_else(|error| {
                            panic!("failed to encode {}: {error}", path.display())
                        });
                        let wasm_path = wasm_dir
                            .join(format!("malformed_{}_{}.wasm", safe_name, malformed_idx));
                        fs::write(&wasm_path, bytes).unwrap();
                        let test_name = format!("malformed_{}_{}", safe_name, malformed_idx);
                        all_tests.push_str(&format!(
                            concat!(
                                "fn {test_name}(report: &mut CoreTestReport) {{\n",
                                "    report.run_case(\"{test_name}\", |case| {{\n",
                                "        let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/malformed_{file}_{idx}.wasm\"));\n",
                                "        let result = Parser::new(wasm_bytes).parse();\n",
                                "        case.check(result.is_err(), \"expected malformed module to fail parsing, but it succeeded\");\n",
                                "    }});\n",
                                "}}\n",
                            ),
                            test_name = test_name,
                            file = safe_name,
                            idx = malformed_idx,
                        ));
                        test_calls.push_str(&format!("    {test_name}(&mut report);\n"));
                        malformed_idx += 1;
                        report.schedule_assertion();
                    }

                    WastDirective::AssertInvalid { mut module, .. } => {
                        if matches!(&module, QuoteWat::QuoteModule(..)) {
                            report.skip(SkipReason::UnsupportedTextFormat);
                            invalid_idx += 1;
                            continue;
                        }

                        let test_name = format!("invalid_{}_{}", safe_name, invalid_idx);
                        if test_name.starts_with("invalid_simd_") {
                            report.skip(SkipReason::UnsupportedSimdInstruction);
                            invalid_idx += 1;
                            continue;
                        }

                        let bytes = module.encode().unwrap_or_else(|error| {
                            panic!("failed to encode {}: {error}", path.display())
                        });
                        let wasm_path =
                            wasm_dir.join(format!("invalid_{}_{}.wasm", safe_name, invalid_idx));
                        fs::write(&wasm_path, bytes).unwrap();
                        all_tests.push_str(&format!(
                            concat!(
                                "fn {test_name}(report: &mut CoreTestReport) {{\n",
                                "    report.run_case(\"{test_name}\", |case| {{\n",
                                "        let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/invalid_{file}_{idx}.wasm\"));\n",
                                "        let result = Module::try_new(wasm_bytes);\n",
                                "        case.check(result.is_err(), \"expected invalid module to fail validation, but it succeeded\");\n",
                                "    }});\n",
                                "}}\n",
                            ),
                            test_name = test_name,
                            file = safe_name,
                            idx = invalid_idx,
                        ));
                        test_calls.push_str(&format!("    {test_name}(&mut report);\n"));
                        invalid_idx += 1;
                        report.schedule_assertion();
                    }

                    WastDirective::AssertUnlinkable { mut module, .. } => {
                        let bytes = module.encode().unwrap_or_else(|error| {
                            panic!("failed to encode {}: {error}", path.display())
                        });
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
                                    "    let prereq_module_{pidx} = Module::try_new(prereq_wasm_{pidx}).unwrap();\n",
                                    "    let prereq_imports_{pidx} = try_resolve_imports_with_registered(&mut store, &prereq_module_{pidx}, &[]).unwrap();\n",
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
                                    "({:?}, &prereq_exports_{}), ",
                                    name, dep_idx
                                ));
                            }
                            setup.push_str("];\n");
                            setup.push_str("    let resolve_result = try_resolve_imports_with_registered(&mut store, &module, &registered_exports);\n");
                        } else {
                            setup.push_str("    let resolve_result = try_resolve_imports_with_registered(&mut store, &module, &[]);\n");
                        }

                        all_tests.push_str(&format!(
                            concat!(
                                "fn {test_name}(report: &mut CoreTestReport) {{\n",
                                "    report.run_case(\"{test_name}\", |case| {{\n",
                                "        let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/unlinkable_{file}_{idx}.wasm\"));\n",
                                "        let module = Module::try_new(wasm_bytes).unwrap();\n",
                                "        let mut store = Store::new();\n",
                                "{setup}",
                                "        let result = resolve_result.and_then(|imports| store.instantiate(&module, imports));\n",
                                "        case.check(matches!(result, Err(gabagool::Error::Instantiation(_))), \"expected an unlinkable-module error, but did not get one\");\n",
                                "    }});\n",
                                "}}\n",
                            ),
                            test_name = test_name,
                            file = safe_name,
                            idx = unlinkable_idx,
                            setup = setup,
                        ));
                        test_calls.push_str(&format!("    {test_name}(&mut report);\n"));
                        unlinkable_idx += 1;
                        report.schedule_assertion();
                    }

                    WastDirective::AssertException { exec, .. } => match exec {
                        WastExecute::Invoke(ref invoke) => {
                            if module_idx < 0 {
                                report.skip(SkipReason::NoCurrentModule);
                                continue;
                            }

                            if !targets_current_module(invoke.module.as_ref(), &current_module_name)
                            {
                                let reason = SkipReason::NamedModuleInvocation;
                                report.skip(reason);
                                unsupported_script_state =
                                    Some(SkipReason::DependentOnUnsupportedScriptState);
                                continue;
                            }

                            let args_code = match render_args(&invoke.args) {
                                Ok(code) => code,
                                Err(reason) => {
                                    report.skip(reason);
                                    continue;
                                }
                            };

                            let steps = &mut modules.last_mut().unwrap().1;
                            let step_idx = steps.len();
                            steps.push(GeneratedStep::assertion(format!(
                                "        spec_step_assert_exception(&mut store, _instance, {:?}, &[{}], {}, _case);",
                                invoke.name, args_code, step_idx
                            ), directive_location.clone()));
                            report.schedule_assertion();
                        }
                        WastExecute::Get { .. } => {
                            report.skip(SkipReason::GlobalGet);
                        }
                        WastExecute::Wat(_) => {
                            panic!("assert_exception with a module execution is unclassified")
                        }
                    },

                    WastDirective::AssertSuspension { .. } => {
                        report.skip(SkipReason::Suspension);
                    }

                    WastDirective::Thread(_) | WastDirective::Wait { .. } => {
                        report.skip(SkipReason::Threads);
                    }
                }
            }

            // build a map from each module to the registrations that precede it
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
                let test_name = format!("{}_{}", safe_name, midx);
                if let Some(reason) = unsupported_feature(&test_name) {
                    report.ignore_scheduled(reason, steps);
                    continue;
                }

                let steps_code = steps
                    .iter()
                    .map(|step| step.code.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");

                // generate prerequisite setup code for registered modules
                let deps = registered_before.get(midx);
                let has_deps = deps.is_some_and(|d| !d.is_empty());

                let setup_code = if has_deps {
                    let deps = deps.unwrap();
                    // collect unique prerequisite module indices in order
                    let mut prereq_indices: Vec<i32> = Vec::new();
                    for (_, dep_idx) in deps {
                        if !prereq_indices.contains(dep_idx) {
                            prereq_indices.push(*dep_idx);
                        }
                    }
                    prereq_indices.sort();

                    let mut setup = String::new();
                    // instantiate each prerequisite module
                    for pidx in &prereq_indices {
                        setup.push_str(&format!(
                            concat!(
                                "    let prereq_wasm_{pidx}: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{file}_{pidx}.wasm\"));\n",
                                "    let prereq_module_{pidx} = Module::try_new(prereq_wasm_{pidx}).unwrap();\n",
                                "    let prereq_imports_{pidx} = try_resolve_imports_with_registered(&mut store, &prereq_module_{pidx}, &[]).unwrap();\n",
                                "    let prereq_instance_{pidx} = store.instantiate(&prereq_module_{pidx}, prereq_imports_{pidx}).unwrap();\n",
                                "    let prereq_exports_{pidx}: Vec<ExportInstance> = store.exports(prereq_instance_{pidx}).to_vec();\n",
                            ),
                            pidx = pidx,
                            file = safe_name,
                        ));
                    }

                    // build the registered exports
                    setup.push_str(
                        "    let registered_exports: Vec<(&str, &[ExportInstance])> = vec![",
                    );
                    for (name, dep_idx) in deps {
                        setup.push_str(&format!("({:?}, &prereq_exports_{}), ", name, dep_idx));
                    }
                    setup.push_str("];\n");

                    // resolve imports using registered modules
                    setup.push_str("    let imports = try_resolve_imports_with_registered(&mut store, &module, &registered_exports).unwrap();\n");
                    setup
                } else {
                    "    let imports = try_resolve_imports_with_registered(&mut store, &module, &[]).unwrap();\n".to_string()
                };

                all_tests.push_str(&format!(
                    concat!(
                        "fn {test_name}(report: &mut CoreTestReport) {{\n",
                        "    report.run_case(\"{test_name}\", |_case| {{\n",
                        "        let wasm_bytes: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/wasm/{file}_{midx}.wasm\"));\n",
                        "        let module = Module::try_new(wasm_bytes).unwrap();\n",
                        "        let mut store = Store::new();\n",
                        "{setup}",
                        "        let _instance = store.instantiate(&module, imports).unwrap();\n",
                        "{steps}\n",
                        "    }});\n",
                        "}}\n",
                    ),
                    test_name = test_name,
                    file = safe_name,
                    midx = midx,
                    setup = setup_code,
                    steps = steps_code,
                ));
                test_calls.push_str(&format!("    {test_name}(&mut report);\n"));
            }
        }

        let skipped = report.skipped_assertions.values().sum::<usize>();
        let skip_reasons = SkipReason::ALL
            .iter()
            .filter_map(|reason| {
                let assertions = report
                    .skipped_assertions
                    .get(reason)
                    .copied()
                    .unwrap_or_default();
                let directives = report
                    .skipped_directives
                    .iter()
                    .filter(|(_, skipped_reason, _)| skipped_reason == reason)
                    .count();
                if assertions == 0
                    && directives == 0
                    && !matches!(
                        reason,
                        SkipReason::UnsupportedGcInstruction
                            | SkipReason::UnsupportedSimdInstruction
                            | SkipReason::Threads
                            | SkipReason::Suspension
                    )
                {
                    return None;
                }

                Some(format!(
                    "({:?}, {assertions}, {directives})",
                    reason.description()
                ))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let skip_manifest = report
            .skipped_directives
            .iter()
            .map(|(location, reason, is_assertion)| {
                let kind = if *is_assertion {
                    "assertion"
                } else {
                    "directive"
                };
                format!("{location}: {kind}: {}", reason.description())
            })
            .collect::<Vec<_>>()
            .join("\n");
        let skip_manifest_path = Path::new(&out_dir).join("core_test_skips.txt");
        fs::write(&skip_manifest_path, skip_manifest).unwrap();
        all_tests.push_str(&format!(
            concat!(
                "fn main() {{\n",
                "    std::panic::set_hook(Box::new(|_| {{}}));\n",
                "    let mut report = CoreTestReport::new({skipped}, vec![{skip_reasons}]);\n",
                "{test_calls}",
                "    report.finish();\n",
                "}}\n",
            ),
            skipped = skipped,
            skip_reasons = skip_reasons,
            test_calls = test_calls,
        ));

        fs::write(
            Path::new(&out_dir).join("core_tests_generated.rs"),
            all_tests,
        )
        .unwrap();
    }

    fn quote_wat_id(wat: &QuoteWat<'_>) -> Option<String> {
        match wat {
            QuoteWat::Wat(Wat::Module(module)) => module.id.as_ref().map(|id| id.name().to_owned()),
            QuoteWat::Wat(Wat::Component(_))
            | QuoteWat::QuoteModule(..)
            | QuoteWat::QuoteComponent(..) => None,
        }
    }

    fn targets_current_module(
        target: Option<&wast::token::Id<'_>>,
        current: &Option<String>,
    ) -> bool {
        target.is_none_or(|id| current.as_deref() == Some(id.name()))
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
        const SIMD_PREFIXES: &[&str] = &[
            "simd_address_",
            "simd_bit_shift_",
            "simd_bitwise_",
            "simd_f32x4_cmp_",
            "simd_f64x2_cmp_",
            "simd_i16x8_cmp_",
            "simd_i32x4_cmp_",
            "simd_i8x16_cmp_",
            "simd_load_",
            "simd_load_extend_",
            "simd_load_splat_",
            "simd_load_zero_",
            "simd_splat_",
            "simd_store_",
        ];

        if GC_PREFIXES
            .iter()
            .any(|prefix| test_name.starts_with(prefix))
            || matches!(
                test_name,
                "type_subtyping_14"
                    | "type_subtyping_15"
                    | "type_subtyping_17"
                    | "type_subtyping_18"
                    | "type_subtyping_19"
                    | "type_subtyping_20"
                    | "type_subtyping_21"
                    | "type_subtyping_22"
                    | "type_subtyping_23"
                    | "type_subtyping_24"
                    | "type_subtyping_25"
            )
        {
            return Some(SkipReason::UnsupportedGcInstruction);
        }

        if SIMD_PREFIXES
            .iter()
            .any(|prefix| test_name.starts_with(prefix))
        {
            return Some(SkipReason::UnsupportedSimdInstruction);
        }

        None
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

    fn render_args(args: &[WastArg]) -> Result<String, SkipReason> {
        let rendered = args
            .iter()
            .map(|arg| match arg {
                WastArg::Core(WastArgCore::I32(v)) => {
                    Ok(format!("RawValue::from({})", render_i32(*v)))
                }
                WastArg::Core(WastArgCore::I64(v)) => {
                    Ok(format!("RawValue::from({})", render_i64(*v)))
                }
                WastArg::Core(WastArgCore::F32(v)) => {
                    Ok(format!("RawValue::from(f32::from_bits({}))", v.bits))
                }
                WastArg::Core(WastArgCore::F64(v)) => {
                    Ok(format!("RawValue::from(f64::from_bits({}))", v.bits))
                }
                WastArg::Core(WastArgCore::RefNull(_)) => {
                    Ok("RawValue::from_ref(Ref::Null)".to_string())
                }
                WastArg::Core(WastArgCore::RefExtern(n)) => Ok(format!(
                    "RawValue::from_ref(Ref::RefExtern(usize::try_from({}u32).unwrap()))",
                    n
                )),
                WastArg::Core(WastArgCore::RefHost(n)) => Ok(format!(
                    "RawValue::from_ref(Ref::RefExtern(usize::try_from({}u32).unwrap()))",
                    n
                )),
                WastArg::Core(WastArgCore::V128(_)) => Err(SkipReason::UnsupportedSimdValue),
                _ => panic!("unclassified core spec argument: {arg:?}"),
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rendered.join(", "))
    }

    fn render_expected(results: &[WastRet]) -> Result<String, SkipReason> {
        let rendered = results
            .iter()
            .map(|ret| match ret {
                WastRet::Core(WastRetCore::I32(v)) => {
                    Ok(format!("ExpectedValue::I32({})", render_i32(*v)))
                }
                WastRet::Core(WastRetCore::I64(v)) => {
                    Ok(format!("ExpectedValue::I64({})", render_i64(*v)))
                }
                WastRet::Core(WastRetCore::F32(np)) => Ok(match np {
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
                WastRet::Core(WastRetCore::F64(np)) => Ok(match np {
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
                    Ok("ExpectedValue::Ref(ExpectedRef::Null)".to_string())
                }
                WastRet::Core(WastRetCore::RefExtern(Some(n))) => Ok(format!(
                    "ExpectedValue::Ref(ExpectedRef::Extern(Some({})))",
                    n
                )),
                WastRet::Core(WastRetCore::RefExtern(None)) => {
                    Ok("ExpectedValue::Ref(ExpectedRef::Extern(None))".to_string())
                }
                WastRet::Core(WastRetCore::RefHost(n)) => Ok(format!(
                    "ExpectedValue::Ref(ExpectedRef::Extern(Some({})))",
                    n
                )),
                WastRet::Core(WastRetCore::RefFunc(_)) => {
                    Ok("ExpectedValue::Ref(ExpectedRef::Func)".to_string())
                }
                WastRet::Core(
                    WastRetCore::RefAny
                    | WastRetCore::RefEq
                    | WastRetCore::RefStruct
                    | WastRetCore::RefArray,
                ) => Err(SkipReason::UnsupportedGcInstruction),
                WastRet::Core(WastRetCore::RefI31 | WastRetCore::RefI31Shared) => {
                    Err(SkipReason::UnsupportedGcInstruction)
                }
                WastRet::Core(WastRetCore::V128(_)) => Err(SkipReason::UnsupportedSimdValue),
                WastRet::Core(WastRetCore::Either(_)) => {
                    Err(SkipReason::UnsupportedExpectedAlternative)
                }
                _ => panic!("unclassified core spec expected value: {ret:?}"),
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rendered.join(", "))
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
