use crate::base64;
use crate::source_map::WatSourceMap;
use crate::transport::Transport;
use gabagool::debugger::{Breakpoint, Debugger, StepResult};
use gabagool::{Module, RawValue, Store, ValueType};
use serde_json::{json, Value};
use std::io::{Error, Result};
use std::{fs, iter};

fn err(e: impl std::fmt::Display) -> Error {
    Error::other(e.to_string())
}

pub struct DAPServer {
    transport: Transport,
    seq: i64,
    debugger: Option<Debugger>,
    source_map: Option<WatSourceMap>,
    // offset for local func index.
    num_imported_funcs: u32,
}

impl DAPServer {
    pub const fn new(transport: Transport) -> Self {
        Self {
            transport,
            seq: 0,
            debugger: None,
            source_map: None,
            num_imported_funcs: 0,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        while let Some(msg) = self.transport.read_message()? {
            let command = msg["command"].as_str().unwrap_or("");
            let request_seq = msg["seq"].as_i64().unwrap_or(0);

            match command {
                "initialize" => {
                    let body = json!({
                        "supportsStepBack": true,
                        "supportsConfigurationDoneRequest": true,
                        "supportsReadMemoryRequest": true,
                        "supportsMemoryEvent": true,
                    });
                    self.send_response(request_seq, command, body)?;
                }
                "launch" => self.handle_launch(request_seq, &msg)?,
                "configurationDone" => self.handle_configuration_done(request_seq)?,
                "threads" => self.handle_threads(request_seq)?,
                "stackTrace" => self.handle_stack_trace(request_seq)?,
                "scopes" => self.handle_scopes(request_seq, &msg)?,
                "variables" => self.handle_variables(request_seq, &msg)?,
                "setBreakpoints" => self.handle_set_breakpoints(request_seq, &msg)?,
                "next" | "stepIn" => self.handle_step_forward(request_seq, command)?,
                "stepOut" => self.handle_step_out(request_seq)?,
                "stepBack" => self.handle_step_back(request_seq)?,
                "continue" => self.handle_continue(request_seq)?,
                "reverseContinue" => self.handle_reverse_continue(request_seq)?,
                "readMemory" => self.handle_read_memory(request_seq, &msg)?,
                "disconnect" => {
                    self.send_response(request_seq, "disconnect", json!({}))?;
                    break;
                }
                _ => {
                    self.send_response(request_seq, command, json!({}))?;
                }
            }
        }

        Ok(())
    }

    fn source_map(&self) -> Result<&WatSourceMap> {
        self.source_map
            .as_ref()
            .ok_or_else(|| err("source map not initialized"))
    }

    fn debugger(&self) -> Result<&Debugger> {
        self.debugger
            .as_ref()
            .ok_or_else(|| err("debugger not initialized"))
    }

    fn debugger_mut(&mut self) -> Result<&mut Debugger> {
        self.debugger
            .as_mut()
            .ok_or_else(|| err("debugger not initialized"))
    }

    fn handle_launch(&mut self, request_seq: i64, msg: &Value) -> Result<()> {
        let args = &msg["arguments"];
        let program = args["program"].as_str().unwrap_or("");
        let function_arg = args["function"].as_str().unwrap_or("");

        let call_args = args["args"].as_array().map_or_else(
            || {
                args["args"].as_str().map_or_else(Vec::new, |s| {
                    s.split(',')
                        .filter_map(|s| s.trim().parse::<i32>().ok())
                        .map(RawValue::from)
                        .collect()
                })
            },
            |arr| {
                arr.iter()
                    .filter_map(|v| v.as_i64().map(|n| RawValue::from(n as i32)))
                    .collect()
            },
        );

        let source_path = args["source"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| program.replace(".wasm", ".wat"));

        let wat_source = fs::read_to_string(&source_path)?;
        let sm = WatSourceMap::from_wat(&source_path, &wat_source);

        let function = if function_arg.is_empty() {
            sm.first_user_func_name().unwrap_or("main").to_string()
        } else {
            function_arg.to_string()
        };

        self.source_map = Some(sm);

        match Self::create_debugger(program, &function, call_args) {
            Ok((debugger, num_imported_funcs)) => {
                self.debugger = Some(debugger);
                self.num_imported_funcs = num_imported_funcs;
                self.send_response(request_seq, "launch", json!({}))?;
                self.send_event("initialized", json!({}))
            }
            Err(e) => self.send_error_response(request_seq, "launch", &e.to_string()),
        }
    }

    fn create_debugger(
        program: &str,
        function: &str,
        args: Vec<RawValue>,
    ) -> Result<(Debugger, u32)> {
        let wasm = fs::read(program)?;
        let module = Module::new(&wasm).map_err(err)?;
        let num_imported_funcs = module
            .import_declarations()
            .iter()
            .filter(|imp| matches!(imp.description, gabagool::ImportDescription::Func(_)))
            .count() as u32;
        let mut store = Store::new();
        let instance = store.instantiate(&module, vec![]).map_err(err)?;
        let mut debugger = Debugger::new(store, instance);
        debugger.start(function, args).map_err(err)?;

        Ok((debugger, num_imported_funcs))
    }

    fn handle_configuration_done(&mut self, request_seq: i64) -> Result<()> {
        self.send_response(request_seq, "configurationDone", json!({}))?;
        self.send_event(
            "stopped",
            json!({
                "reason": "entry",
                "threadId": 1,
            }),
        )?;
        Ok(())
    }

    fn handle_threads(&mut self, request_seq: i64) -> Result<()> {
        self.send_response(
            request_seq,
            "threads",
            // because it's single threaded
            json!({
                "threads": [{"id": 1, "name": "wasm"}]
            }),
        )
    }

    fn handle_stack_trace(&mut self, request_seq: i64) -> Result<()> {
        let mut frames = self
            .debugger()?
            .call_stack()
            .enumerate()
            .map(|(i, frame)| {
                let local_func_idx = frame
                    .compiled_func_i
                    .saturating_sub(self.num_imported_funcs)
                    as usize;

                let sm = self
                    .source_map
                    .as_ref()
                    .expect("source map not initialized");
                let name = sm
                    .func_name(local_func_idx)
                    .unwrap_or(&format!("func_{}", frame.compiled_func_i))
                    .to_string();
                let line = sm
                    .instruction_to_line(local_func_idx, frame.source_position)
                    .unwrap_or(0);
                let source = json!({
                    "name": std::path::Path::new(&sm.path)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&sm.path),
                    "path": sm.path,
                });

                json!({
                    "id": i,
                    "name": name,
                    "line": line,
                    "column": 0,
                    "source": source,
                })
            })
            .collect::<Vec<_>>();

        frames.reverse();

        self.send_response(
            request_seq,
            "stackTrace",
            json!({
                "stackFrames": frames,
                "totalFrames": frames.len(),
            }),
        )
    }

