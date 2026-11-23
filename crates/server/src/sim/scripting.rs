use std::cell::RefCell;
use std::rc::Rc;

use boa_engine::object::{ObjectInitializer, builtins::JsArray};
use boa_engine::prelude::{
    Context, Finalize, JsData, JsObject, JsString, JsValue, NativeFunction, Source, Trace,
};
use boa_engine::property::Attribute;
use boa_engine::{JsArgs, JsNativeError, JsResult, js_string};
use shared::entity::{EntityId, InventoryEntry, PropertyBag, PropertyValue};
use shared::protocol::PopupTone;
use tracing::info;

use super::glue::{SimCommand, TriggerWay};

const MAX_LOOP_ITERATIONS: u64 = 10_000;
const MAX_RECURSION_DEPTH: usize = 64;
const MAX_STACK_SIZE: usize = 2048;
const MAX_COMMANDS_PER_RUN: usize = 64;
const MAX_TEXT_CHARS: usize = 2048;

pub struct ScriptTriggerInput {
    pub trigger_entity_id: EntityId,
    pub trigger_entity_name: String,
    pub trigger_pos: [f32; 3],
    pub trigger_props: PropertyBag,
    pub trigger_inventory: Vec<InventoryEntry>,
    pub actor_entity_id: EntityId,
    pub actor_pos: [f32; 3],
    pub actor_props: PropertyBag,
    pub actor_inventory: Vec<InventoryEntry>,
    pub actor_is_player: bool,
    pub area_id: String,
    pub way: TriggerWay,
}

pub struct ScriptExecutionOutput {
    pub commands: Vec<SimCommand>,
    pub logs: Vec<String>,
}

struct ScriptRunState {
    trigger_entity_id: EntityId,
    trigger_props: PropertyBag,
    trigger_inventory: Vec<InventoryEntry>,
    actor_entity_id: EntityId,
    actor_props: PropertyBag,
    actor_inventory: Vec<InventoryEntry>,
    actor_is_player: bool,
    commands: Vec<SimCommand>,
    logs: Vec<String>,
}

impl ScriptRunState {
    fn push_command(&mut self, cmd: SimCommand) -> JsResult<()> {
        if self.commands.len() >= MAX_COMMANDS_PER_RUN {
            return Err(JsNativeError::range()
                .with_message("script exceeded maximum command quota per trigger")
                .into());
        }
        self.commands.push(cmd);
        Ok(())
    }

    fn props_mut(&mut self, is_entity: bool) -> &mut PropertyBag {
        if is_entity {
            &mut self.trigger_props
        } else {
            &mut self.actor_props
        }
    }

    fn props_ref(&self, is_entity: bool) -> &PropertyBag {
        if is_entity {
            &self.trigger_props
        } else {
            &self.actor_props
        }
    }

    fn target_id(&self, is_entity: bool) -> EntityId {
        if is_entity {
            self.trigger_entity_id
        } else {
            self.actor_entity_id
        }
    }

    fn inventory_ref(&self, is_entity: bool) -> &[InventoryEntry] {
        if is_entity {
            &self.trigger_inventory
        } else {
            &self.actor_inventory
        }
    }
}

fn check_has_item(
    args: &[JsValue],
    state: &SharedScriptState,
    is_entity: bool,
    context: &mut Context,
) -> JsResult<JsValue> {
    let item_id = arg_string(args.get_or_undefined(0), context)?;
    let min_val = args.get_or_undefined(1);
    let min_count = if min_val.is_null_or_undefined() {
        1.0
    } else {
        let n = min_val.to_number(context)?;
        if !n.is_finite() {
            return Err(JsNativeError::range()
                .with_message("hasItem minCount must be finite")
                .into());
        }
        n
    };
    if min_count <= 0.0 {
        return Ok(JsValue::from(false));
    }
    let st = state.inner.borrow();
    let total: f64 = st
        .inventory_ref(is_entity)
        .iter()
        .filter(|e| e.item_id == item_id && e.count > 0)
        .map(|e| e.count as f64)
        .sum();
    Ok(JsValue::from(total >= min_count))
}

