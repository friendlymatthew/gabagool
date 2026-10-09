#![cfg(feature = "core-tests")]
use std::collections::HashMap;

use gabagool::{
    parser::Parser, AddrType, ExternalValue, FunctionInstance, FunctionType, GlobalInstance,
    GlobalType, GuestMemory, Instance, Limit, MemoryInstance, MemoryType, Module, Mutability,
    RawValue, Ref, Store, ValueType,
};

const DEPENDENT_SKIP: &str = "depends on earlier unsupported script state";

struct ScriptContext {
    store: Store,
    current: Option<Instance>,
    named: HashMap<String, Instance>,
    registered: HashMap<String, Instance>,
    spectest: HashMap<String, ExternalValue>,
}

impl ScriptContext {
    fn new() -> Self {
        Self {
            store: Store::new(),
            current: None,
            named: HashMap::new(),
            registered: HashMap::new(),
            spectest: HashMap::new(),
        }
    }

    fn instantiate_module(&mut self, bytes: &[u8], id: Option<&str>) -> Result<(), String> {
        let instance = self.instantiate(bytes)?;
        self.current = Some(instance);

        if let Some(id) = id {
            self.named.insert(id.to_string(), instance);
        }

        Ok(())
    }

    fn instantiate(&mut self, bytes: &[u8]) -> Result<Instance, String> {
        let module = Module::try_new(bytes).map_err(|error| error.to_string())?;
        let imports = self
            .resolve_imports(&module)
            .map_err(|error| error.to_string())?;

        self.store
            .instantiate(&module, imports)
            .map_err(|error| error.to_string())
    }

    fn register(&mut self, name: &str, target: Option<&str>) -> Result<(), String> {
        let instance = self.resolve_target(target)?;
        self.registered.insert(name.to_string(), instance);

        Ok(())
    }

    fn resolve_target(&self, target: Option<&str>) -> Result<Instance, String> {
        match target {
            Some(name) => self
                .named
                .get(name)
                .copied()
                .ok_or_else(|| format!("unknown module ${name}")),
            None => self
                .current
                .ok_or_else(|| "directive has no current module".to_string()),
        }
    }

    fn resolve_imports(&mut self, module: &Module) -> Result<Vec<ExternalValue>, gabagool::Error> {
        let mut imports = Vec::with_capacity(module.import_declarations().len());

        for import in module.import_declarations() {
            if let Some(instance) = self.registered.get(&import.module).copied() {
                let export = self
                    .store
                    .exports(instance)
                    .iter()
                    .find(|export| export.name == import.name)
                    .ok_or_else(|| {
                        gabagool::Error::Instantiation(format!(
                            "unknown import {}.{}",
                            import.module, import.name
                        ))
                    })?;

                imports.push(export.value.clone());
                continue;
            }

            if import.module == "spectest" {
                let value = match self.spectest.get(&import.name) {
                    Some(value) => value.clone(),
                    None => {
                        let value = resolve_spectest_export(&mut self.store, &import.name)?;
                        self.spectest.insert(import.name.clone(), value.clone());
                        value
                    }
                };
                imports.push(value);
                continue;
            }

            return Err(gabagool::Error::Instantiation(format!(
                "unknown module {}",
                import.module
            )));
        }

        Ok(imports)
    }
}

enum AssertionStatus {
    Passed,
    Failed(String),
    Skipped(&'static str),
}

struct AssertionRecord {
    location: &'static str,
    status: AssertionStatus,
}

struct ScriptRunner {
    name: &'static str,
    context: ScriptContext,
    assertion_filter: Option<String>,
    assertion_reached: bool,
    assertions: Vec<AssertionRecord>,
    directive_skips: Vec<(&'static str, &'static str)>,
    runner_errors: Vec<String>,
    blocked: Option<&'static str>,
}

impl ScriptRunner {
    fn new(name: &'static str, assertion_filter: Option<String>) -> Self {
        Self {
            name,
            context: ScriptContext::new(),
            assertion_filter,
            assertion_reached: false,
            assertions: Vec::new(),
            directive_skips: Vec::new(),
            runner_errors: Vec::new(),
            blocked: None,
        }
    }

