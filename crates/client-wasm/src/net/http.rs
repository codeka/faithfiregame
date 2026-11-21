use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestCache, RequestInit, RequestMode, Response};

pub async fn http_get_bytes(url: &str) -> Result<(u16, Vec<u8>), String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_owned())?;
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::SameOrigin);
    opts.set_cache(RequestCache::NoCache);
    let req = Request::new_with_str_and_init(url, &opts).map_err(|e| format!("{e:?}"))?;
    let resp_val = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: Response = resp_val.dyn_into().map_err(|e| format!("{e:?}"))?;
    let status = resp.status();
    if status != 200 {
        return Ok((status, Vec::new()));
    }
    let buf_val = JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let arr = js_sys::Uint8Array::new(&buf_val);
    Ok((status, arr.to_vec()))
}

#[cfg(feature = "editor")]
pub async fn http_put_bytes(url: &str, bytes: &[u8]) -> Result<u16, String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_owned())?;
    let opts = RequestInit::new();
    opts.set_method("PUT");
    opts.set_mode(RequestMode::SameOrigin);
    let uint8 = js_sys::Uint8Array::from(bytes);
    opts.set_body(&uint8.into());
    let req = Request::new_with_str_and_init(url, &opts).map_err(|e| format!("{e:?}"))?;
    let resp_val = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: Response = resp_val.dyn_into().map_err(|e| format!("{e:?}"))?;
    Ok(resp.status())
}

#[cfg(feature = "editor")]
pub async fn http_put_json_bytes(url: &str, bytes: &[u8]) -> Result<u16, String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_owned())?;
    let opts = RequestInit::new();
    opts.set_method("PUT");
    opts.set_mode(RequestMode::SameOrigin);
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers
        .set("Content-Type", "application/json")
        .map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);
    let uint8 = js_sys::Uint8Array::from(bytes);
    opts.set_body(&uint8.into());
    let req = Request::new_with_str_and_init(url, &opts).map_err(|e| format!("{e:?}"))?;
    let resp_val = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: Response = resp_val.dyn_into().map_err(|e| format!("{e:?}"))?;
    Ok(resp.status())
}