fn attach_inventory_methods(
    init: &mut ObjectInitializer<'_>,
    state: &SharedScriptState,
    is_entity: bool,
) {
    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| check_has_item(args, shared, is_entity, context),
            state.clone(),
        ),
        js_string!("hasItem"),
        2,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let item_id = arg_string(args.get_or_undefined(0), context)?;
                let st = shared.inner.borrow();
                let total: u64 = st
                    .inventory_ref(is_entity)
                    .iter()
                    .filter(|e| e.item_id == item_id && e.count > 0)
                    .map(|e| e.count as u64)
                    .sum();
                Ok(JsValue::from(total as f64))
            },
            state.clone(),
        ),
        js_string!("itemCount"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let item_id = arg_string(args.get_or_undefined(0), context)?;
                let st = shared.inner.borrow();
                let equipped = st
                    .inventory_ref(is_entity)
                    .iter()
                    .any(|e| e.item_id == item_id && e.count > 0 && e.equipped);
                Ok(JsValue::from(equipped))
            },
            state.clone(),
        ),
        js_string!("isEquipped"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, _args, shared, context| {
                let entries = shared.inner.borrow().inventory_ref(is_entity).to_vec();
                let objects: Vec<JsValue> = entries
                    .into_iter()
                    .filter(|e| e.count > 0)
                    .map(|entry| {
                        let mut obj = ObjectInitializer::new(context);
                        obj.property(
                            js_string!("itemId"),
                            JsValue::from(JsString::from(entry.item_id.as_str())),
                            Attribute::READONLY | Attribute::ENUMERABLE,
                        );
                        obj.property(
                            js_string!("count"),
                            JsValue::from(entry.count as f64),
                            Attribute::READONLY | Attribute::ENUMERABLE,
                        );
                        obj.property(
                            js_string!("equipped"),
                            JsValue::from(entry.equipped),
                            Attribute::READONLY | Attribute::ENUMERABLE,
                        );
                        JsValue::from(obj.build())
                    })
                    .collect();
                Ok(JsArray::from_iter(objects, context).into())
            },
            state.clone(),
        ),
        js_string!("items"),
        0,
    );
}

fn push_teleport_to_area_target(
    args: &[JsValue],
    shared: &SharedScriptState,
    context: &mut Context,
) -> JsResult<JsValue> {
    let area_id = arg_string(args.get_or_undefined(0), context)?;
    if !is_valid_script_area_id(&area_id) {
        return Err(JsNativeError::range()
            .with_message("teleportToArea requires a valid area id [a-zA-Z0-9_-]")
            .into());
    }
    let raw_target = arg_string(args.get_or_undefined(1), context)?;
    let target_name = clamp_text(raw_target.trim());
    if target_name.is_empty() {
        return Err(JsNativeError::range()
            .with_message("teleportToArea requires a non-empty target entity name")
            .into());
    }
    let mut st = shared.inner.borrow_mut();
    let entity_id = st.actor_entity_id;
    st.push_command(SimCommand::TeleportToAreaTarget {
        entity_id,
        area_id,
        target_name,
    })?;
    Ok(JsValue::undefined())
}

#[derive(Clone, Trace, Finalize, JsData)]
struct SharedScriptState {
    #[unsafe_ignore_trace]
    inner: Rc<RefCell<ScriptRunState>>,
}

fn clamp_text(raw: &str) -> String {
    raw.chars().take(MAX_TEXT_CHARS).collect()
}