    fn directive(
        &mut self,
        location: &'static str,
        run: impl FnOnce(&mut ScriptContext) -> Result<(), String>,
    ) {
        if self.assertion_reached {
            return;
        }

        if self.blocked.is_some() {
            if self.assertion_filter.is_none() {
                self.directive_skips.push((location, DEPENDENT_SKIP));
            }
            return;
        }

        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&mut self.context))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => self.runner_error(location, error),
            Err(payload) => self.runner_error(location, panic_message(payload)),
        }
    }

    fn assertion(
        &mut self,
        location: &'static str,
        run: impl FnOnce(&mut ScriptContext) -> Result<(), String>,
    ) {
        if self.assertion_reached {
            return;
        }

        let selected = self
            .assertion_filter
            .as_deref()
            .is_none_or(|filter| filter == location);

        if self.blocked.is_some() {
            if self.assertion_filter.is_none() || selected {
                self.assertions.push(AssertionRecord {
                    location,
                    status: AssertionStatus::Skipped(DEPENDENT_SKIP),
                });
            }

            if self.assertion_filter.is_some() && selected {
                self.assertion_reached = true;
            }
            return;
        }

        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&mut self.context)));
        match result {
            Ok(Ok(())) if selected => self.assertions.push(AssertionRecord {
                location,
                status: AssertionStatus::Passed,
            }),
            Ok(Err(error)) if selected => self.assertions.push(AssertionRecord {
                location,
                status: AssertionStatus::Failed(error),
            }),
            Ok(_) => {}
            Err(payload) => {
                if selected {
                    self.assertions.push(AssertionRecord {
                        location,
                        status: AssertionStatus::Skipped(DEPENDENT_SKIP),
                    });
                }
                self.runner_error(location, panic_message(payload));
            }
        }

        if self.assertion_filter.is_some() && selected {
            self.assertion_reached = true;
        }
    }

    fn skip(
        &mut self,
        location: &'static str,
        is_assertion: bool,
        reason: &'static str,
        blocks_script: bool,
    ) {
        if self.assertion_reached {
            return;
        }

        if let Some(filter) = &self.assertion_filter {
            if is_assertion && filter == location {
                self.assertions.push(AssertionRecord {
                    location,
                    status: AssertionStatus::Skipped(reason),
                });
                self.assertion_reached = true;
            }

            if blocks_script && self.blocked.is_none() {
                self.blocked = Some(reason);
            }
            return;
        }

        if is_assertion {
            self.assertions.push(AssertionRecord {
                location,
                status: AssertionStatus::Skipped(reason),
            });
        } else {
            self.directive_skips.push((location, reason));
        }

        if blocks_script && self.blocked.is_none() {
            self.blocked = Some(reason);
        }
    }

    fn runner_error(&mut self, location: &'static str, error: String) {
        self.runner_errors.push(format!("{location}: {error}"));
        self.blocked = Some(DEPENDENT_SKIP);
    }

    fn finish(mut self) {
        if self.assertion_filter.is_some() && !self.assertion_reached {
            let filter = self.assertion_filter.as_deref().unwrap();
            self.runner_errors
                .push(format!("assertion filter did not match {filter}"));
        }

        let passed = self
            .assertions
            .iter()
            .filter(|record| matches!(record.status, AssertionStatus::Passed))
            .count();
        let failed = self
            .assertions
            .iter()
            .filter(|record| matches!(record.status, AssertionStatus::Failed(_)))
            .count();
        let skipped = self.assertions.len() - passed - failed;

        let mut contents = String::new();
        for record in &self.assertions {
            match &record.status {
                AssertionStatus::Passed => {
                    contents.push_str(&format!("assertion\tpassed\t{}\n", record.location));
                }
                AssertionStatus::Failed(error) => contents.push_str(&format!(
                    "assertion\tfailed\t{}\t{}\n",
                    record.location,
                    sanitize(error)
                )),
                AssertionStatus::Skipped(reason) => contents.push_str(&format!(
                    "assertion\tskipped\t{}\t{}\n",
                    record.location, reason
                )),
            }
        }
        for (location, reason) in &self.directive_skips {
            contents.push_str(&format!("directive\tskipped\t{location}\t{reason}\n"));
        }
        for error in &self.runner_errors {
            contents.push_str(&format!("runner_error\t{}\n", sanitize(error)));
        }

        let result_dir = std::path::Path::new("target/core-test-results");
        std::fs::create_dir_all(result_dir).unwrap();
        std::fs::write(result_dir.join(format!("{}.tsv", self.name)), contents).unwrap();

        println!(
            "{}: {passed} passed, {failed} failed, {skipped} skipped",
            self.name
        );
        for record in self
            .assertions
            .iter()
            .filter(|record| matches!(record.status, AssertionStatus::Failed(_)))
            .take(20)
        {
            if let AssertionStatus::Failed(error) = &record.status {
                println!("  {}: {error}", record.location);
            }
        }
        if failed > 20 {
            println!("  ... and {} more assertion failures", failed - 20);
        }
        for error in &self.runner_errors {
            println!("  runner error: {error}");
        }

        assert!(
            failed == 0 && self.runner_errors.is_empty(),
            "{} had {failed} assertion failures and {} runner errors",
            self.name,
            self.runner_errors.len()
        );
    }
}

