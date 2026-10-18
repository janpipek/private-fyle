//! Runs one validated extension column request inside a restricted Lua state.

use std::{
    io::{self, Read, Write},
    sync::{Arc, Mutex},
};

use mlua::{Function, HookTriggers, Lua, LuaOptions, LuaString, StdLib, Table, Value, VmState};

use super::protocol::{
    self, ContributionOutput, ContributionResult, HostMessage, Init, OutputState, ReadRequest,
    Request, VERSION, WorkerFrame, WorkerMessage,
};

mod convert;

const MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const INSTRUCTION_LIMIT: u64 = 5_000_000;
const HOOK_INTERVAL: u32 = 1_000;
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
        Err(error) => failed_output(&init.request, bounded_error(&error)),
    };
    protocol::write_worker_frame(
        writer,
        &WorkerFrame {
            version: VERSION,
            message: WorkerMessage::Result { output: result },
        },
    )?;
    writer.flush()
}

fn execute<R: Read, W: Write>(
    init: &Init,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<ContributionOutput> {
    let package = super::manifest::PackageId::new(init.package.clone())?;
    let provider = crate::data_providers::ProviderId::new(init.provider.clone())?;
    crate::data_providers::ContributionId::new(init.contribution.clone())?;
    let owner_prefix = format!("{}.", provider.as_str());
    let package_prefix = format!("extension.{}.", package.as_str());
    let valid_request = match &init.request {
        Request::Column { field, .. } => {
            crate::data_providers::FieldId::new(field.clone()).is_ok()
                && field.len() <= 256
                && field.starts_with(&owner_prefix)
        }
        Request::Properties { fields } => {
            !fields.is_empty()
                && fields.len() <= super::manifest::MAX_FIELDS_PER_PROVIDER
                && fields.iter().enumerate().all(|(index, field)| {
                    field.field.len() <= 256
                        && field.field.starts_with(&owner_prefix)
                        && crate::data_providers::FieldId::new(field.field.clone()).is_ok()
                        && fields[..index]
                            .iter()
                            .all(|earlier| earlier.field != field.field)
                })
        }
        Request::Viewer {
            representation,
            version,
        } => representation == "fyle.scene" && *version == 1,
    };
    if init.script.len() > super::manifest::SCRIPT_BYTES
        || init.name.len() > MAX_NAME_BYTES
        || init.provider.len() > 256
        || init.contribution.len() > 256
        || !init.provider.starts_with(&package_prefix)
        || !init.contribution.starts_with(&owner_prefix)
        || !valid_request
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
                .set_name(format!("extension.{}", init.package))
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
                        return Ok(unavailable_output(
                            &init.request,
                            "contribution does not apply".into(),
                        ));
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
            convert::convert_result(&value, init).map_err(mlua::Error::external)
        })
        .map_err(lua_error)?;
    Ok(result)
}

fn failed_output(request: &Request, reason: String) -> ContributionOutput {
    match request {
        Request::Column { .. } => ContributionOutput::Column(ContributionResult {
            field: None,
            state: OutputState::Failed { reason },
        }),
        Request::Properties { .. } => {
            ContributionOutput::Properties(super::protocol::PropertiesOutput::Failed { reason })
        }
        Request::Viewer { .. } => {
            ContributionOutput::Viewer(super::protocol::ViewerOutput::Failed { reason })
        }
    }
}