fn is_valid_script_area_id(area_id: &str) -> bool {
    !area_id.is_empty()
        && area_id.len() <= 64
        && area_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn arg_string(val: &JsValue, context: &mut Context) -> JsResult<String> {
    Ok(val.to_string(context)?.to_std_string_escaped())
}

fn opt_arg_string(args: &[JsValue], idx: usize, context: &mut Context) -> JsResult<Option<String>> {
    let val = args.get_or_undefined(idx);
    if val.is_null_or_undefined() {
        return Ok(None);
    }
    Ok(Some(val.to_string(context)?.to_std_string_escaped()))
}

fn parse_popup_tone(raw: Option<&str>) -> PopupTone {
    match raw.unwrap_or("info").trim().to_ascii_lowercase().as_str() {
        "success" | "mint" => PopupTone::Success,
        "warning" | "warn" | "amber" => PopupTone::Warning,
        "danger" | "error" | "crimson" => PopupTone::Danger,
        _ => PopupTone::Info,
    }
}

fn prop_value_to_js(val: Option<&PropertyValue>) -> JsValue {
    match val {
        None => JsValue::null(),
        Some(PropertyValue::Bool(b)) => JsValue::from(*b),
        Some(PropertyValue::Int(i)) => JsValue::from(*i as f64),
        Some(PropertyValue::Float(f)) => JsValue::from(*f as f64),
        Some(PropertyValue::String(s)) => JsValue::from(JsString::from(s.as_str())),
    }
}

fn js_to_prop_value(val: &JsValue, context: &mut Context) -> JsResult<Option<PropertyValue>> {
    if val.is_null_or_undefined() {
        return Ok(None);
    }
    if let Some(b) = val.as_boolean() {
        return Ok(Some(PropertyValue::Bool(b)));
    }
    if val.is_number() {
        let n = val.to_number(context)?;
        if !n.is_finite() {
            return Err(JsNativeError::range()
                .with_message("property number must be finite")
                .into());
        }
        if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
            return Ok(Some(PropertyValue::Int(n as i64)));
        }
        return Ok(Some(PropertyValue::Float(n as f32)));
    }
    Ok(Some(PropertyValue::String(clamp_text(
        &val.to_string(context)?.to_std_string_escaped(),
    ))))
}

fn pos_array(pos: [f32; 3], context: &mut Context) -> JsValue {
    JsArray::from_iter(
        [
            JsValue::from(pos[0] as f64),
            JsValue::from(pos[1] as f64),
            JsValue::from(pos[2] as f64),
        ],
        context,
    )
    .into()
}