fn run_script(name: &'static str, run: impl FnOnce(&mut ScriptRunner)) {
    let assertion_filter = std::env::var("CORE_TEST_ASSERTION").ok();
    if assertion_filter
        .as_deref()
        .is_some_and(|filter| !filter.starts_with(&format!("{name}.wast:")))
    {
        return;
    }

    let mut script = ScriptRunner::new(name, assertion_filter);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&mut script)));

    if let Err(payload) = result {
        script.runner_error("script", panic_message(payload));
    }

    script.finish();
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic")
        .to_string()
}

fn sanitize(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
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

fn spec_invoke(
    context: &mut ScriptContext,
    target: Option<&str>,
    name: &str,
    args: &[RawValue],
) -> Result<Vec<RawValue>, String> {
    let instance = context.resolve_target(target)?;
    invoke_and_resume(&mut context.store, instance, name, args).map_err(|error| error.to_string())
}

fn spec_assert_return(
    context: &mut ScriptContext,
    target: Option<&str>,
    name: &str,
    args: &[RawValue],
    expected: &[ExpectedValue],
) -> Result<(), String> {
    let actual = spec_invoke(context, target, name, args)?;

    if values_match(expected, &actual) {
        Ok(())
    } else {
        Err(format!(
            "assert_return {name:?} expected {expected:?}, got {actual:?}"
        ))
    }
}

fn spec_assert_get(
    context: &mut ScriptContext,
    target: Option<&str>,
    name: &str,
    expected: &[ExpectedValue],
) -> Result<(), String> {
    let instance = context.resolve_target(target)?;
    let export = context
        .store
        .exports(instance)
        .iter()
        .find(|export| export.name == name)
        .ok_or_else(|| format!("unknown export {name:?}"))?;
    let ExternalValue::Global { addr } = export.value else {
        return Err(format!("export {name:?} is not a global"));
    };
    let actual = [context.store.globals[addr].value];

    if values_match(expected, &actual) {
        Ok(())
    } else {
        Err(format!(
            "assert_return get {name:?} expected {expected:?}, got {actual:?}"
        ))
    }
}

fn spec_assert_trap(
    context: &mut ScriptContext,
    target: Option<&str>,
    name: &str,
    args: &[RawValue],
) -> Result<(), String> {
    let instance = context.resolve_target(target)?;

    match invoke_and_resume(&mut context.store, instance, name, args) {
        Err(gabagool::Error::Trap(_)) => Ok(()),
        Ok(results) => Err(format!("expected trap, got {results:?}")),
        Err(error) => Err(format!("expected trap, got {error}")),
    }
}

fn spec_assert_exhaustion(
    context: &mut ScriptContext,
    target: Option<&str>,
    name: &str,
    args: &[RawValue],
) -> Result<(), String> {
    let instance = context.resolve_target(target)?;

    match invoke_and_resume(&mut context.store, instance, name, args) {
        Err(gabagool::Error::Trap(gabagool::Trap::CallStackExhausted)) => Ok(()),
        Ok(results) => Err(format!("expected exhaustion, got {results:?}")),
        Err(error) => Err(format!("expected exhaustion, got {error}")),
    }
}

fn spec_assert_exception(
    context: &mut ScriptContext,
    target: Option<&str>,
    name: &str,
    args: &[RawValue],
) -> Result<(), String> {
    let instance = context.resolve_target(target)?;

    match invoke_and_resume(&mut context.store, instance, name, args) {
        Err(gabagool::Error::Exception(_)) => Ok(()),
        Ok(results) => Err(format!("expected exception, got {results:?}")),
        Err(error) => Err(format!("expected exception, got {error}")),
    }
}

fn spec_assert_module_trap(context: &mut ScriptContext, bytes: &[u8]) -> Result<(), String> {
    let module = Module::try_new(bytes).map_err(|error| error.to_string())?;
    let imports = context
        .resolve_imports(&module)
        .map_err(|error| error.to_string())?;

    match context.store.instantiate(&module, imports) {
        Err(gabagool::Error::Trap(_)) => Ok(()),
        Ok(_) => Err("expected module instantiation to trap".to_string()),
        Err(error) => Err(format!("expected trap, got {error}")),
    }
}

fn spec_assert_unlinkable(context: &mut ScriptContext, bytes: &[u8]) -> Result<(), String> {
    let module =
        Module::try_new(bytes).map_err(|error| format!("expected a valid module, got {error}"))?;
    let result = context
        .resolve_imports(&module)
        .and_then(|imports| context.store.instantiate(&module, imports));

    match result {
        Err(gabagool::Error::Instantiation(_)) => Ok(()),
        Ok(_) => Err("expected module instantiation to be unlinkable".to_string()),
        Err(error) => Err(format!("expected an unlinkable module, got {error}")),
    }
}

fn spec_assert_malformed(bytes: &[u8]) -> Result<(), String> {
    if Parser::new(bytes).parse().is_err() {
        Ok(())
    } else {
        Err("expected malformed module to fail parsing".to_string())
    }
}

fn spec_assert_invalid(bytes: &[u8]) -> Result<(), String> {
    if Module::try_new(bytes).is_err() {
        Ok(())
    } else {
        Err("expected invalid module to fail validation".to_string())
    }
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
            gabagool::ExecutionState::Completed(values) => return Ok(values),
            gabagool::ExecutionState::Suspended { .. } => state = store.resume()?,
            gabagool::ExecutionState::FuelExhausted => {
                return Err(gabagool::Error::Instantiation("fuel exhausted".into()));
            }
        }
    }
}