    fn handle_scopes(&mut self, request_seq: i64, msg: &Value) -> Result<()> {
        let frame_id = msg["arguments"]["frameId"].as_i64().unwrap_or(0);
        self.send_response(
            request_seq,
            "scopes",
            json!({
                "scopes": [
                    { "name": "Locals", "variablesReference": Scope::Locals.encode(frame_id), "expensive": false },
                    { "name": "Globals", "variablesReference": Scope::Globals.encode(frame_id), "expensive": false },
                    { "name": "Stack", "variablesReference": Scope::Stack.encode(frame_id), "expensive": false },
                    { "name": "Memory", "variablesReference": Scope::Memory.encode(frame_id), "expensive": false },
                ]
            }),
        )
    }

    fn handle_variables(&mut self, request_seq: i64, msg: &Value) -> Result<()> {
        let var_ref = msg["arguments"]["variablesReference"].as_i64().unwrap_or(0);
        let (scope, frame_id) = Scope::decode(var_ref);

        let dbg = self.debugger()?;
        let frames = dbg.call_stack().collect::<Vec<_>>();

        if frames.is_empty() {
            return self.send_response(request_seq, "variables", json!({ "variables": [] }));
        }

        let frame_idx = frames.len().saturating_sub(1).saturating_sub(frame_id);

        let vars = match scope {
            Scope::Stack => {
                let stack = dbg.value_stack();

                iter::once(json!({
                    "name": "instruction_count",
                    "value": format!("{}", dbg.instruction_count()),
                    "type": "u64",
                    "variablesReference": 0,
                }))
                .chain(stack.iter().enumerate().rev().map(|(i, val)| {
                    let name = if i == stack.len() - 1 {
                        format!("[{i}] (top)")
                    } else {
                        format!("[{i}]")
                    };
                    json!({
                        "name": name,
                        "value": format!("{}", val.as_i32()),
                        "type": "i32",
                        "variablesReference": 0,
                    })
                }))
                .collect::<Vec<_>>()
            }
            Scope::Globals => {
                let frame = &frames[frame_idx];
                let globals = dbg.globals(frame.module_i);
                globals
                    .iter()
                    .enumerate()
                    .map(|(i, (gt, val))| {
                        json!({
                            "name": format!("global_{i}"),
                            "value": format_value(val, &gt.value_type),
                            "type": format!("{:?} {:?}", gt.mutability, gt.value_type),
                            "variablesReference": 0,
                        })
                    })
                    .collect::<Vec<_>>()
            }
            Scope::Locals => {
                let frame = &frames[frame_idx];
                let local_func_idx = frame
                    .compiled_func_i
                    .saturating_sub(self.num_imported_funcs)
                    as usize;

                frame
                    .locals
                    .iter()
                    .zip(frame.local_types.iter())
                    .enumerate()
                    .map(|(i, (val, ty))| {
                        let default_name = format!("local_{i}");
                        let sm = self
                            .source_map
                            .as_ref()
                            .expect("source map not initialized");
                        let name = sm
                            .local_name(local_func_idx, i)
                            .unwrap_or(&default_name)
                            .to_string();
                        json!({
                            "name": name,
                            "value": format_value(val, ty),
                            "type": format!("{ty:?}"),
                            "variablesReference": 0,
                        })
                    })
                    .collect::<Vec<_>>()
            }
            Scope::Memory => {
                let frame = &frames[frame_idx];
                let total = dbg.memory_size(frame.module_i, 0).unwrap_or(0);
                let mem_ref = format!("mem_{}_{}", frame.module_i, 0);

                vec![json!({
                    "name": "memory[0]",
                    "value": format!("{total} bytes"),
                    "type": "bytes",
                    "variablesReference": 0,
                    "memoryReference": mem_ref,
                })]
            }
        };

        self.send_response(request_seq, "variables", json!({ "variables": vars }))
    }

