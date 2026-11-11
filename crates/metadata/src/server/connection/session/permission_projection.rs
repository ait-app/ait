//! Legacy compatibility projection at the connection-owned permission event sink.

use serde_json::Value;

pub(super) fn request(params: &mut Value, secret_questions: bool) {
    if secret_questions {
        return;
    }
    let Some(request) = params.get_mut("request") else {
        return;
    };
    model::session::permission::project(request, false);
}

#[cfg(test)]
mod tests;