fn attach_prop_methods(
    init: &mut ObjectInitializer<'_>,
    state: &SharedScriptState,
    is_entity: bool,
) {
    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let key = arg_string(args.get_or_undefined(0), context)?;
                let st = shared.inner.borrow();
                Ok(prop_value_to_js(st.props_ref(is_entity).get(&key)))
            },
            state.clone(),
        ),
        js_string!("getProp"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let key = arg_string(args.get_or_undefined(0), context)?;
                let st = shared.inner.borrow();
                Ok(match st.props_ref(is_entity).get_bool(&key) {
                    Some(b) => JsValue::from(b),
                    None => JsValue::null(),
                })
            },
            state.clone(),
        ),
        js_string!("getBool"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let key = arg_string(args.get_or_undefined(0), context)?;
                let st = shared.inner.borrow();
                Ok(match st.props_ref(is_entity).get(&key) {
                    Some(PropertyValue::Int(i)) => JsValue::from(*i as f64),
                    Some(PropertyValue::Float(f)) => JsValue::from((*f as f64).trunc()),
                    _ => JsValue::null(),
                })
            },
            state.clone(),
        ),
        js_string!("getInt"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let key = arg_string(args.get_or_undefined(0), context)?;
                let st = shared.inner.borrow();
                Ok(match st.props_ref(is_entity).get(&key) {
                    Some(PropertyValue::Float(f)) => JsValue::from(*f as f64),
                    Some(PropertyValue::Int(i)) => JsValue::from(*i as f64),
                    _ => JsValue::null(),
                })
            },
            state.clone(),
        ),
        js_string!("getFloat"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let key = arg_string(args.get_or_undefined(0), context)?;
                let st = shared.inner.borrow();
                Ok(match st.props_ref(is_entity).get_str(&key) {
                    Some(s) => JsValue::from(JsString::from(s)),
                    None => JsValue::null(),
                })
            },
            state.clone(),
        ),
        js_string!("getString"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let key = clamp_text(&arg_string(args.get_or_undefined(0), context)?);
                let parsed = js_to_prop_value(args.get_or_undefined(1), context)?;
                let mut st = shared.inner.borrow_mut();
                let target_id = st.target_id(is_entity);
                match parsed {
                    Some(value) => {
                        st.props_mut(is_entity).set(key.clone(), value.clone());
                        st.push_command(SimCommand::SetProperty {
                            entity_id: target_id,
                            key,
                            value,
                        })?;
                    }
                    None => {
                        st.props_mut(is_entity).remove(&key);
                        st.push_command(SimCommand::RemoveProperty {
                            entity_id: target_id,
                            key,
                        })?;
                    }
                }
                Ok(JsValue::undefined())
            },
            state.clone(),
        ),
        js_string!("setProp"),
        2,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let key = clamp_text(&arg_string(args.get_or_undefined(0), context)?);
                let n = args.get_or_undefined(1).to_number(context)?;
                if !n.is_finite() {
                    return Err(JsNativeError::range()
                        .with_message("setInt value must be finite")
                        .into());
                }
                let val = PropertyValue::Int(n.round() as i64);
                let mut st = shared.inner.borrow_mut();
                let target_id = st.target_id(is_entity);
                st.props_mut(is_entity).set(key.clone(), val.clone());
                st.push_command(SimCommand::SetProperty {
                    entity_id: target_id,
                    key,
                    value: val,
                })?;
                Ok(JsValue::undefined())
            },
            state.clone(),
        ),
        js_string!("setInt"),
        2,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let key = clamp_text(&arg_string(args.get_or_undefined(0), context)?);
                let n = args.get_or_undefined(1).to_number(context)?;
                if !n.is_finite() {
                    return Err(JsNativeError::range()
                        .with_message("setFloat value must be finite")
                        .into());
                }
                let val = PropertyValue::Float(n as f32);
                let mut st = shared.inner.borrow_mut();
                let target_id = st.target_id(is_entity);
                st.props_mut(is_entity).set(key.clone(), val.clone());
                st.push_command(SimCommand::SetProperty {
                    entity_id: target_id,
                    key,
                    value: val,
                })?;
                Ok(JsValue::undefined())
            },
            state.clone(),
        ),
        js_string!("setFloat"),
        2,
    );
}

fn build_entity_object(
    context: &mut Context,
    input: &ScriptTriggerInput,
    state: &SharedScriptState,
) -> JsObject {
    let pos = pos_array(input.trigger_pos, context);
    let mut init = ObjectInitializer::new(context);
    init.property(
        js_string!("id"),
        JsValue::from(input.trigger_entity_id.raw() as f64),
        Attribute::READONLY | Attribute::ENUMERABLE,
    );
    init.property(
        js_string!("name"),
        JsValue::from(JsString::from(input.trigger_entity_name.as_str())),
        Attribute::READONLY | Attribute::ENUMERABLE,
    );
    init.property(
        js_string!("pos"),
        pos,
        Attribute::READONLY | Attribute::ENUMERABLE,
    );
    attach_prop_methods(&mut init, state, true);
    attach_inventory_methods(&mut init, state, true);
    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, _args, shared, _ctx| {
                let mut st = shared.inner.borrow_mut();
                let id = st.trigger_entity_id;
                st.push_command(SimCommand::Despawn { entity_id: id })?;
                Ok(JsValue::undefined())
            },
            state.clone(),
        ),
        js_string!("despawn"),
        0,
    );
    init.build()
}