    fn handle_set_breakpoints(&mut self, request_seq: i64, msg: &Value) -> Result<()> {
        let args = &msg["arguments"];
        let bp_args = args["breakpoints"].as_array().cloned().unwrap_or_default();

        let mut breakpoints = Vec::new();
        let mut result_bps = Vec::new();

        for bp_arg in &bp_args {
            let line = bp_arg["line"].as_u64().unwrap_or(0) as usize;

            let sm = self.source_map()?;
            let resolved =
                sm.line_to_instruction(line)
                    .map(|(local_func_idx, instruction_index)| {
                        let compiled_func_idx = local_func_idx as u32 + self.num_imported_funcs;
                        Breakpoint::new(0, compiled_func_idx, instruction_index as usize)
                    });

            result_bps.push(json!({ "verified": resolved.is_some(), "line": line }));
            if let Some(bp) = resolved {
                breakpoints.push(bp);
            }
        }

        let dbg = self.debugger_mut()?;
        dbg.clear_breakpoints();
        for bp in breakpoints {
            dbg.set_breakpoint(bp);
        }

        self.send_response(
            request_seq,
            "setBreakpoints",
            json!({ "breakpoints": result_bps }),
        )
    }

    fn handle_step_out(&mut self, request_seq: i64) -> Result<()> {
        let result = self.debugger_mut()?.step_out().map_err(err)?;
        self.send_response(request_seq, "stepOut", json!({}))?;
        self.send_step_event(result)
    }

    fn handle_step_forward(&mut self, request_seq: i64, command: &str) -> Result<()> {
        let result = self.debugger_mut()?.step_forward().map_err(err)?;
        self.send_response(request_seq, command, json!({}))?;
        self.send_step_event(result)
    }

    fn handle_step_back(&mut self, request_seq: i64) -> Result<()> {
        let result = self.debugger_mut()?.step_back().map_err(err)?;
        self.send_response(request_seq, "stepBack", json!({}))?;
        self.send_step_event(result)
    }

