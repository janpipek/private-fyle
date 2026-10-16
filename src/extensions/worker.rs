//! Runs one validated extension column request inside a restricted Lua state.

use std::{
    io::{self, Read, Write},
    sync::{Arc, Mutex},
};

use mlua::{Function, HookTriggers, Lua, LuaOptions, LuaString, StdLib, Table, Value, VmState};

use super::protocol::{
    self, Completeness, ContributionResult, FieldType, HostMessage, Init, OutputState, OutputValue,
    ReadRequest, Unit, VERSION, WorkerFrame, WorkerMessage,
};

const MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const INSTRUCTION_LIMIT: u64 = 5_000_000;
const HOOK_INTERVAL: u32 = 1_000;
const MAX_TRAVERSAL_NODES: usize = 4_096;
const MAX_READ_BYTES: u64 = 64 * 1024;
const MAX_NAME_BYTES: usize = 4_096;

/// Reads one Init frame, executes the selected contribution, and writes one terminal result.
///
/// The host supervises wall time, cancellation, and protocol byte totals. This
/// function separately limits Lua memory, instructions, source reads, and result traversal.
pub fn run<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<()> {
    let frame = protocol::read_host_frame(reader)?;
    let init = match frame.message {
        HostMessage::Init(init) => init,
        HostMessage::ReadResult(_) => return Err(invalid("worker expected Init")),
    };
    let result = match execute(&init, reader, writer) {
        Ok(result) => result,
        Err(error) => ContributionResult {
            field: None,
            state: OutputState::Failed {
                reason: bounded_error(&error),
            },
        },
    };
    protocol::write_worker_frame(
        writer,
        &WorkerFrame {
            version: VERSION,
            message: WorkerMessage::Result(result),
        },
    )?;
    writer.flush()
}