fn values_match(expected: &[ExpectedValue], actual: &[RawValue]) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual.iter())
            .all(|(expected, actual)| match expected {
                ExpectedValue::I32(value) => *value == actual.as_i32(),
                ExpectedValue::I64(value) => *value == actual.as_i64(),
                ExpectedValue::F32(pattern) => match pattern {
                    NanPat::CanonicalNan => {
                        actual.as_f32().is_nan() && (actual.as_f32().to_bits() & 0x003F_FFFF == 0)
                    }
                    NanPat::ArithmeticNan => actual.as_f32().is_nan(),
                    NanPat::Value(value) => actual.as_f32().to_bits() == *value,
                },
                ExpectedValue::F64(pattern) => match pattern {
                    NanPat::CanonicalNan => {
                        actual.as_f64().is_nan()
                            && (actual.as_f64().to_bits() & 0x0007_FFFF_FFFF_FFFF == 0)
                    }
                    NanPat::ArithmeticNan => actual.as_f64().is_nan(),
                    NanPat::Value(value) => actual.as_f64().to_bits() == *value,
                },
                ExpectedValue::Ref(expected) => match (expected, actual.as_ref()) {
                    (ExpectedRef::Null, Ref::Null) => true,
                    (ExpectedRef::Extern(Some(expected)), Ref::RefExtern(actual)) => {
                        usize::try_from(*expected).unwrap() == actual
                    }
                    (ExpectedRef::Extern(None), Ref::RefExtern(_)) => true,
                    (ExpectedRef::Func, Ref::FunctionAddr(_)) => true,
                    _ => false,
                },
            })
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
        let addr = store.memories.len();
        store.memories.push(MemoryInstance {
            memory_type: MemoryType {
                addr_type: AddrType::I32,
                limit: Limit { min: 1, max: 2 },
            },
            data: GuestMemory::new(65536),
        });

        return Ok(ExternalValue::Memory { addr });
    }

    Err(gabagool::Error::Instantiation(format!(
        "unknown import spectest.{name}"
    )))
}

include!(concat!(env!("OUT_DIR"), "/core_tests_generated.rs"));
