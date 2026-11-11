//! Whole-node grants manage the Host, reserving account authority and automation writes.

use std::collections::HashMap;
use std::sync::OnceLock;

use model::methods::InboundKind;
use model::{ErrorCode, Request};

use super::{Decision, Private, Session, agents};
use crate::Shared;

/// Admit registered methods while reserving sharing and account authority for the owner.
pub(super) fn allows(method: &str, kind: InboundKind) -> bool {
    static METHODS: OnceLock<HashMap<&'static str, InboundKind>> = OnceLock::new();
    if ["share.", "publication.", "relay.", "hub.", "push."]
        .iter()
        .any(|prefix| method.starts_with(prefix))
        || method == "session.heartbeat"
    {
        return false;
    }
    if (method.starts_with("bot.") || method.starts_with("schedule."))
        && !matches!(
            method,
            "schedule.list.request"
                | "schedule.inspect.request"
                | "schedule.logs.request"
                | "bot.get.request"
                | "bot.channel.list.request"
                | "bot.channel.types.request"
                | "bot.channel.status.request"
                | "bot.conversation.list.request"
                | "bot.history.get.request"
                | "bot.turn.get.request"
                | "bot.memory.get.request"
        )
    {
        return false;
    }
    let methods = METHODS.get_or_init(|| {
        crate::capabilities::implemented_methods()
            .map(|spec| (spec.name, spec.kind))
            .chain([("server.status.unsubscribe", InboundKind::Request)])
            .collect()
    });
    methods.get(method) == Some(&kind)
}

/// Preserve attribution and private download/terminal accounting for a whole-node request.
pub(super) async fn request(session: &Session, state: &Shared, mut request: Request) -> Decision {
    if !allows(&request.method, InboundKind::Request) {
        return Decision::Refuse(request, ErrorCode::Forbidden);
    }
    match request.method.as_str() {
        "terminal.create.request" => Decision::Private(request, Private::TerminalCreate),
        "fs.file.download_token.request" | "agent.timeline.export.request" => {
            if session.take_download(std::time::Instant::now()) {
                Decision::Private(request, Private::Download)
            } else {
                Decision::Refuse(request, ErrorCode::ResourceExhausted)
            }
        }
        "agent.create.request" => match agents::create(session, state, &mut request.params).await {
            Ok(()) => Decision::Private(request, Private::AgentCreate),
            Err(code) => Decision::Refuse(request, code),
        },
        "workspace.create.request" => {
            if let Some(agent) = request
                .params
                .get_mut("agent")
                .filter(|agent| !agent.is_null())
                && let Err(code) = agents::create(session, state, agent).await
            {
                return Decision::Refuse(request, code);
            }
            if request
                .params
                .get("agent")
                .is_some_and(|agent| !agent.is_null())
            {
                Decision::Private(request, Private::AgentCreate)
            } else {
                Decision::Forward(request)
            }
        }
        _ => Decision::Forward(request),
    }
}