fn execute<R: Read, W: Write>(
    init: &Init,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<ContributionResult> {
    let package = super::manifest::PackageId::new(init.package.clone())?;
    let provider = crate::data_providers::ProviderId::new(init.provider.clone())?;
    crate::data_providers::ContributionId::new(init.contribution.clone())?;
    crate::data_providers::FieldId::new(init.field.clone())?;
    let package_prefix = format!("extension.{}.", package.as_str());
    let owner_prefix = format!("{}.", provider.as_str());
    if init.script.len() > super::manifest::SCRIPT_BYTES
        || init.name.len() > MAX_NAME_BYTES
        || init.provider.len() > 256
        || init.contribution.len() > 256
        || init.field.len() > 256
        || !init.provider.starts_with(&package_prefix)
        || !init.contribution.starts_with(&owner_prefix)
        || !init.field.starts_with(&owner_prefix)
        || init.is_dir
        || init.demand.max_bytes == 0
        || init.demand.max_bytes > super::manifest::MAX_DEMAND_BYTES
        || init.demand.max_items == 0
        || init.demand.max_items > super::manifest::MAX_DEMAND_ITEMS
    {
        return Err(invalid(
            "worker received invalid script, context, or demand bounds",
        ));
    }
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
        LuaOptions::default(),
    )
    .map_err(lua_error)?;
    lua.set_memory_limit(MEMORY_LIMIT).map_err(lua_error)?;
    let instructions = Arc::new(Mutex::new(0u64));
    let counter = instructions.clone();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
        move |_lua, _debug| {
            let mut count = counter
                .lock()
                .expect("instruction counter mutex is not poisoned");
            *count = count.saturating_add(HOOK_INTERVAL as u64);
            if *count >= INSTRUCTION_LIMIT {
                return Err(mlua::Error::RuntimeError(
                    "instruction limit exceeded".into(),
                ));
            }
            Ok(VmState::Continue)
        },
    )
    .map_err(lua_error)?;

    for name in [
        "dofile",
        "loadfile",
        "load",
        "collectgarbage",
        "print",
        "warn",
    ] {
        lua.globals().set(name, Value::Nil).map_err(lua_error)?;
    }
    lua.globals()
        .get::<Table>("string")
        .and_then(|string| string.set("dump", Value::Nil))
        .map_err(lua_error)?;

    let result = lua
        .scope(|scope| {
            let mut next_read_id = 0u32;
            let source_handle = init.source_handle;
            let read_reader = &mut *reader;
            let read_writer = &mut *writer;
            let fyle = lua.create_table().map_err(lua_error)?;
            let read = scope
                .create_function_mut(
                    move |_lua, (handle, offset, length): (Value, Value, Value)| {
                        let handle = integer_u32(handle)?;
                        let offset = integer_u64(offset)?;
                        let length = integer_u64(length)?;
                        if handle != source_handle || length > MAX_READ_BYTES {
                            return Err(mlua::Error::RuntimeError(
                                "invalid bounded read request".into(),
                            ));
                        }
                        offset.checked_add(length).ok_or_else(|| {
                            mlua::Error::RuntimeError("read range overflows".into())
                        })?;
                        next_read_id = next_read_id.checked_add(1).ok_or_else(|| {
                            mlua::Error::RuntimeError("read request identity exhausted".into())
                        })?;
                        let id = next_read_id;
                        let request = WorkerFrame {
                            version: VERSION,
                            message: WorkerMessage::Read(ReadRequest {
                                read_id: id,
                                source_handle: handle,
                                offset,
                                length,
                            }),
                        };
                        protocol::write_worker_frame(read_writer, &request)
                            .map_err(mlua::Error::external)?;
                        read_writer.flush().map_err(mlua::Error::external)?;
                        let response = protocol::read_host_frame(read_reader)
                            .map_err(mlua::Error::external)?;
                        let HostMessage::ReadResult(reply) = response.message else {
                            return Err(mlua::Error::RuntimeError(
                                "unexpected host message".into(),
                            ));
                        };
                        if reply.read_id != id {
                            return Err(mlua::Error::RuntimeError("mismatched read reply".into()));
                        }
                        if let Some(error) = reply.error {
                            if !reply.bytes.is_empty()
                                || error.len() > protocol::MAX_RESULT_TEXT_BYTES
                            {
                                return Err(mlua::Error::RuntimeError(
                                    "invalid read error response".into(),
                                ));
                            }
                            return Err(mlua::Error::RuntimeError(error));
                        }
                        if reply.bytes.len() as u64 > length
                            || reply.bytes.len() > MAX_READ_BYTES as usize
                        {
                            return Err(mlua::Error::RuntimeError(
                                "host returned oversized read".into(),
                            ));
                        }
                        Ok((LuaString::wrap(reply.bytes), reply.eof))
                    },
                )
                .map_err(lua_error)?;
            fyle.set("read", read).map_err(lua_error)?;
            lua.globals().set("fyle", fyle).map_err(lua_error)?;

            let module = lua
                .load(&init.script)
                .set_mode(mlua::chunk::ChunkMode::Text)
                .eval::<Table>()
                .map_err(lua_error)?;
            let contribution: Table = module
                .raw_get(init.contribution.as_str())
                .map_err(lua_error)?;
            let applies: Value = contribution.raw_get("applies").map_err(lua_error)?;
            if !matches!(applies, Value::Nil) {
                let callback = match applies {
                    Value::Function(function) => function,
                    _ => {
                        return Err(mlua::Error::external(invalid(
                            "applies callback must be a function",
                        )));
                    }
                };
                let ctx = context(&lua, init).map_err(lua_error)?;
                let applies_result: Value = callback.call(ctx).map_err(lua_error)?;
                match applies_result {
                    Value::Boolean(true) => {}
                    Value::Boolean(false) => {
                        return Ok(ContributionResult {
                            field: None,
                            state: OutputState::Unavailable {
                                reason: "contribution does not apply".into(),
                            },
                        });
                    }
                    _ => {
                        return Err(mlua::Error::external(invalid(
                            "applies callback must return a boolean",
                        )));
                    }
                }
            }
            let callback: Function = contribution.raw_get("read").map_err(|_| {
                mlua::Error::external(invalid("selected contribution has no read callback"))
            })?;
            let ctx = context(&lua, init).map_err(lua_error)?;
            let value: Value = callback.call(ctx).map_err(lua_error)?;
            convert_result(&value, init).map_err(mlua::Error::external)
        })
        .map_err(lua_error)?;
    Ok(result)
}

