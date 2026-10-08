#![cfg(feature = "core-tests")]

use gabagool::{
    parser::Parser, AddrType, ExportInstance, ExternalValue, FunctionInstance, FunctionType,
    GlobalInstance, GlobalType, GuestMemory, ImportDescription, Instance, Limit, MemoryInstance,
    MemoryType, Module, Mutability, RawValue, Ref, Store, ValueType,
};

#[derive(Default)]
struct CaseReport {
    executed: usize,
    failures: Vec<String>,
}

impl CaseReport {
    fn check(&mut self, passed: bool, failure: impl Into<String>) {
        self.executed += 1;

        if !passed {
            self.failures.push(failure.into());
        }
    }
}

struct CoreTestReport {
    executed: usize,
    failed: usize,
    skipped: usize,
    skip_reasons: Vec<(&'static str, usize, usize)>,
    failures: Vec<String>,
    runner_errors: Vec<String>,
}

impl CoreTestReport {
    fn new(skipped: usize, skip_reasons: Vec<(&'static str, usize, usize)>) -> Self {
        Self {
            executed: 0,
            failed: 0,
            skipped,
            skip_reasons,
            failures: Vec::new(),
            runner_errors: Vec::new(),
        }
    }

    fn run_case(&mut self, name: &'static str, run: impl FnOnce(&mut CaseReport)) {
        let mut case = CaseReport::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&mut case)));
        let case_failed = !case.failures.is_empty();
        let runner_error = result.err().map(|payload| {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic");
            format!("{name}: {message}")
        });

        let status = if runner_error.is_some() {
            "RUNNER ERROR"
        } else if case_failed {
            "FAILED"
        } else {
            "ok"
        };
        println!("test {name} ... {status}");

        self.executed += case.executed;
        self.failed += case.failures.len();
        self.failures.extend(
            case.failures
                .into_iter()
                .map(|failure| format!("{name}: {failure}")),
        );

        if let Some(error) = runner_error {
            self.runner_errors.push(error);
        }
    }

    fn finish(self) {
        println!(
            "core spec assertions: {} executed, {} failed, {} skipped",
            self.executed, self.failed, self.skipped
        );

        for (reason, assertions, directives) in self.skip_reasons {
            println!("  skipped {assertions} assertions across {directives} directives: {reason}");
        }

        let details = self
            .failures
            .iter()
            .map(|failure| format!("failed: {failure}"))
            .chain(
                self.runner_errors
                    .iter()
                    .map(|error| format!("runner error: {error}")),
            )
            .collect::<Vec<_>>()
            .join("\n");
        let details_path = concat!(env!("OUT_DIR"), "/core_test_failures.txt");
        std::fs::write(details_path, details).unwrap();

        if !self.failures.is_empty() || !self.runner_errors.is_empty() {
            eprintln!(
                "core spec details: {} assertion failures and {} runner errors written to {}",
                self.failures.len(),
                self.runner_errors.len(),
                details_path
            );
        }

        if self.failed != 0 || !self.runner_errors.is_empty() {
            std::process::exit(1);
        }
    }
}

#[derive(Debug)]
enum NanPat<T> {
    CanonicalNan,
    ArithmeticNan,
    Value(T),
}

#[derive(Debug)]
enum ExpectedRef {
    Null,
    Extern(Option<u32>),
    Func,
}

#[derive(Debug)]
enum ExpectedValue {
    I32(i32),
    I64(i64),
    F32(NanPat<u32>),
    F64(NanPat<u64>),
    Ref(ExpectedRef),
}

/// create a spectest-style memory: 1 page initial, 2 pages max
/// (matching the standard WebAssembly spectest module)
fn create_spectest_memory(store: &mut Store, mt: &MemoryType) -> ExternalValue {
    let addr = store.memories.len();
    // spectest module always provides memory with 1 page initial, 2 pages max
    store.memories.push(MemoryInstance {
        memory_type: MemoryType {
            addr_type: mt.addr_type,
            limit: Limit { min: 1, max: 2 },
        },
        data: GuestMemory::new(65536),
    });
    ExternalValue::Memory { addr }
}

fn invoke_and_resume(
    store: &mut Store,
    instance: Instance,
    name: &str,
    args: &[RawValue],
) -> Result<Vec<RawValue>, gabagool::Error> {
    let mut state = store.invoke(instance, name, args.to_vec())?;
    loop {
        match state {
            gabagool::ExecutionState::Completed(v) => return Ok(v),
            gabagool::ExecutionState::Suspended { .. } => {
                state = store.resume()?;
            }
            gabagool::ExecutionState::FuelExhausted => {
                return Err(gabagool::Error::Instantiation("fuel exhausted".into()));
            }
        }
    }
}

