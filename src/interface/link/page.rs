//! What the browser is shown.
//!
//! Every message is a fixed string chosen here. Nothing the caller sent is
//! echoed: this page is reached straight from a redirect an attacker can
//! compose, so anything reflected onto it is reflected on their behalf.

use bytes::Bytes;
use http::{Response, StatusCode, header};

use super::respond;

pub fn linked(message: &'static str) -> Response<Bytes> {
    respond(StatusCode::OK, body("Linked", message))
}

pub fn failed(message: &'static str) -> Response<Bytes> {
    respond(StatusCode::BAD_REQUEST, body("Not linked", message))
}

/// Send the browser on to the authorization server.
///
/// 303 rather than 302: the browser must follow it with a GET regardless of how
/// it arrived, and must not treat the destination as a replacement for the link
/// the user clicked.
pub fn redirect(location: &str, browser_token: &str) -> Response<Bytes> {
    let mut response = respond(StatusCode::SEE_OTHER, body("Signing in", "Redirecting…"));
    match header::HeaderValue::from_str(location) {
        Ok(value) => {
            response.headers_mut().insert(header::LOCATION, value);
            // `Lax` rather than `Strict`: the callback is a top-level navigation
            // from the authorization server, and `Strict` would withhold the
            // cookie on exactly the request that needs it. `HttpOnly` and the
            // path keep it away from scripts and from the rest of the host.
            if let Ok(cookie) = header::HeaderValue::from_str(&format!(
                "{}={browser_token}; Path=/link; Max-Age={}; HttpOnly; Secure; SameSite=Lax",
                super::BROWSER_COOKIE,
                crate::domain::identity::LINK_TICKET_TTL,
            )) {
                response.headers_mut().insert(header::SET_COOKIE, cookie);
            }
            response
        }
        // Unreachable with a well-formed authorization endpoint, and a 303 with
        // no Location is a blank page with no explanation.
        Err(_) => failed("Something went wrong. Try the button again."),
    }
}

fn body(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title}</title>\
         <style>body{{font:16px/1.5 system-ui,sans-serif;margin:0;display:grid;\
         place-items:center;min-height:100vh;color:#111;background:#fafafa}}\
         p{{max-width:28rem;padding:0 1.5rem;text-align:center}}\
         @media(prefers-color-scheme:dark){{body{{color:#eee;background:#111}}}}</style>\
         <p>{message}<br><small>You can close this page and return to Telegram.</small></p>"
    )
}
