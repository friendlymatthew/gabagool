use std::collections::HashMap;

use crate::{
    CompositeType, Error, ExecutionState, ExternalValue, FunctionInstance, FunctionType,
    ImportDeclaration, ImportDescription, Instance, MemoryInstance, Module, RawValue, Result,
    Store,
};

type HostCallback = dyn for<'a> Fn(Caller<'a>, &[RawValue]) -> Result<Vec<RawValue>>;

struct HostFunc {
    function_type: FunctionType,
    callback: Box<HostCallback>,
}

pub struct Linker {
    funcs: HashMap<String, HashMap<String, HostFunc>>,
    allow_shadowing: bool,
}

impl Default for Linker {
    fn default() -> Self {
        Self::new()
    }
}

impl Linker {
    pub fn new() -> Self {
        Self {
            funcs: HashMap::new(),
            allow_shadowing: false,
        }
    }

    pub const fn allow_shadowing(&mut self, allow: bool) -> &mut Self {
        self.allow_shadowing = allow;
        self
    }

    pub fn func_new<F>(
        &mut self,
        module: &str,
        name: &str,
        function_type: FunctionType,
        callback: F,
    ) -> Result<&mut Self>
    where
        F: for<'a> Fn(Caller<'a>, &[RawValue]) -> Result<Vec<RawValue>> + 'static,
    {
        let module_funcs = self.funcs.entry(module.to_string()).or_default();
        if !self.allow_shadowing && module_funcs.contains_key(name) {
            return Err(Error::Instantiation(format!(
                "import {module}.{name} defined twice"
            )));
        }

        module_funcs.insert(
            name.to_string(),
            HostFunc {
                function_type,
                callback: Box::new(callback),
            },
        );

        Ok(self)
    }

    pub fn instantiate(&self, store: &mut Store, module: &Module) -> Result<Instance> {
        let resolved_imports = module
            .import_declarations()
            .iter()
            .map(|import| self.resolve_func_import(module, import))
            .collect::<Result<Vec<_>>>()?;

        let external_addresses = resolved_imports
            .into_iter()
            .map(|(module_name, function_name, function_type)| {
                let addr = store.functions.len();

                store.functions.push(FunctionInstance::Host {
                    function_type,
                    module_name,
                    function_name,
                });

                ExternalValue::Function { addr }
            })
            .collect::<Vec<_>>();

        store.instantiate(module, external_addresses)
    }

    pub fn invoke<I>(
        &self,
        store: &mut Store,
        instance: Instance,
        name: &str,
        args: I,
    ) -> Result<ExecutionState>
    where
        I: IntoIterator<IntoIter: ExactSizeIterator>,
        I::Item: Into<RawValue>,
    {
        let state = store.invoke(instance, name, args)?;

        self.dispatch(store, instance, state)
    }

    pub fn dispatch(
        &self,
        store: &mut Store,
        instance: Instance,
        mut state: ExecutionState,
    ) -> Result<ExecutionState> {
        loop {
            match state {
                ExecutionState::Suspended {
                    module_name,
                    func_name,
                    args,
                } => {
                    let func = self.get_func(&module_name, &func_name)?;
                    self.validate_arg_count(func, &module_name, &func_name, args.len())?;

                    let expected_results = func.function_type.1 .0.len();
                    let caller = Caller { store, instance };
                    let results = (func.callback)(caller, &args)?;

                    if results.len() != expected_results {
                        return Err(Error::Instantiation(format!(
                            "host function {module_name}.{func_name} returned {} values, expected {expected_results}",
                            results.len()
                        )));
                    }

                    state = store.resume_with(&results)?;
                }
                state => return Ok(state),
            }
        }
    }

    fn resolve_func_import(
        &self,
        module: &Module,
        import: &ImportDeclaration,
    ) -> Result<(String, String, FunctionType)> {
        let ImportDescription::Func(type_index) = import.description else {
            return Err(Error::Instantiation(format!(
                "unsupported import kind for {}.{}",
                import.module, import.name
            )));
        };

        let expected = module
            .types()
            .get(usize::try_from(type_index).map_err(|_| {
                Error::Instantiation(format!(
                    "function type index {type_index} for {}.{} is out of range",
                    import.module, import.name
                ))
            })?)
            .ok_or_else(|| {
                Error::Instantiation(format!(
                    "function type index {type_index} for {}.{} not found",
                    import.module, import.name
                ))
            })
            .and_then(|ty| match &ty.composite_type {
                CompositeType::Func(function_type) => Ok(function_type.clone()),
                _ => Err(Error::Instantiation(format!(
                    "type index {type_index} for {}.{} is not a function type",
                    import.module, import.name
                ))),
            })?;

        let func = self.get_func(&import.module, &import.name)?;
        if func.function_type != expected {
            return Err(Error::Instantiation(format!(
                "incompatible import type for {}.{}: expected {:?}, got {:?}",
                import.module, import.name, expected, func.function_type
            )));
        }

        Ok((import.module.clone(), import.name.clone(), expected))
    }

    fn get_func(&self, module: &str, name: &str) -> Result<&HostFunc> {
        self.funcs
            .get(module)
            .and_then(|module_funcs| module_funcs.get(name))
            .ok_or_else(|| Error::Instantiation(format!("unknown import {module}.{name}")))
    }

    fn validate_arg_count(
        &self,
        func: &HostFunc,
        module: &str,
        name: &str,
        actual: usize,
    ) -> Result<()> {
        let expected = func.function_type.0 .0.len();
        if actual != expected {
            return Err(Error::Instantiation(format!(
                "host function {module}.{name} received {actual} args, expected {expected}"
            )));
        }

        Ok(())
    }
}

pub struct Caller<'a> {
    store: &'a mut Store,
    instance: Instance,
}

impl<'a> Caller<'a> {
    pub const fn store(&self) -> &Store {
        self.store
    }

    pub const fn store_mut(&mut self) -> &mut Store {
        self.store
    }

    pub fn memory(&self, name: &str) -> Result<&MemoryInstance> {
        let addr = memory_addr(self.store, self.instance, name)?;

        Ok(&self.store.memories[addr])
    }

    pub fn memory_mut(&mut self, name: &str) -> Result<&mut MemoryInstance> {
        let addr = memory_addr(self.store, self.instance, name)?;

        Ok(&mut self.store.memories[addr])
    }
}

fn memory_addr(store: &Store, instance: Instance, name: &str) -> Result<usize> {
    for export in store.exports(instance) {
        if export.name == name {
            if let ExternalValue::Memory { addr } = export.value {
                return Ok(addr);
            }

            return Err(Error::Instantiation(format!(
                "export '{name}' is not a memory"
            )));
        }
    }

    Err(Error::Instantiation(format!(
        "memory export '{name}' not found"
    )))
}
