use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::JsValue;
use web_sys::{Element, HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};

use crate::engine::EngineInner;

fn is_typing_in_form(target: Option<web_sys::EventTarget>) -> bool {
    let Some(t) = target else {
        return false;
    };
    let Ok(el) = t.dyn_into::<Element>() else {
        return false;
    };
    let tag = el.tag_name();
    match tag.as_str() {
        "TEXTAREA" | "SELECT" => true,
        "INPUT" => {
            let input_type = el
                .get_attribute("type")
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            !matches!(
                input_type.as_str(),
                "range" | "checkbox" | "radio" | "button"
            )
        }
        _ => false,
    }
}

pub(crate) fn attach_input_listeners(
    window: &web_sys::Window,
    canvas: &HtmlCanvasElement,
    inner: &Rc<RefCell<EngineInner>>,
) -> Result<Vec<Box<dyn std::any::Any>>, JsValue> {
    let mut kept: Vec<Box<dyn std::any::Any>> = Vec::new();

    // Prevent browser auto-scroll on middle-click and context menu on right-click
    let on_context = Closure::<dyn FnMut(MouseEvent)>::new(|ev: MouseEvent| {
        ev.prevent_default();
    });
    canvas.add_event_listener_with_callback("contextmenu", on_context.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_context));

    // Mouse down on canvas
    let st_down = Rc::clone(inner);
    let c_down = canvas.clone();
    let on_mousedown = Closure::<dyn FnMut(MouseEvent)>::new(move |ev: MouseEvent| {
        if ev.button() == 1 {
            ev.prevent_default();
        }
        let rect = c_down.get_bounding_client_rect();
        let mut st = st_down.borrow_mut();
        st.input.mouse_x = (ev.client_x() as f64 - rect.left()) as f32;
        st.input.mouse_y = (ev.client_y() as f64 - rect.top()) as f32;
        match ev.button() {
            0 => {
                st.input.left_mouse_down = true;
                st.input.left_just_pressed = true;
            }
            1 => st.input.middle_mouse_down = true,
            2 => st.input.right_just_pressed = true,
            _ => {}
        }
    });
    canvas.add_event_listener_with_callback("mousedown", on_mousedown.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_mousedown));

    // Mouse up on window so releasing outside canvas doesn't stick
    let st_up = Rc::clone(inner);
    let on_mouseup = Closure::<dyn FnMut(MouseEvent)>::new(move |ev: MouseEvent| {
        let mut st = st_up.borrow_mut();
        match ev.button() {
            0 => {
                if st.input.left_mouse_down {
                    st.input.left_just_released = true;
                }
                st.input.left_mouse_down = false;
            }
            1 => st.input.middle_mouse_down = false,
            _ => {}
        }
    });
    window.add_event_listener_with_callback("mouseup", on_mouseup.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_mouseup));

    // Mouse move on window
    let st_move = Rc::clone(inner);
    let c_move = canvas.clone();
    let on_mousemove = Closure::<dyn FnMut(MouseEvent)>::new(move |ev: MouseEvent| {
        let rect = c_move.get_bounding_client_rect();
        let nx = (ev.client_x() as f64 - rect.left()) as f32;
        let ny = (ev.client_y() as f64 - rect.top()) as f32;
        let mut st = st_move.borrow_mut();
        let dx = nx - st.input.mouse_x;
        let dy = ny - st.input.mouse_y;
        st.input.mouse_x = nx;
        st.input.mouse_y = ny;
        st.input.mouse_delta.0 += dx;
        st.input.mouse_delta.1 += dy;
    });
    window.add_event_listener_with_callback("mousemove", on_mousemove.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_mousemove));

    // Wheel on canvas
    let st_wheel = Rc::clone(inner);
    let on_wheel = Closure::<dyn FnMut(WheelEvent)>::new(move |ev: WheelEvent| {
        ev.prevent_default();
        let delta = (ev.delta_y() as f32 / 100.0).clamp(-3.0, 3.0);
        st_wheel.borrow_mut().input.scroll_delta += delta;
    });
    canvas.add_event_listener_with_callback("wheel", on_wheel.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_wheel));

    // Key down on window
    let st_kd = Rc::clone(inner);
    let on_keydown = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
        if ev.ctrl_key() || ev.meta_key() || ev.alt_key() || is_typing_in_form(ev.target()) {
            return;
        }
        let code = ev.code();
        if matches!(
            code.as_str(),
            "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight"
        ) {
            ev.prevent_default();
        }
        let mut st = st_kd.borrow_mut();
        if !ev.repeat() {
            st.input.just_pressed_keys.insert(code.clone());
        }
        st.input.keys.insert(code);
    });
    window.add_event_listener_with_callback("keydown", on_keydown.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_keydown));

    // Key up on window
    let st_ku = Rc::clone(inner);
    let on_keyup = Closure::<dyn FnMut(KeyboardEvent)>::new(move |ev: KeyboardEvent| {
        st_ku.borrow_mut().input.keys.remove(&ev.code());
    });
    window.add_event_listener_with_callback("keyup", on_keyup.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_keyup));

    // Blur clears stuck keys
    let st_blur = Rc::clone(inner);
    let on_blur = Closure::<dyn FnMut()>::new(move || {
        let mut st = st_blur.borrow_mut();
        st.input.keys.clear();
        st.input.just_pressed_keys.clear();
        st.input.left_mouse_down = false;
        st.input.right_just_pressed = false;
        st.input.middle_mouse_down = false;
    });
    window.add_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref())?;
    kept.push(Box::new(on_blur));

    Ok(kept)
}