fn unavailable_output(request: &Request, reason: String) -> ContributionOutput {
    match request {
        Request::Column { .. } => ContributionOutput::Column(ContributionResult {
            field: None,
            state: OutputState::Unavailable { reason },
        }),
        Request::Properties { .. } => {
            ContributionOutput::Properties(super::protocol::PropertiesOutput::Unavailable {
                reason,
            })
        }
        Request::Viewer { .. } => {
            ContributionOutput::Viewer(super::protocol::ViewerOutput::Unavailable { reason })
        }
    }
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
        self, Demand, FieldType, HostFrame, HostMessage, Init, OutputState, ReadRequest, Request,
        Unit, WorkerMessage,
    };

    fn init(script: &str, field_type: FieldType) -> Init {
        Init {
            package: "sensor".into(),
            provider: "extension.sensor.reader".into(),
            contribution: "extension.sensor.reader.distance-cell".into(),
            request: Request::Column {
                field: "extension.sensor.reader.distance".into(),
                field_type,
            },
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
            WorkerMessage::Result {
                output: protocol::ContributionOutput::Column(result),
            } => result,
            WorkerMessage::Result { .. } => panic!("unexpected non-column result"),
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
                WorkerMessage::Result {
                    output: protocol::ContributionOutput::Column(result),
                } => return result,
                WorkerMessage::Result { .. } => panic!("unexpected non-column result"),
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
    fn lua_diagnostics_name_the_package_without_exposing_worker_source_path() {
        let script = r#"return { ["extension.sensor.reader.distance-cell"] = {
            read=function() error("fixture failure") end } }"#;
        let OutputState::Failed { reason } =
            result(&invoke(init(script, FieldType::Text), &[])).state
        else {
            panic!("reader error should be reported");
        };
        assert!(reason.contains("extension.sensor"), "{reason}");
        assert!(
            !reason.contains("src/extensions/worker.rs"),
            "diagnostic exposed Rust source path: {reason}"
        );
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
        let WorkerMessage::Result {
            output: protocol::ContributionOutput::Column(terminal),
        } = terminal
        else {
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

        let mut duplicate_fields = init(script, FieldType::Text);
        duplicate_fields.contribution = "extension.sensor.reader.summary".into();
        let duplicate_field = protocol::FieldSchema {
            field: "extension.sensor.reader.distance".into(),
            field_type: FieldType::Text,
        };
        duplicate_fields.request = Request::Properties {
            fields: vec![duplicate_field.clone(), duplicate_field],
        };
        assert!(matches!(
            protocol::read_worker_frame(&mut Cursor::new(invoke(duplicate_fields, &[])))
                .expect("invalid Properties init receives a terminal frame")
                .message,
            WorkerMessage::Result {
                output: protocol::ContributionOutput::Properties(
                    protocol::PropertiesOutput::Failed { .. }
                )
            }
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
                "field must be a number",
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

    #[test]
    fn scalar_text_accepts_the_full_documented_boundary() {
        let text = "x".repeat(protocol::MAX_RESULT_TEXT_BYTES);
        let script = format!(
            "return {{ ['extension.sensor.reader.distance-cell'] = {{ read=function() return {{state='ready',field='extension.sensor.reader.distance',value={{kind='text',value='{}'}},completeness={{kind='complete'}} }} end }} }}",
            text
        );
        let OutputState::Ready {
            value: protocol::OutputValue::Text { value },
            ..
        } = result(&invoke(init(&script, FieldType::Text), &[])).state
        else {
            panic!("exactly 64 KiB scalar text must remain valid");
        };
        assert_eq!(value.len(), protocol::MAX_RESULT_TEXT_BYTES);
    }

    #[test]
    fn properties_bad_quantity_isolated_from_valid_sibling() {
        let mut request = init("", FieldType::Text);
        request.request = Request::Properties {
            fields: vec![
                protocol::FieldSchema {
                    field: "extension.sensor.reader.distance".into(),
                    field_type: FieldType::Quantity(Unit::Metres),
                },
                protocol::FieldSchema {
                    field: "extension.sensor.reader.duration".into(),
                    field_type: FieldType::Quantity(Unit::Seconds),
                },
            ],
        };
        request.contribution = "extension.sensor.reader.summary".into();
        request.script = r#"return { ["extension.sensor.reader.summary"] = { read=function()
            return {state="ready", fields={
                {field="extension.sensor.reader.distance", state="ready",
                 value={kind="quantity", value=-1, unit="metres"}, completeness={kind="complete"}},
                {field="extension.sensor.reader.duration", state="ready",
                 value={kind="quantity", value=30, unit="seconds"}, completeness={kind="complete"}}}}
        end } }"#.into();
        let WorkerMessage::Result {
            output:
                protocol::ContributionOutput::Properties(protocol::PropertiesOutput::Ready { fields }),
        } = protocol::read_worker_frame(&mut Cursor::new(invoke(request, &[])))
            .expect("worker frame decodes")
            .message
        else {
            panic!("expected independent Properties field outcomes");
        };
        assert!(matches!(
            fields[0].state,
            protocol::FieldOutputState::Failed { .. }
        ));
        assert!(matches!(
            fields[1].state,
            protocol::FieldOutputState::Ready { .. }
        ));
    }

    #[test]
    fn properties_aggregate_text_overflow_fails_the_contribution() {
        let first = "a".repeat(40 * 1024);
        let second = "b".repeat(30 * 1024);
        let mut request = init("", FieldType::Text);
        request.request = Request::Properties {
            fields: vec![
                protocol::FieldSchema {
                    field: "extension.sensor.reader.first".into(),
                    field_type: FieldType::Text,
                },
                protocol::FieldSchema {
                    field: "extension.sensor.reader.second".into(),
                    field_type: FieldType::Text,
                },
            ],
        };
        request.contribution = "extension.sensor.reader.summary".into();
        request.script = format!(
            r#"return {{ ["extension.sensor.reader.summary"] = {{ read=function()
                return {{state="ready",fields={{
                    {{field="extension.sensor.reader.first",state="ready",value={{kind="text",value="{first}"}},completeness={{kind="complete"}}}},
                    {{field="extension.sensor.reader.second",state="ready",value={{kind="text",value="{second}"}},completeness={{kind="complete"}}}}
                }}}}
            end }} }}"#
        );
        let WorkerMessage::Result {
            output:
                protocol::ContributionOutput::Properties(protocol::PropertiesOutput::Failed { reason }),
        } = protocol::read_worker_frame(&mut Cursor::new(invoke(request, &[])))
            .expect("worker frame decodes")
            .message
        else {
            panic!("aggregate text overflow must fail the whole contribution");
        };
        assert!(reason.contains("aggregate text limit"));
    }

    #[test]
    fn viewer_accepts_false_blank_cells_and_rejects_sparse_or_invalid_scenes() {
        let mut request = init("", FieldType::Text);
        request.contribution = "extension.sensor.reader.graph".into();
        request.request = Request::Viewer {
            representation: "fyle.scene".into(),
            version: 1,
        };
        let valid = r#"return { ["extension.sensor.reader.graph"] = { read=function()
            return {state="ready",completeness={kind="complete"},scene={version=1,root={
                kind="table",id="samples",columns={{id="time",label="Time",kind={kind="text"}}},rows={{false}}
            }}}
        end } }"#;
        let WorkerMessage::Result {
            output:
                protocol::ContributionOutput::Viewer(protocol::ViewerOutput::Ready { scene, .. }),
        } = protocol::read_worker_frame(&mut Cursor::new(invoke(
            Init {
                script: valid.into(),
                ..request.clone()
            },
            &[],
        )))
        .expect("viewer frame decodes")
        .message
        else {
            panic!("false is an explicit blank table-cell sentinel");
        };
        let crate::preview_scene::NodeDto::Table { rows, .. } = scene.root else {
            panic!("expected a table scene");
        };
        assert_eq!(rows, vec![vec![None]]);

        let invalid_scenes = [
            r#"{version=1,root={kind="table",id="samples",columns={{id="time",label="Time",kind={kind="text"}}},rows={{[1]=false,[3]=false}}}}"#,
            r#"{version=1,root={kind="canvas",id="plot",width=-1,height=10,paths={}}}"#,
        ];
        for scene in invalid_scenes {
            let script = format!(
                r#"return {{ ["extension.sensor.reader.graph"] = {{ read=function() return {{state="ready",completeness={{kind="complete"}},scene={scene}}} end }} }}"#
            );
            let WorkerMessage::Result {
                output:
                    protocol::ContributionOutput::Viewer(protocol::ViewerOutput::Failed { reason }),
            } = protocol::read_worker_frame(&mut Cursor::new(invoke(
                Init {
                    script,
                    ..request.clone()
                },
                &[],
            )))
            .expect("viewer failure frame decodes")
            .message
            else {
                panic!("invalid scene structure must fail before host delivery");
            };
            if scene.contains("rows=") {
                assert!(
                    reason.contains("sparse") || reason.contains("extra keys"),
                    "sparse row should fail for its shape, got {reason:?}"
                );
            }
        }

        let mut deep_root = r#"{kind="text",id="leaf",text="x",role="body"}"#.to_owned();
        for level in 0..crate::preview_scene::MAX_DEPTH {
            deep_root = format!(
                "{{kind='stack',id='n{level}',axis='row',gap='small',children={{{deep_root}}}}}"
            );
        }
        let script = format!(
            r#"return {{ ["extension.sensor.reader.graph"] = {{ read=function() return {{state="ready",completeness={{kind="complete"}},scene={{version=1,root={deep_root}}}}} end }} }}"#
        );
        assert!(matches!(
            protocol::read_worker_frame(&mut Cursor::new(invoke(Init { script, ..request }, &[],)))
                .expect("depth failure frame decodes")
                .message,
            WorkerMessage::Result {
                output: protocol::ContributionOutput::Viewer(protocol::ViewerOutput::Failed { .. })
            }
        ));
    }
}