fn context(lua: &Lua, init: &Init) -> mlua::Result<Table> {
    let ctx = lua.create_table()?;
    ctx.set("source", init.source_handle)?;
    ctx.set("name", init.name.as_str())?;
    ctx.set("is_dir", init.is_dir)?;
    let demand = lua.create_table()?;
    demand.set("max_bytes", init.demand.max_bytes)?;
    demand.set("max_items", init.demand.max_items)?;
    ctx.set("demand", demand)?;
    Ok(ctx)
}

fn convert_result(value: &Value, init: &Init) -> io::Result<ContributionResult> {
    let table = match value {
        Value::Table(table) => table,
        _ => return Err(invalid("read callback must return a result table")),
    };
    check_table(
        table,
        &["state", "field", "value", "completeness", "reason"],
    )?;
    let state = required_string(table, "state", 32)?;
    let result = match state.as_str() {
        "absent" => ContributionResult {
            field: None,
            state: OutputState::Absent,
        },
        "unavailable" | "failed" => {
            check_table(table, &["state", "reason"])?;
            let reason = bounded_lua_string(table.raw_get("reason").map_err(lua_error)?)?;
            let state = if state == "unavailable" {
                OutputState::Unavailable { reason }
            } else {
                OutputState::Failed { reason }
            };
            ContributionResult { field: None, state }
        }
        "ready" => {
            check_table(table, &["state", "field", "value", "completeness"])?;
            let field = required_string(table, "field", 256)?;
            if field != init.field {
                return Err(invalid("result field does not match selected column"));
            }
            let lua_value: Table = table.raw_get("value").map_err(lua_error)?;
            check_table(&lua_value, &["kind", "value", "unit"])?;
            let kind = required_string(&lua_value, "kind", 16)?;
            let output_value = match (init.field_type, kind.as_str()) {
                (FieldType::Text, "text") => {
                    check_table(&lua_value, &["kind", "value"])?;
                    let value = bounded_lua_string(lua_value.raw_get("value").map_err(lua_error)?)?;
                    OutputValue::Text { value }
                }
                (FieldType::Quantity(expected), "quantity") => {
                    let value = required_number(&lua_value, "value")?;
                    let unit_name = required_string(&lua_value, "unit", 16)?;
                    let unit = match unit_name.as_str() {
                        "seconds" => Unit::Seconds,
                        "metres" => Unit::Metres,
                        _ => return Err(invalid("unknown quantity unit")),
                    };
                    if unit != expected || !value.is_finite() || value < 0.0 {
                        return Err(invalid("quantity does not match declared field"));
                    }
                    OutputValue::Quantity { value, unit }
                }
                _ => return Err(invalid("value does not match declared field type")),
            };
            let completeness: Table = table.raw_get("completeness").map_err(lua_error)?;
            check_table(&completeness, &["kind", "reason"])?;
            let completeness_kind = required_string(&completeness, "kind", 32)?;
            let completeness = match completeness_kind.as_str() {
                "complete" => {
                    check_table(&completeness, &["kind"])?;
                    Completeness::Complete
                }
                "lower_bound" => {
                    check_table(&completeness, &["kind"])?;
                    Completeness::LowerBound
                }
                "truncated" => Completeness::Truncated {
                    reason: {
                        check_table(&completeness, &["kind", "reason"])?;
                        bounded_lua_string(completeness.raw_get("reason").map_err(lua_error)?)?
                    },
                },
                _ => return Err(invalid("unknown completeness state")),
            };
            ContributionResult {
                field: Some(field),
                state: OutputState::Ready {
                    value: output_value,
                    completeness,
                },
            }
        }
        _ => return Err(invalid("unknown contribution result state")),
    };
    match &result.state {
        OutputState::Absent => check_table(table, &["state"])?,
        OutputState::Unavailable { .. } | OutputState::Failed { .. } => {
            check_table(table, &["state", "reason"])?;
        }
        OutputState::Ready { .. } => {}
    }
    Ok(result)
}