fn spec_step_assert_return(
    store: &mut Store,
    instance: Instance,
    name: &str,
    args: &[RawValue],
    expected: &[ExpectedValue],
    step: usize,
    report: &mut CaseReport,
) {
    let result = invoke_and_resume(store, instance, name, args);
    match result {
        Ok(actual) => {
            report.check(
                values_match(expected, &actual),
                format!(
                    "step {} assert_return(\"{}\", {:?}): expected {:?}, got {:?}",
                    step, name, args, expected, actual
                ),
            );
        }
        Err(e) => {
            report.check(
                false,
                format!(
                    "step {} assert_return(\"{}\", {:?}): unexpected error: {}",
                    step, name, args, e
                ),
            );
        }
    }
}

fn spec_step_assert_trap(
    store: &mut Store,
    instance: Instance,
    name: &str,
    args: &[RawValue],
    step: usize,
    report: &mut CaseReport,
) {
    match invoke_and_resume(store, instance, name, args) {
        Ok(results) => report.check(
            false,
            format!(
                "step {} assert_trap(\"{}\", {:?}): expected trap, got {:?}",
                step, name, args, results
            ),
        ),
        Err(gabagool::Error::Trap(_)) => report.check(true, ""),
        Err(other) => report.check(
            false,
            format!(
                "step {} assert_trap(\"{}\", {:?}): expected trap, got error: {}",
                step, name, args, other
            ),
        ),
    }
}

fn spec_step_assert_exhaustion(
    store: &mut Store,
    instance: Instance,
    name: &str,
    args: &[RawValue],
    step: usize,
    report: &mut CaseReport,
) {
    match invoke_and_resume(store, instance, name, args) {
        Err(gabagool::Error::Trap(gabagool::Trap::CallStackExhausted)) => report.check(true, ""),
        Ok(results) => report.check(
            false,
            format!(
                "step {} assert_exhaustion(\"{}\", {:?}): expected exhaustion, got {:?}",
                step, name, args, results
            ),
        ),
        Err(other) => report.check(
            false,
            format!(
                "step {} assert_exhaustion(\"{}\", {:?}): expected exhaustion, got error: {}",
                step, name, args, other
            ),
        ),
    }
}

fn spec_step_assert_exception(
    store: &mut Store,
    instance: Instance,
    name: &str,
    args: &[RawValue],
    step: usize,
    report: &mut CaseReport,
) {
    match invoke_and_resume(store, instance, name, args) {
        Ok(results) => {
            report.check(
                false,
                format!(
                    "step {} assert_exception(\"{}\", {:?}): expected exception, got {:?}",
                    step, name, args, results
                ),
            );
        }
        Err(gabagool::Error::Exception(_)) => {
            report.check(true, "");
        }
        Err(other) => {
            report.check(
                false,
                format!(
                    "step {} assert_exception(\"{}\", {:?}): expected exception, got error: {}",
                    step, name, args, other
                ),
            );
        }
    }
}

fn spec_step_invoke(store: &mut Store, instance: Instance, name: &str, args: &[RawValue]) {
    invoke_and_resume(store, instance, name, args)
        .unwrap_or_else(|error| panic!("standalone invoke failed: {error}"));
}

fn values_match(expected: &[ExpectedValue], actual: &[RawValue]) -> bool {
    if expected.len() != actual.len() {
        return false;
    }

    expected
        .iter()
        .zip(actual.iter())
        .all(|(exp, act)| match exp {
            ExpectedValue::I32(e) => *e == act.as_i32(),
            ExpectedValue::I64(e) => *e == act.as_i64(),
            ExpectedValue::F32(pat) => {
                let a = act.as_f32();
                match pat {
                    NanPat::CanonicalNan => a.is_nan() && (a.to_bits() & 0x003F_FFFF == 0),
                    NanPat::ArithmeticNan => a.is_nan(),
                    NanPat::Value(e) => a.to_bits() == *e,
                }
            }
            ExpectedValue::F64(pat) => {
                let a = act.as_f64();
                match pat {
                    NanPat::CanonicalNan => {
                        a.is_nan() && (a.to_bits() & 0x0007_FFFF_FFFF_FFFF == 0)
                    }
                    NanPat::ArithmeticNan => a.is_nan(),
                    NanPat::Value(e) => a.to_bits() == *e,
                }
            }
            ExpectedValue::Ref(exp_ref) => {
                let act_ref = act.as_ref();
                match (exp_ref, act_ref) {
                    (ExpectedRef::Null, Ref::Null) => true,
                    (ExpectedRef::Extern(Some(n)), Ref::RefExtern(m)) => {
                        usize::try_from(*n).unwrap() == m
                    }
                    (ExpectedRef::Extern(None), Ref::RefExtern(_)) => true,
                    (ExpectedRef::Func, Ref::FunctionAddr(_)) => true,
                    _ => false,
                }
            }
        })
}

