use std::cell::RefCell;
use std::future::ready;

use super::*;

#[tokio::test]
async fn revocation_cancels_agents_beyond_the_first_directory_page() {
    let grant = Grant {
        share_id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
    };
    let cancelled = RefCell::new(Vec::new());
    let pages = RefCell::new(0);
    cancel_pages(|method, params| {
        let result = match method {
            "agent.list.request" => {
                assert_eq!(params["filter"]["labels"][GUEST_SHARE_LABEL], grant.share_id.to_string());
                assert_eq!(params["filter"]["labels"][GUEST_USER_LABEL], grant.user_id.to_string());
                assert_eq!(params["sort"], json!([{"key": "created_at", "direction": "asc"}]));
                *pages.borrow_mut() += 1;
                if params["page"]["cursor"].is_null() {
                    json!({"entries": (0..200).map(|id| json!({"agent": {"id": format!("agent-{id}")}})).collect::<Vec<_>>(),
                        "pageInfo": {"hasMore": true, "nextCursor": "after-200"}})
                } else {
                    assert_eq!(params["page"]["cursor"], "after-200");
                    json!({"entries": [{"agent": {"id": "agent-200"}}], "pageInfo": {"hasMore": false}})
                }
            }
            "agent.cancel.request" => {
                cancelled.borrow_mut().push(params["agentId"].as_str().unwrap().to_owned());
                json!({"accepted": true})
            }
            _ => panic!("unexpected method {method}"),
        };
        ready(Ok(result))
    }, grant).await;
    assert_eq!(*pages.borrow(), 2);
    let cancelled = cancelled.into_inner();
    assert_eq!(cancelled.len(), 201);
    assert_eq!(cancelled.last().unwrap(), "agent-200");
}

#[tokio::test]
async fn one_failed_cancellation_does_not_skip_later_agents() {
    let mut cancelled = Vec::new();
    cancel_pages(
        |method, params| {
            if method == "agent.list.request" {
                return ready(Ok(
                    json!({"entries": [{"agent": {"id": "gone"}}, {"agent": {"id": "running"}}]}),
                ));
            }
            let id = params["agentId"].as_str().unwrap().to_owned();
            let result = if id == "gone" {
                Err(provider::rpc::ErrorCode::AgentNotFound)
            } else {
                Ok(json!({}))
            };
            cancelled.push(id);
            ready(result)
        },
        Grant {
            share_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
        },
    )
    .await;
    assert_eq!(cancelled, ["gone", "running"]);
}