fn check_table(table: &Table, allowed: &[&str]) -> io::Result<()> {
    if table.metatable().is_some() {
        return Err(invalid("Lua result tables cannot have metatables"));
    }
    let mut nodes = 0usize;
    for pair in table.clone().pairs::<Value, Value>() {
        nodes += 1;
        if nodes > MAX_TRAVERSAL_NODES {
            return Err(invalid("Lua result exceeds traversal limit"));
        }
        let (key, _value) = pair.map_err(lua_error)?;
        let key = match key {
            Value::String(key) => key.to_str().map_err(lua_error)?,
            _ => return Err(invalid("Lua result keys must be strings")),
        };
        if !allowed.iter().any(|allowed| key.as_ref() == *allowed) {
            return Err(invalid("Lua result contains an unknown field"));
        }
    }
    Ok(())
}

fn required_string(table: &Table, key: &str, max_bytes: usize) -> io::Result<String> {
    match table.raw_get::<Value>(key).map_err(lua_error)? {
        Value::String(value) => {
            if value.as_bytes().len() > max_bytes {
                return Err(invalid("result field is too long"));
            }
            let value = value.to_str().map_err(lua_error)?;
            Ok(value.to_string())
        }
        _ => Err(invalid("result field must be a string")),
    }
}

fn required_number(table: &Table, key: &str) -> io::Result<f64> {
    match table.raw_get::<Value>(key).map_err(lua_error)? {
        Value::Integer(value) => Ok(value as f64),
        Value::Number(value) => Ok(value),
        _ => Err(invalid("quantity must be a number")),
    }
}

fn integer_u32(value: Value) -> mlua::Result<u32> {
    match value {
        Value::Integer(value) => u32::try_from(value)
            .map_err(|_| mlua::Error::RuntimeError("read handle is out of range".into())),
        _ => Err(mlua::Error::RuntimeError(
            "read handle must be an unsigned integer".into(),
        )),
    }
}

fn integer_u64(value: Value) -> mlua::Result<u64> {
    match value {
        Value::Integer(value) => u64::try_from(value)
            .map_err(|_| mlua::Error::RuntimeError("read range is out of range".into())),
        _ => Err(mlua::Error::RuntimeError(
            "read range must use unsigned integers".into(),
        )),
    }
}

fn bounded_lua_string(value: Value) -> io::Result<String> {
    let value = match value {
        Value::String(value) => value,
        _ => return Err(invalid("result text must be a string")),
    };
    let value = value.to_str().map_err(lua_error)?;
    if value.len() > protocol::MAX_RESULT_TEXT_BYTES {
        return Err(invalid("result text exceeds size limit"));
    }
    Ok(value.to_string())
}