fn try_resolve_imports_with_registered(
    store: &mut Store,
    module: &Module,
    registered_exports: &[(&str, &[ExportInstance])],
) -> Result<Vec<ExternalValue>, gabagool::Error> {
    module
        .import_declarations()
        .iter()
        .map(|import| {
            for &(reg_name, exports) in registered_exports {
                if import.module == reg_name {
                    for export in exports {
                        if export.name == import.name {
                            let kind_ok = matches!(
                                (&export.value, &import.description),
                                (ExternalValue::Function { .. }, ImportDescription::Func(_))
                                    | (ExternalValue::Table { .. }, ImportDescription::Table(_))
                                    | (ExternalValue::Memory { .. }, ImportDescription::Mem(_))
                                    | (ExternalValue::Global { .. }, ImportDescription::Global(_))
                                    | (ExternalValue::Tag { .. }, ImportDescription::Tag(_))
                            );
                            if kind_ok {
                                return Ok(export.value.clone());
                            } else {
                                return Err(gabagool::Error::Instantiation(format!(
                                    "incompatible import type for {}.{}",
                                    import.module, import.name
                                )));
                            }
                        }
                    }
                    return Err(gabagool::Error::Instantiation(format!(
                        "unknown import {}.{}",
                        import.module, import.name
                    )));
                }
            }
            if import.module == "spectest" {
                return resolve_spectest_export(store, &import.name);
            }

            Err(gabagool::Error::Instantiation(format!(
                "unknown module {}",
                import.module
            )))
        })
        .collect()
}

fn resolve_spectest_export(
    store: &mut Store,
    name: &str,
) -> Result<ExternalValue, gabagool::Error> {
    let function_type = match name {
        "print" => Some(FunctionType {
            params: vec![],
            results: vec![],
        }),
        "print_i32" => Some(FunctionType {
            params: vec![ValueType::I32],
            results: vec![],
        }),
        "print_i64" => Some(FunctionType {
            params: vec![ValueType::I64],
            results: vec![],
        }),
        "print_f32" => Some(FunctionType {
            params: vec![ValueType::F32],
            results: vec![],
        }),
        "print_f64" => Some(FunctionType {
            params: vec![ValueType::F64],
            results: vec![],
        }),
        "print_i32_f32" => Some(FunctionType {
            params: vec![ValueType::I32, ValueType::F32],
            results: vec![],
        }),
        "print_f64_f64" => Some(FunctionType {
            params: vec![ValueType::F64, ValueType::F64],
            results: vec![],
        }),
        _ => None,
    };

    if let Some(function_type) = function_type {
        let addr = store.functions.len();
        store.functions.push(FunctionInstance::Host {
            function_type,
            module_name: "spectest".to_string(),
            function_name: name.to_string(),
        });

        return Ok(ExternalValue::Function { addr });
    }

    let global = match name {
        "global_i32" => Some((ValueType::I32, RawValue::from(666i32))),
        "global_i64" => Some((ValueType::I64, RawValue::from(666i64))),
        "global_f32" => Some((ValueType::F32, RawValue::from(666.6f32))),
        "global_f64" => Some((ValueType::F64, RawValue::from(666.6f64))),
        _ => None,
    };

    if let Some((value_type, value)) = global {
        let addr = store.globals.len();
        store.globals.push(GlobalInstance {
            global_type: GlobalType {
                value_type,
                mutability: Mutability::Const,
            },
            value,
        });

        return Ok(ExternalValue::Global { addr });
    }

    if name == "table" {
        let addr = store.tables.len();
        store.tables.push(gabagool::TableInstance {
            table_type: gabagool::TableType {
                element_reference_type: gabagool::RefType::FuncRef,
                addr_type: AddrType::I32,
                limit: Limit { min: 10, max: 20 },
            },
            elem: vec![Ref::Null; 10],
        });

        return Ok(ExternalValue::Table { addr });
    }

    if name == "memory" {
        return Ok(create_spectest_memory(
            store,
            &MemoryType {
                addr_type: AddrType::I32,
                limit: Limit { min: 1, max: 2 },
            },
        ));
    }

    Err(gabagool::Error::Instantiation(format!(
        "unknown import spectest.{name}"
    )))
}

include!(concat!(env!("OUT_DIR"), "/core_tests_generated.rs"));