fn build_player_object(
    context: &mut Context,
    input: &ScriptTriggerInput,
    state: &SharedScriptState,
) -> JsObject {
    let pos = pos_array(input.actor_pos, context);
    let mut init = ObjectInitializer::new(context);
    init.property(
        js_string!("id"),
        JsValue::from(input.actor_entity_id.raw() as f64),
        Attribute::READONLY | Attribute::ENUMERABLE,
    );
    init.property(
        js_string!("pos"),
        pos,
        Attribute::READONLY | Attribute::ENUMERABLE,
    );
    attach_prop_methods(&mut init, state, false);
    attach_inventory_methods(&mut init, state, false);

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let quest_id = arg_string(args.get_or_undefined(0), context)?;
                let key = format!("quest:{quest_id}");
                let st = shared.inner.borrow();
                let status = st.actor_props.get_str(&key).unwrap_or("none");
                Ok(JsValue::from(JsString::from(status)))
            },
            state.clone(),
        ),
        js_string!("questStatus"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let quest_id = arg_string(args.get_or_undefined(0), context)?;
                let key = format!("quest:{quest_id}");
                let st = shared.inner.borrow();
                let status = st.actor_props.get_str(&key).unwrap_or("none");
                Ok(JsValue::from(status != "none"))
            },
            state.clone(),
        ),
        js_string!("hasQuest"),
        1,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let quest_id = clamp_text(&arg_string(args.get_or_undefined(0), context)?);
                if quest_id.is_empty() {
                    return Ok(JsValue::from(false));
                }
                let title = opt_arg_string(args, 1, context)?;
                let key = format!("quest:{quest_id}");
                let mut st = shared.inner.borrow_mut();
                let current = st.actor_props.get_str(&key).unwrap_or("none");
                if current != "none" {
                    return Ok(JsValue::from(false));
                }
                let actor_id = st.actor_entity_id;
                let active = PropertyValue::String("active".to_owned());
                st.actor_props.set(key.clone(), active.clone());
                st.push_command(SimCommand::SetProperty {
                    entity_id: actor_id,
                    key,
                    value: active,
                })?;
                if let Some(title_text) = title {
                    let tkey = format!("quest_title:{quest_id}");
                    let tval = PropertyValue::String(clamp_text(&title_text));
                    st.actor_props.set(tkey.clone(), tval.clone());
                    st.push_command(SimCommand::SetProperty {
                        entity_id: actor_id,
                        key: tkey,
                        value: tval,
                    })?;
                }
                Ok(JsValue::from(true))
            },
            state.clone(),
        ),
        js_string!("giveQuest"),
        2,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let quest_id = clamp_text(&arg_string(args.get_or_undefined(0), context)?);
                if quest_id.is_empty() {
                    return Ok(JsValue::undefined());
                }
                let key = format!("quest:{quest_id}");
                let completed = PropertyValue::String("completed".to_owned());
                let mut st = shared.inner.borrow_mut();
                let actor_id = st.actor_entity_id;
                st.actor_props.set(key.clone(), completed.clone());
                st.push_command(SimCommand::SetProperty {
                    entity_id: actor_id,
                    key,
                    value: completed,
                })?;
                Ok(JsValue::undefined())
            },
            state.clone(),
        ),
        js_string!("completeQuest"),
        1,
    );

    let area_id = input.area_id.clone();
    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let x = args.get_or_undefined(0).to_number(context)?;
                let z = args.get_or_undefined(1).to_number(context)?;
                if !x.is_finite() || !z.is_finite() {
                    return Err(JsNativeError::range()
                        .with_message("teleport coordinates must be finite")
                        .into());
                }
                let yaw = {
                    let yaw_val = args.get_or_undefined(2);
                    if yaw_val.is_null_or_undefined() {
                        None
                    } else {
                        let deg = yaw_val.to_number(context)?;
                        if !deg.is_finite() {
                            return Err(JsNativeError::range()
                                .with_message("teleport yaw must be finite")
                                .into());
                        }
                        Some((deg as f32).to_radians())
                    }
                };
                let mut st = shared.inner.borrow_mut();
                let actor_id = st.actor_entity_id;
                st.push_command(SimCommand::TransferToArea {
                    entity_id: actor_id,
                    area_id: String::new(),
                    pos: [x as f32, 0.0, z as f32],
                    yaw,
                })?;
                Ok(JsValue::undefined())
            },
            state.clone(),
        ),
        js_string!("teleport"),
        3,
    );
    let _ = area_id;

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| {
                let area_id = arg_string(args.get_or_undefined(0), context)?;
                if !is_valid_script_area_id(&area_id) {
                    return Err(JsNativeError::range()
                        .with_message("moveToArea requires a valid area id [a-zA-Z0-9_-]")
                        .into());
                }
                let x = args.get_or_undefined(1).to_number(context)?;
                let z = args.get_or_undefined(2).to_number(context)?;
                if !x.is_finite() || !z.is_finite() {
                    return Err(JsNativeError::range()
                        .with_message("moveToArea coordinates must be finite")
                        .into());
                }
                let yaw = {
                    let yaw_val = args.get_or_undefined(3);
                    if yaw_val.is_null_or_undefined() {
                        None
                    } else {
                        let deg = yaw_val.to_number(context)?;
                        if !deg.is_finite() {
                            return Err(JsNativeError::range()
                                .with_message("moveToArea yaw must be finite")
                                .into());
                        }
                        Some((deg as f32).to_radians())
                    }
                };
                let mut st = shared.inner.borrow_mut();
                let actor_id = st.actor_entity_id;
                st.push_command(SimCommand::TransferToArea {
                    entity_id: actor_id,
                    area_id,
                    pos: [x as f32, 0.0, z as f32],
                    yaw,
                })?;
                Ok(JsValue::undefined())
            },
            state.clone(),
        ),
        js_string!("moveToArea"),
        4,
    );

    init.function(
        NativeFunction::from_copy_closure_with_captures(
            move |_this, args, shared, context| push_teleport_to_area_target(args, shared, context),
            state.clone(),
        ),
        js_string!("teleportToArea"),
        2,
    );

    init.build()
}