    fn handle_continue(&mut self, request_seq: i64) -> Result<()> {
        let result = self.debugger_mut()?.continue_forward().map_err(err)?;
        self.send_response(request_seq, "continue", json!({}))?;
        self.send_step_event(result)
    }

    fn handle_reverse_continue(&mut self, request_seq: i64) -> Result<()> {
        let result = self.debugger_mut()?.continue_backward().map_err(err)?;
        self.send_response(request_seq, "reverseContinue", json!({}))?;
        self.send_step_event(result)
    }

    fn handle_read_memory(&mut self, request_seq: i64, msg: &Value) -> Result<()> {
        let args = &msg["arguments"];
        let mem_ref = args["memoryReference"].as_str().unwrap_or("mem_0_0");
        let offset = args["offset"].as_i64().unwrap_or(0) as usize;
        let count = args["count"].as_i64().unwrap_or(0) as usize;

        let (module_idx, mem_idx) = mem_ref
            .strip_prefix("mem_")
            .and_then(|s| s.split_once('_'))
            .and_then(|(m, i)| Some((m.parse::<u16>().ok()?, i.parse::<usize>().ok()?)))
            .unwrap_or((0, 0));

        let dbg = self.debugger()?;
        let data = dbg.read_memory(module_idx, mem_idx, offset, count);

        self.send_response(
            request_seq,
            "readMemory",
            json!({
                "address": format!("0x{offset:x}"),
                "data": base64::encode(data),
            }),
        )
    }

    fn send_step_event(&mut self, result: StepResult) -> Result<()> {
        self.send_event(
            "memory",
            // note: right now we're just hardcoding mod and mem idx
            json!({ "memoryReference": "mem_0_0", "offset": 0, "count": 0 }),
        )?;

        match result {
            StepResult::Stepped | StepResult::ReachedStart => {
                self.send_event("stopped", json!({"reason": "step", "threadId": 1}))
            }
            StepResult::BreakpointHit => {
                self.send_event("stopped", json!({"reason": "breakpoint", "threadId": 1}))
            }
            StepResult::Completed => self.send_event("terminated", json!({})),
            StepResult::Trap(trap) => self.send_event(
                "stopped",
                json!({"reason": "exception", "threadId": 1, "text": trap.to_string()}),
            ),
        }
    }

    fn send_response(&mut self, request_seq: i64, command: &str, body: Value) -> Result<()> {
        self.seq += 1;
        self.transport.write_message(&json!({
            "seq": self.seq,
            "type": "response",
            "request_seq": request_seq,
            "success": true,
            "command": command,
            "body": body,
        }))
    }

    fn send_error_response(
        &mut self,
        request_seq: i64,
        command: &str,
        message: &str,
    ) -> Result<()> {
        self.seq += 1;
        self.transport.write_message(&json!({
            "seq": self.seq,
            "type": "response",
            "request_seq": request_seq,
            "success": false,
            "command": command,
            "message": message,
        }))
    }

    fn send_event(&mut self, event: &str, body: Value) -> Result<()> {
        self.seq += 1;
        self.transport.write_message(&json!({
            "seq": self.seq,
            "type": "event",
            "event": event,
            "body": body,
        }))
    }
}

enum Scope {
    Locals,
    Stack,
    Globals,
    Memory,
}

const NUM_SCOPES: i64 = 4;

impl Scope {
    const fn encode(&self, frame_id: i64) -> i64 {
        let kind = match self {
            Self::Locals => 0,
            Self::Stack => 1,
            Self::Globals => 2,
            Self::Memory => 3,
        };
        frame_id * NUM_SCOPES + kind + 1
    }

    const fn decode(var_ref: i64) -> (Self, usize) {
        let adjusted = var_ref - 1;
        let frame_id = (adjusted / NUM_SCOPES) as usize;
        let scope = match adjusted % NUM_SCOPES {
            1 => Self::Stack,
            2 => Self::Globals,
            3 => Self::Memory,
            _ => Self::Locals,
        };
        (scope, frame_id)
    }
}

fn format_value(val: &RawValue, ty: &ValueType) -> String {
    match ty {
        ValueType::I32 => val.as_i32().to_string(),
        ValueType::I64 => val.as_i64().to_string(),
        ValueType::F32 => val.as_f32().to_string(),
        ValueType::F64 => val.as_f64().to_string(),
        _ => format!("{val:?}"),
    }
}