fn bounded_error(error: &io::Error) -> String {
    let mut message = error.to_string();
    if message.len() > protocol::MAX_RESULT_TEXT_BYTES {
        let mut end = protocol::MAX_RESULT_TEXT_BYTES;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    message
}

fn lua_error(error: mlua::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::run;
    use crate::extensions::protocol::{
        self, Demand, FieldType, HostFrame, HostMessage, Init, OutputState, ReadRequest, Unit,
        WorkerMessage,
    };

    fn init(script: &str, field_type: FieldType) -> Init {
        Init {
            package: "sensor".into(),
            provider: "extension.sensor.reader".into(),
            contribution: "extension.sensor.reader.distance-cell".into(),
            field: "extension.sensor.reader.distance".into(),
            field_type,
            demand: Demand {
                max_bytes: 1024,
                max_items: 10,
            },
            name: "track.gpx".into(),
            is_dir: false,
            source_handle: 9,
            script: script.into(),
        }
    }

    fn invoke(init: Init, additional_host_frames: &[u8]) -> Vec<u8> {
        let mut input = Vec::new();
        protocol::write_host_frame(
            &mut input,
            &HostFrame {
                version: protocol::VERSION,
                message: HostMessage::Init(init),
            },
        )
        .expect("valid init frame encodes");
        input.extend_from_slice(additional_host_frames);
        let mut output = Vec::new();
        run(&mut Cursor::new(input), &mut output).expect("worker writes terminal result");
        output
    }

    fn result(output: &[u8]) -> protocol::ContributionResult {
        match protocol::read_worker_frame(&mut Cursor::new(output.to_vec()))
            .expect("worker result frame decodes")
            .message
        {
            WorkerMessage::Result(result) => result,
            WorkerMessage::Read(_) => panic!("unexpected unhandled read"),
        }
    }

    fn terminal_result(output: &[u8]) -> protocol::ContributionResult {
        let mut reader = Cursor::new(output.to_vec());
        loop {
            match protocol::read_worker_frame(&mut reader)
                .expect("worker frame decodes")
                .message
            {
                WorkerMessage::Read(_) => {}
                WorkerMessage::Result(result) => return result,
            }
        }
    }

    #[test]
    fn only_the_selected_contribution_runs_and_ambient_apis_are_denied() {
        let script = r#"
            return { ["extension.sensor.reader.distance-cell"] = {
                read = function(ctx)
                    return { state="ready", field="extension.sensor.reader.distance",
                        value={kind="quantity", value=42.5, unit="metres"},
                        completeness={kind="complete"} }
                end
            } }
        "#;
        let ready_result = result(&invoke(
            init(script, FieldType::Quantity(Unit::Metres)),
            &[],
        ));
        assert!(matches!(ready_result.state, OutputState::Ready { .. }));

        let denied = r#"
            assert(io == nil and os == nil and package == nil and debug == nil and coroutine == nil)
            assert(require == nil and dofile == nil and loadfile == nil and load == nil)
            assert(collectgarbage == nil and print == nil and string.dump == nil)
            return { ["extension.sensor.reader.distance-cell"] = {
                read = function() return {state="absent"} end
            } }
        "#;
        assert_eq!(
            result(&invoke(
                init(denied, FieldType::Quantity(Unit::Metres)),
                &[]
            ))
            .state,
            OutputState::Absent
        );
    }

    #[test]
    fn memory_budget_applies_during_module_evaluation() {
        let memory = r#"local x = string.rep('x', 40 * 1024 * 1024)
            return { ["extension.sensor.reader.distance-cell"] = {
                read=function() return { state="absent" } end } }"#;
        let OutputState::Failed { reason } =
            result(&invoke(init(memory, FieldType::Text), &[])).state
        else {
            panic!("large allocation should fail at the memory limit");
        };
        assert!(reason.contains("memory"), "{reason}");
    }

    #[test]
    fn applicability_errors_partial_results_and_schema_are_preserved_or_rejected() {
        let not_applicable = r#"return { ["extension.sensor.reader.distance-cell"] = {
            applies=function() return false end, read=function() error("must not run") end } }"#;
        assert!(matches!(
            result(&invoke(init(not_applicable, FieldType::Text), &[])).state,
            OutputState::Unavailable { .. }
        ));
        let partial = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function() return { state="ready", field="extension.sensor.reader.distance",
                value={kind="text", value="ok"}, completeness={kind="truncated", reason="item cap"} }
            end } }"#;
        let partial_result = result(&invoke(init(partial, FieldType::Text), &[]));
        assert!(matches!(
            partial_result.state,
            OutputState::Ready {
                completeness: protocol::Completeness::Truncated { .. },
                ..
            }
        ));
        let lower_bound = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function() return { state="ready", field="extension.sensor.reader.distance",
                value={kind="text", value="observed"},
                completeness={kind="lower_bound"} } end } }"#;
        assert!(matches!(
            result(&invoke(init(lower_bound, FieldType::Text), &[])).state,
            OutputState::Ready {
                completeness: protocol::Completeness::LowerBound,
                ..
            }
        ));
        let wrong_unit = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function() return { state="ready", field="extension.sensor.reader.distance",
                value={kind="quantity", value=1, unit="seconds"}, completeness={kind="complete"} }
            end } }"#;
        assert!(matches!(
            result(&invoke(
                init(wrong_unit, FieldType::Quantity(Unit::Metres)),
                &[]
            ))
            .state,
            OutputState::Failed { .. }
        ));
        let wrong_field = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function() return { state="ready", field="extension.sensor.reader.name",
                value={kind="text", value="ok"}, completeness={kind="complete"} }
            end } }"#;
        assert!(matches!(
            result(&invoke(init(wrong_field, FieldType::Text), &[])).state,
            OutputState::Failed { .. }
        ));
        let oversized = format!(
            "return {{ ['extension.sensor.reader.distance-cell'] = {{ read=function() return {{ state='ready', field='extension.sensor.reader.distance', value={{kind='text', value=string.rep('x', {})}}, completeness={{kind='complete'}} }} end }} }}",
            protocol::MAX_RESULT_TEXT_BYTES + 1
        );
        assert!(matches!(
            result(&invoke(init(&oversized, FieldType::Text), &[])).state,
            OutputState::Failed { .. }
        ));
    }

    #[test]
    fn source_reads_use_sequenced_bounded_rpc_and_return_byte_strings() {
        let script = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function(ctx)
                local bytes, eof = fyle.read(ctx.source, 0, 3)
                return {state="ready", field="extension.sensor.reader.distance",
                    value={kind="text", value=bytes .. tostring(eof)}, completeness={kind="complete"}}
            end } }"#;
        let mut host_reply = Vec::new();
        protocol::write_host_frame(
            &mut host_reply,
            &HostFrame {
                version: protocol::VERSION,
                message: HostMessage::ReadResult(protocol::ReadResult {
                    read_id: 1,
                    bytes: vec![b'a', 0, b'b'],
                    eof: true,
                    error: None,
                }),
            },
        )
        .expect("valid read reply encodes");
        let mut input = Vec::new();
        protocol::write_host_frame(
            &mut input,
            &HostFrame {
                version: protocol::VERSION,
                message: HostMessage::Init(init(script, FieldType::Text)),
            },
        )
        .expect("valid init frame encodes");
        input.extend_from_slice(&host_reply);
        let mut output = Vec::new();
        run(&mut Cursor::new(input), &mut output).expect("worker completes");
        let mut output_reader = Cursor::new(output);
        assert!(matches!(
            protocol::read_worker_frame(&mut output_reader)
                .unwrap()
                .message,
            WorkerMessage::Read(ReadRequest {
                read_id: 1,
                source_handle: 9,
                length: 3,
                ..
            })
        ));
        let terminal = protocol::read_worker_frame(&mut output_reader)
            .unwrap()
            .message;
        let WorkerMessage::Result(terminal) = terminal else {
            panic!("expected terminal result after read reply");
        };
        assert!(matches!(
            terminal.state,
            OutputState::Ready {
                value: protocol::OutputValue::Text { ref value },
                ..
            } if value == "a\0btrue"
        ));
    }

    #[test]
    fn source_read_replies_are_checked_against_the_pending_request() {
        let script = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function(ctx)
                local bytes = fyle.read(ctx.source, 0, 3)
                return {state="ready", field="extension.sensor.reader.distance",
                    value={kind="text", value=bytes}, completeness={kind="complete"}}
            end } }"#;
        let cases = [
            (
                protocol::ReadResult {
                    read_id: 2,
                    bytes: Vec::new(),
                    eof: false,
                    error: None,
                },
                "mismatched read reply",
            ),
            (
                protocol::ReadResult {
                    read_id: 1,
                    bytes: vec![b'x'; 4],
                    eof: false,
                    error: None,
                },
                "oversized read",
            ),
            (
                protocol::ReadResult {
                    read_id: 1,
                    bytes: vec![b'x'],
                    eof: false,
                    error: Some("source read failed".into()),
                },
                "invalid read error response",
            ),
            (
                protocol::ReadResult {
                    read_id: 1,
                    bytes: Vec::new(),
                    eof: false,
                    error: Some("x".repeat(protocol::MAX_RESULT_TEXT_BYTES + 1)),
                },
                "invalid read error response",
            ),
        ];
        for (reply, expected) in cases {
            let mut input = Vec::new();
            protocol::write_host_frame(
                &mut input,
                &HostFrame {
                    version: protocol::VERSION,
                    message: HostMessage::Init(init(script, FieldType::Text)),
                },
            )
            .expect("valid init frame encodes");
            protocol::write_host_frame(
                &mut input,
                &HostFrame {
                    version: protocol::VERSION,
                    message: HostMessage::ReadResult(reply),
                },
            )
            .expect("test read reply encodes");
            let mut output = Vec::new();
            run(&mut Cursor::new(input), &mut output).expect("worker writes terminal failure");
            let OutputState::Failed { reason } = terminal_result(&output).state else {
                panic!("malformed broker reply should fail");
            };
            assert!(
                reason.contains(expected),
                "expected {expected:?}, got {reason:?}"
            );
        }
    }

    #[test]
    fn malformed_init_and_noninteger_read_arguments_are_rejected_before_brokerage() {
        let script = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function() return {state="absent"} end } }"#;
        assert_eq!(
            result(&invoke(init(script, FieldType::Text), &[])).state,
            OutputState::Absent
        );

        let mut invalid_directory = init(script, FieldType::Text);
        invalid_directory.is_dir = true;
        assert!(matches!(
            result(&invoke(invalid_directory, &[])).state,
            OutputState::Failed { .. }
        ));

        let mut invalid_package = init(script, FieldType::Text);
        invalid_package.package = "other".into();
        assert!(matches!(
            result(&invoke(invalid_package, &[])).state,
            OutputState::Failed { .. }
        ));

        let alternate_contribution = "extension.sensor.other.distance-cell";
        let mut invalid_contribution = init(script, FieldType::Text);
        invalid_contribution.contribution = alternate_contribution.into();
        invalid_contribution.script = format!(
            r#"return {{ ["{alternate_contribution}"] = {{
                read=function() return {{state="absent"}} end }} }}"#
        );
        assert!(matches!(
            result(&invoke(invalid_contribution, &[])).state,
            OutputState::Failed { .. }
        ));

        let mut invalid_demand = init(script, FieldType::Text);
        invalid_demand.demand.max_bytes = crate::extensions::manifest::MAX_DEMAND_BYTES + 1;
        assert!(matches!(
            result(&invoke(invalid_demand, &[])).state,
            OutputState::Failed { .. }
        ));

        let script = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function(ctx)
                fyle.read(ctx.source, "0", 1)
                return {state="absent"}
            end } }"#;
        let output = invoke(init(script, FieldType::Text), &[]);
        assert!(matches!(result(&output).state, OutputState::Failed { .. }));
        let mut output_reader = Cursor::new(output.as_slice());
        protocol::read_worker_frame(&mut output_reader).unwrap();
        assert_eq!(output_reader.position(), output.len() as u64);
    }

    #[test]
    fn result_values_reject_wrong_lua_types_and_unknown_table_shape() {
        let cases = [
            (
                FieldType::Quantity(Unit::Metres),
                r#"{state="ready", field="extension.sensor.reader.distance",
                   value={kind="quantity", value="1", unit="metres"},
                   completeness={kind="complete"}}"#,
                "quantity must be a number",
            ),
            (
                FieldType::Text,
                r#"{state="ready", field="extension.sensor.reader.distance",
                   value={kind="text", value=123}, completeness={kind="complete"}}"#,
                "result text must be a string",
            ),
            (
                FieldType::Text,
                r#"setmetatable({state="absent"}, {})"#,
                "metatables",
            ),
            (
                FieldType::Text,
                r#"{state="ready", field="extension.sensor.reader.distance",
                   value={kind="text", value="x"},
                   completeness={kind="complete"}, reason="unexpected"}"#,
                "unknown field",
            ),
        ];
        for (field_type, returned, expected) in cases {
            let script = format!(
                r#"return {{ ["extension.sensor.reader.distance-cell"] = {{
                    read=function() return {returned} end }} }}"#
            );
            let OutputState::Failed { reason } =
                result(&invoke(init(&script, field_type), &[])).state
            else {
                panic!("malformed result should fail: {returned}");
            };
            assert!(
                reason.contains(expected),
                "expected {expected:?}, got {reason:?}"
            );
        }
    }
}