/// Evaluates `js_source` inside a bounded `boa_engine` sandbox and invokes `onTrigger(ctx)`.
pub fn execute_trigger_script(
    js_source: &str,
    input: ScriptTriggerInput,
) -> Result<ScriptExecutionOutput, String> {
    let mut context = Context::default();
    context
        .runtime_limits_mut()
        .set_loop_iteration_limit(MAX_LOOP_ITERATIONS);
    context
        .runtime_limits_mut()
        .set_recursion_limit(MAX_RECURSION_DEPTH);
    context
        .runtime_limits_mut()
        .set_stack_size_limit(MAX_STACK_SIZE);

    let way_str = match input.way {
        TriggerWay::Click => "click",
        TriggerWay::WalkInto => "walk_into",
    };
    let shared = SharedScriptState {
        inner: Rc::new(RefCell::new(ScriptRunState {
            trigger_entity_id: input.trigger_entity_id,
            trigger_props: input.trigger_props.clone(),
            trigger_inventory: input.trigger_inventory.clone(),
            actor_entity_id: input.actor_entity_id,
            actor_props: input.actor_props.clone(),
            actor_inventory: input.actor_inventory.clone(),
            actor_is_player: input.actor_is_player,
            commands: Vec::new(),
            logs: Vec::new(),
        })),
    };

    let entity_obj = build_entity_object(&mut context, &input, &shared);
    let player_obj = build_player_object(&mut context, &input, &shared);

    let ctx_obj = {
        let mut init = ObjectInitializer::new(&mut context);
        init.property(
            js_string!("way"),
            JsValue::from(JsString::from(way_str)),
            Attribute::READONLY | Attribute::ENUMERABLE,
        );
        init.property(
            js_string!("areaId"),
            JsValue::from(JsString::from(input.area_id.as_str())),
            Attribute::READONLY | Attribute::ENUMERABLE,
        );
        init.property(
            js_string!("entity"),
            JsValue::from(entity_obj),
            Attribute::READONLY | Attribute::ENUMERABLE,
        );
        init.property(
            js_string!("player"),
            JsValue::from(player_obj),
            Attribute::READONLY | Attribute::ENUMERABLE,
        );
        init.function(
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, shared, context| check_has_item(args, shared, false, context),
                shared.clone(),
            ),
            js_string!("hasItem"),
            2,
        );
        init.function(
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, shared, context| {
                    push_teleport_to_area_target(args, shared, context)
                },
                shared.clone(),
            ),
            js_string!("teleportToArea"),
            2,
        );
        init.function(
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, shared, context| {
                    let title = clamp_text(&arg_string(args.get_or_undefined(0), context)?);
                    let body = clamp_text(&arg_string(args.get_or_undefined(1), context)?);
                    let tone_arg = opt_arg_string(args, 2, context)?;
                    let tone = parse_popup_tone(tone_arg.as_deref());
                    let mut st = shared.inner.borrow_mut();
                    if st.actor_is_player {
                        let player_entity_id = st.actor_entity_id;
                        st.push_command(SimCommand::ShowPopup {
                            player_entity_id,
                            title,
                            body,
                            tone,
                        })?;
                    }
                    Ok(JsValue::undefined())
                },
                shared.clone(),
            ),
            js_string!("showPopup"),
            3,
        );
        init.function(
            NativeFunction::from_copy_closure_with_captures(
                move |_this, args, shared, context| {
                    let msg = clamp_text(&arg_string(args.get_or_undefined(0), context)?);
                    let mut st = shared.inner.borrow_mut();
                    info!(
                        "Entity script log [entity={}]: {}",
                        st.trigger_entity_id, msg
                    );
                    if st.logs.len() < MAX_COMMANDS_PER_RUN {
                        st.logs.push(msg);
                    }
                    Ok(JsValue::undefined())
                },
                shared.clone(),
            ),
            js_string!("log"),
            1,
        );
        init.build()
    };

    context
        .eval(Source::from_bytes(js_source))
        .map_err(|e| format!("script compile/eval error: {e}"))?;

    let handler_val = context
        .global_object()
        .get(js_string!("onTrigger"), &mut context)
        .map_err(|e| format!("failed to look up onTrigger handler: {e}"))?;
    let handler = handler_val.as_callable().ok_or_else(|| {
        "script must define a top-level function onTrigger(ctx)".to_owned()
    })?;

    handler
        .call(&JsValue::undefined(), &[ctx_obj.into()], &mut context)
        .map_err(|e| format!("script runtime error in onTrigger: {e}"))?;

    let st = shared.inner.borrow();
    Ok(ScriptExecutionOutput {
        commands: st.commands.clone(),
        logs: st.logs.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> ScriptTriggerInput {
        let mut trigger_props = PropertyBag::new();
        trigger_props.set("uses", PropertyValue::Int(2));
        trigger_props.set("locked", PropertyValue::Bool(false));
        ScriptTriggerInput {
            trigger_entity_id: EntityId::new(10),
            trigger_entity_name: "Cryo Terminal".to_owned(),
            trigger_pos: [4.0, 0.0, 8.0],
            trigger_props,
            trigger_inventory: Vec::new(),
            actor_entity_id: EntityId::new(1),
            actor_pos: [3.5, 0.0, 7.5],
            actor_props: PropertyBag::new(),
            actor_inventory: Vec::new(),
            actor_is_player: true,
            area_id: "cryo_prison".to_owned(),
            way: TriggerWay::Click,
        }
    }

    #[test]
    fn executes_trigger_script_with_props_quests_popups_and_transfers() {
        let js = r#"
            function onTrigger(ctx) {
                const uses = ctx.entity.getInt("uses") || 0;
                ctx.entity.setInt("uses", uses + 1);
                if (ctx.player.giveQuest("escape_cryo", "Escape Cryo Prison")) {
                    ctx.showPopup(ctx.entity.name, "Quest accepted on " + ctx.way, "success");
                }
                ctx.player.moveToArea("main", 12.5, -8.25, 90);
                ctx.log("uses now " + ctx.entity.getInt("uses"));
            }
        "#;

        let out = execute_trigger_script(js, sample_input()).expect("script succeeds");
        assert_eq!(out.logs, vec!["uses now 3".to_owned()]);
        assert!(out.commands.iter().any(|c| matches!(
            c,
            SimCommand::SetProperty {
                entity_id,
                key,
                value: PropertyValue::Int(3),
            } if *entity_id == EntityId::new(10) && key == "uses"
        )));
        assert!(out.commands.iter().any(|c| matches!(
            c,
            SimCommand::SetProperty {
                entity_id,
                key,
                value: PropertyValue::String(v),
            } if *entity_id == EntityId::new(1) && key == "quest:escape_cryo" && v == "active"
        )));
        assert!(out.commands.iter().any(|c| matches!(
            c,
            SimCommand::ShowPopup {
                player_entity_id,
                title,
                body,
                tone: PopupTone::Success,
            } if *player_entity_id == EntityId::new(1)
                && title == "Cryo Terminal"
                && body == "Quest accepted on click"
        )));
        assert!(out.commands.iter().any(|c| matches!(
            c,
            SimCommand::TransferToArea {
                entity_id,
                area_id,
                pos,
                yaw: Some(_),
            } if *entity_id == EntityId::new(1) && area_id == "main" && (pos[0] - 12.5).abs() < 1e-4
        )));
    }

    #[test]
    fn enforces_loop_iteration_limit_and_reports_runtime_errors() {
        let infinite_js = r#"
            function onTrigger(ctx) {
                while (true) {}
            }
        "#;
        let err = execute_trigger_script(infinite_js, sample_input())
            .err()
            .expect("infinite loop must terminate via runtime limit");
        assert!(
            err.contains("runtime error") || err.contains("Maximum") || err.contains("Limit"),
            "unexpected error message: {err}"
        );

        let missing_handler = "const x = 42;";
        let missing_err = execute_trigger_script(missing_handler, sample_input())
            .err()
            .expect("missing onTrigger must error");
        assert!(missing_err.contains("onTrigger"));
    }

    #[test]
    fn executes_inventory_checks_and_teleport_to_area_target() {
        let mut input = sample_input();
        input.actor_inventory = vec![
            InventoryEntry {
                item_id: "cryo_jumpsuit".to_owned(),
                count: 1,
                equipped: true,
            },
            InventoryEntry {
                item_id: "ration_pack".to_owned(),
                count: 3,
                equipped: false,
            },
        ];

        let js = r#"
            function onTrigger(ctx) {
                const items = ctx.player.items();
                ctx.log("len=" + items.length + ",count=" + ctx.player.itemCount("ration_pack"));
                if (
                    ctx.player.hasItem("cryo_jumpsuit") &&
                    ctx.player.isEquipped("cryo_jumpsuit") &&
                    ctx.hasItem("ration_pack", 2) &&
                    !ctx.player.hasItem("guard_rifle") &&
                    items[0].itemId === "cryo_jumpsuit" &&
                    items[0].equipped === true
                ) {
                    ctx.teleportToArea("main", "CryoElevatorExit");
                }
            }
        "#;

        let out = execute_trigger_script(js, input).expect("inventory + teleport script succeeds");
        assert_eq!(out.logs, vec!["len=2,count=3".to_owned()]);
        assert!(out.commands.iter().any(|c| matches!(
            c,
            SimCommand::TeleportToAreaTarget {
                entity_id,
                area_id,
                target_name,
            } if *entity_id == EntityId::new(1)
                && area_id == "main"
                && target_name == "CryoElevatorExit"
        )));
    }
}
