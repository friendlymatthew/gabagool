use crate::binary_grammar::ParsedModule;
use crate::Result;

use super::binary_grammar::{
    Alias, ComponentSection, ComponentSort, ComponentTypeDef, CoreInstance, CoreSort,
    ParsedCanonOpts, ParsedComponent,
};

#[derive(Debug)]
pub struct FlattenedComponent {
    pub modules: Vec<Box<ParsedModule>>,
    pub types: Vec<ComponentTypeDef>,
    pub initializers: Vec<Initializer>,
}

#[derive(Debug)]
pub enum Initializer {
    InstantiateModule {
        module_i: usize,
    },
    AliasCoreFunc {
        instance_i: usize,
        name: String,
    },
    AliasCoreMemory {
        instance_i: usize,
        name: String,
    },
    Lift {
        core_func_i: usize,
        opts: ParsedCanonOpts,
        type_i: u32,
    },
    Export {
        name: String,
        func_i: usize,
    },
}

pub fn flatten(parsed: &ParsedComponent) -> Result<FlattenedComponent> {
    let mut modules = Vec::new();
    let mut types = Vec::new();
    let mut initializers = Vec::new();

    for section in &parsed.sections {
        match section {
            ComponentSection::CoreModule(parsed_module) => {
                modules.push(parsed_module.clone());
            }
            ComponentSection::CoreInstance(instances) => {
                for instance in instances {
                    match instance {
                        CoreInstance::Instantiate { module_i, args: _ } => {
                            initializers.push(Initializer::InstantiateModule {
                                module_i: *module_i as usize,
                            });
                        }
                        CoreInstance::FromExports(_) => {
                            todo!("core instance from exports")
                        }
                    }
                }
            }
            ComponentSection::Alias(aliases) => {
                for alias in aliases {
                    match alias {
                        Alias::CoreExport {
                            sort,
                            instance_i,
                            name,
                        } => match sort {
                            ComponentSort::Core(CoreSort::Func) => {
                                initializers.push(Initializer::AliasCoreFunc {
                                    instance_i: *instance_i as usize,
                                    name: name.clone(),
                                });
                            }
                            ComponentSort::Core(CoreSort::Memory) => {
                                initializers.push(Initializer::AliasCoreMemory {
                                    instance_i: *instance_i as usize,
                                    name: name.clone(),
                                });
                            }
                            _ => todo!("alias core export sort {:?}", sort),
                        },
                        _ => todo!("alias kind {:?}", alias),
                    }
                }
            }
            ComponentSection::Canonical(defs) => {
                for def in defs {
                    match def {
                        crate::CanonicalDef::Lift {
                            core_func_i,
                            opts,
                            type_i,
                        } => {
                            initializers.push(Initializer::Lift {
                                core_func_i: *core_func_i as usize,
                                opts: opts.clone(),
                                type_i: *type_i,
                            });
                        }
                        crate::CanonicalDef::Lower { .. } => {
                            todo!("canon lower")
                        }
                        _ => todo!("canonical def {:?}", def),
                    }
                }
            }
            ComponentSection::Export(exports) => {
                for export in exports {
                    match export.sort {
                        ComponentSort::Func => {
                            initializers.push(Initializer::Export {
                                name: export.name.clone(),
                                func_i: export.i as usize,
                            });
                        }
                        _ => todo!("export sort {:?}", export.sort),
                    }
                }
            }
            ComponentSection::CoreType(_) => {}
            ComponentSection::ComponentType(defs) => {
                types.extend(defs.iter().cloned());
            }
            other => todo!("component section {:?}", other),
        }
    }

    Ok(FlattenedComponent {
        modules,
        types,
        initializers,
    })
}
