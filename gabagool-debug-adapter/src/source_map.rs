use std::collections::HashMap;

use wast::core::{ExportKind, FuncKind, ModuleField, ModuleKind};
use wast::parser::{self, ParseBuffer};
use wast::Wat;

#[derive(Debug)]
pub struct WatSourceMap {
    // for every local function index, maps instruction index to wat line number
    func_maps: Vec<HashMap<u32, usize>>,
    // reverse
    line_to_instruction: HashMap<usize, (usize, u32)>,
    func_names: Vec<Option<String>>,
    local_names: Vec<Vec<String>>,
    func_exports: Vec<String>,
    pub path: String,
}

impl WatSourceMap {
    pub fn from_wat(path: &str, source: &str) -> Self {
        let mut source_map = Self {
            func_maps: Vec::new(),
            line_to_instruction: HashMap::new(),
            func_names: Vec::new(),
            local_names: Vec::new(),
            func_exports: Vec::new(),
            path: path.to_string(),
        };

        let Ok(mut buf) = ParseBuffer::new(source) else {
            eprintln!("failed to create parse buffer");
            return source_map;
        };
        buf.track_instr_spans(true);

        let Ok(wat) = parser::parse::<Wat>(&buf) else {
            eprintln!("failed to parse WAT");
            return source_map;
        };

        let Wat::Module(module) = wat else {
            eprintln!("expected module, got component");
            return source_map;
        };

        let ModuleKind::Text(fields) = &module.kind else {
            eprintln!("module is binary, not text");
            return source_map;
        };

        for field in fields.iter() {
            if let ModuleField::Export(export) = field {
                if matches!(export.kind, ExportKind::Func) {
                    source_map.func_exports.push(export.name.to_string());
                }
            }
        }

        let mut local_func_idx: usize = 0;

        for field in fields.iter() {
            let ModuleField::Func(func) = field else {
                continue;
            };

            let FuncKind::Inline { locals, expression } = &func.kind else {
                continue;
            };

            let name = func.id.map(|id| format!("${}", id.name()));
            source_map.func_names.push(name);

            let mut names = Vec::new();

            if let Some(ft) = &func.ty.inline {
                for (id, _name_ann, _ty) in ft.params.iter() {
                    names.push(id.as_ref().map_or_else(
                        || format!("param_{}", names.len()),
                        |id| format!("${}", id.name()),
                    ));
                }
            }

            for local in locals.iter() {
                names.push(local.id.map_or_else(
                    || format!("local_{}", names.len()),
                    |id| format!("${}", id.name()),
                ));
            }

            source_map.local_names.push(names);

            let mut instr_map = HashMap::new();
            if let Some(spans) = &expression.instr_spans {
                for (i, span) in spans.iter().enumerate() {
                    let (line_0, _col) = span.linecol_in(source);
                    let line_1 = line_0 + 1;

                    instr_map.insert(i as u32, line_1);
                    source_map
                        .line_to_instruction
                        .entry(line_1)
                        .or_insert((local_func_idx, i as u32));
                }
            }

            source_map.func_maps.push(instr_map);
            local_func_idx += 1;
        }

        source_map
    }

    pub fn instruction_to_line(
        &self,
        local_func_idx: usize,
        instruction_index: u32,
    ) -> Option<usize> {
        self.func_maps
            .get(local_func_idx)?
            .get(&instruction_index)
            .copied()
    }

    pub fn func_name(&self, local_func_idx: usize) -> Option<&str> {
        self.func_names.get(local_func_idx)?.as_deref()
    }

    pub fn line_to_instruction(&self, line: usize) -> Option<(usize, u32)> {
        self.line_to_instruction.get(&line).copied()
    }

    pub fn first_user_func_name(&self) -> Option<&str> {
        self.func_exports
            .iter()
            .find(|name| !name.starts_with("__"))
            .map(|s| s.as_str())
    }

    pub fn local_name(&self, local_func_idx: usize, local_idx: usize) -> Option<&str> {
        self.local_names
            .get(local_func_idx)?
            .get(local_idx)
            .map(|s| s.as_str())
    }
}

impl std::fmt::Display for WatSourceMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (func_idx, map) in self.func_maps.iter().enumerate() {
            let name = self
                .func_names
                .get(func_idx)
                .and_then(|n| n.as_deref())
                .unwrap_or("?");
            writeln!(f, "func {func_idx} ({name}):")?;

            let locals = &self.local_names[func_idx];
            if !locals.is_empty() {
                writeln!(f, "  locals: {}", locals.join(", "))?;
            }

            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(instr, _)| *instr);
            for (instr, line) in entries {
                writeln!(f, "  instr {instr} -> line {line}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fibonacci_source_map() {
        let wat = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-programs/fibonacci.wat"
        ))
        .unwrap();
        let sm = WatSourceMap::from_wat("fibonacci.wat", &wat);
        insta::assert_snapshot!(sm, @r"
        func 0 ($__wasm_call_ctors):
        func 1 ($fib):
          locals: param_0, local_1, local_2
          instr 0 -> line 7
          instr 1 -> line 8
          instr 2 -> line 9
          instr 3 -> line 10
          instr 4 -> line 11
          instr 5 -> line 12
          instr 6 -> line 13
          instr 7 -> line 14
          instr 8 -> line 15
          instr 9 -> line 16
          instr 10 -> line 17
          instr 11 -> line 18
          instr 12 -> line 19
          instr 13 -> line 20
          instr 14 -> line 21
          instr 15 -> line 22
          instr 16 -> line 23
          instr 17 -> line 24
          instr 18 -> line 25
          instr 19 -> line 26
          instr 20 -> line 27
          instr 21 -> line 28
          instr 22 -> line 29
          instr 23 -> line 30
          instr 24 -> line 31
          instr 25 -> line 32
          instr 26 -> line 33
          instr 27 -> line 34
          instr 28 -> line 35
          instr 29 -> line 36
          instr 30 -> line 37
        ");
    }
}
