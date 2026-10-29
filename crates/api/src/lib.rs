//! Authenticated local HTTP and WebSocket transport for the independent server.

mod auth;
mod browser_auth;
mod capabilities;
mod composition;
pub use composition::summary_source;
mod connection;
mod files;
mod listener;
pub mod managed_relay;
mod mcp;
mod publication;
mod terminal_activity;
mod workspace_cleanup;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{ConnectInfo, Request, State, WebSocketUpgrade};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use browser::broker::Broker;
use metadata::automation::service::WorkspaceAutomation;
use metadata::directory::service::directory::Directory;
use metadata::worktrees::service::worktrees::{WorkspaceWorktrees, Worktrees};
use model::server::{Lifecycle, Limits, ServerInfo, VERSION};
use model::{LifecycleIntent, Runtime};
use provider::service::agent_execution::AgentExecution;
use schedule::service::Schedules;
use secrecy::{ExposeSecret, SecretString};
use terminal::service::Terminals;
use tokio::sync::Semaphore;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use voice::service::Speech;

pub use auth::validate_token;
pub use browser_auth::validate_browser_origin;
use capabilities::installed_capabilities;
pub use listener::LocalAddress;
use listener::allowed_authorities;

/// Invalid startup configuration for the transport.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Only addresses with an assigned port are supported.
    #[error("server requires an address with an assigned port")]
    InvalidAddress,
    /// Tokens must contain 32–256 visible ASCII characters without spaces.
    #[error("AIT_SERVER_TOKEN must contain 32–256 visible ASCII characters without spaces")]
    InvalidToken,
    /// Browser origins must be explicit canonical HTTP loopback origins.
    #[error(
        "browser origin must be an HTTP loopback origin without a path (for example http://localhost:8081)"
    )]
    InvalidBrowserOrigin,
    /// Browser policy must be configured before cloning or serving the API.
    #[error("configure browser origins before cloning the API")]
    SharedConfiguration,
    /// A shared service failed before transport initialization completed.
    #[error("shared service unavailable during API initialization")]
    ServiceInitialization,
}

#[derive(Debug)]
struct Shared {
    _push_delivery: Option<metadata::push::delivery::Worker>,
    relay: relay::Connector,
    managed_relay: bool,
    publication_control: Option<host_link::control::Handle>,
    managed_status: Arc<Mutex<serde_json::Value>>,
    runtime: Arc<Runtime>,
    token: SecretString,
    agent_mcp_token: Option<SecretString>,
    browser_auth: browser_auth::BrowserAuth,
    authorities: Vec<String>,
    wildcard_listener: bool,
    connections: Arc<Semaphore>,
    metadata: Arc<metadata::dispatch::State>,
    filesystem: Arc<filesystem::dispatch::State>,
    provider: Arc<provider::dispatch::State>,
    terminal: Arc<terminal::dispatch::State>,
    voice: Arc<voice::dispatch::State>,
    schedule: schedule::dispatch::State,
    bot: bot::dispatch::State,
    browser: browser::dispatch::State,
    workspace_names: Option<metadata::directory::service::workspace_names::WorkspaceNames>,
}

impl std::ops::Deref for Shared {
    type Target = Runtime;

    fn deref(&self) -> &Runtime {
        &self.runtime
    }
}

/// Complete functional services installed independently at crate granularity.
/// Each present service supplies every method owned by its crate; API connection methods are built in.
#[derive(Debug, Default)]
pub struct Services {
    /// Workspace Bots, owned channels and private conversations.
    pub bot: Option<bot::service::Bots>,
    /// Project and Workspace metadata, configuration, labels, and automation.
    pub metadata: Option<metadata::Service>,
    /// File, Git, Forge, worktree, recovery, and skill operations.
    pub filesystem: Option<filesystem::Service>,
    /// Agent presets, runtime metadata, native execution, and history.
    pub provider: Option<provider::Service>,
    /// Persistent timed Agent executions.
    pub schedule: Option<Schedules>,
    /// Connection-owned browser automation.
    pub browser: Option<Broker>,
    /// Local terminal lifecycle and streaming.
    pub terminal: Option<Terminals>,
    /// Voice conversations and dictation.
    pub voice: Option<Speech>,
}

impl Shared {
    fn relay_management(&self) -> serde_json::Value {
        let mut status = self
            .managed_status
            .lock()
            .map_or(serde_json::Value::Null, |status| status.clone());
        if self.publication_control.is_some() {
            status["autonomous"] = serde_json::Value::Bool(true);
        }
        status
    }

    fn start_draining(&self) {
        if let Some(bots) = &self.bot.bots {
            bots.stop();
        }
        self.relay.begin_shutdown();
        if let Some(schedules) = &self.schedule.schedules {
            schedules.stop();
        }
        if let Some(generator) = &self.filesystem.summary_source {
            generator.shutdown();
        }
        if let Some(names) = &self.workspace_names {
            names.shutdown();
        }
        self.metadata.start_draining();
    }
}

/// Cloneable transport owner; the process host owns the listener and shutdown deadline.
#[derive(Debug, Clone)]
pub struct Api {
    shared: Arc<Shared>,
}

fn runtime_info(
    address: SocketAddr,
    server_id: String,
    instance_id: String,
    services: &Services,
) -> Arc<Runtime> {
    let implemented_capabilities = installed_capabilities(services);
    let capabilities = registered_capabilities();
    Arc::new(Runtime::new(ServerInfo {
        server_id,
        version: Some(model::BUILD_VERSION.to_owned()),
        instance_id,
        listen: address.to_string(),
        lifecycle: Lifecycle::Ready,
        protocol: VERSION,
        capabilities,
        features: capabilities::features(services),
        implemented_capabilities,
        limits: Limits::default(),
    }))
}

fn validate_listener(address: SocketAddr, token: &SecretString) -> Result<(), ConfigError> {
    use secrecy::ExposeSecret;
    if address.port() == 0 {
        return Err(ConfigError::InvalidAddress);
    }
    validate_token(token.expose_secret())
}

impl Api {
    /// Accept a private host credential on the Agent MCP route only.
    /// Configure before sharing or serving this API; the credential is never returned to clients.
    /// # Errors
    /// Returns invalid credential or shared-configuration errors.
    pub fn with_agent_mcp_token(mut self, token: SecretString) -> Result<Self, ConfigError> {
        validate_token(token.expose_secret())?;
        Arc::get_mut(&mut self.shared)
            .ok_or(ConfigError::SharedConfiguration)?
            .agent_mcp_token = Some(token);
        Ok(self)
    }

    /// Return the existing serve link as a center call port; no new network connection is opened.
    #[must_use]
    pub fn hub_center(&self) -> Arc<dyn model::hub::Center> {
        Arc::new(self.shared.relay.hub().clone())
    }

    /// Attach the daemon-owned publisher to committed session, creation and timeline events.
    pub fn set_event_publisher(&self, publisher: Arc<dyn model::hub::EventPublisher>) {
        self.shared
            .metadata
            .session_events
            .set_publisher(publisher.clone());
        self.shared
            .metadata
            .creations
            .set_publisher(publisher.clone());
        if let Some(execution) = &self.shared.provider.agent_execution {
            execution.set_timeline_publisher(publisher);
        }
    }

    /// Claim exclusive Relay control before sharing or serving the API.
    ///
    /// External HTTP/RPC start and stop operations then return conflict errors.
    /// # Errors
    /// Rejects an already shared API or a duplicate claim.
    pub fn claim_managed_relay(
        &mut self,
    ) -> Result<managed_relay::ManagedRelayHandle, ConfigError> {
        let shared = Arc::get_mut(&mut self.shared).ok_or(ConfigError::SharedConfiguration)?;
        if shared.managed_relay {
            return Err(ConfigError::SharedConfiguration);
        }
        shared.managed_relay = true;
        *shared
            .managed_status
            .lock()
            .map_err(|_| ConfigError::SharedConfiguration)? =
            serde_json::json!({"mode":"managed", "phase":"starting"});
        Ok(managed_relay::ManagedRelayHandle {
            relay: shared.relay.clone(),
            hub: shared.relay.hub().clone(),
            status: shared.managed_status.clone(),
        })
    }

    /// Install the daemon's sole credential and publication owner before serving requests.
    /// # Errors
    /// Rejects an already shared API or a duplicate control owner.
    pub fn set_publication_control(
        &mut self,
        control: host_link::control::Handle,
    ) -> Result<(), ConfigError> {
        let shared = Arc::get_mut(&mut self.shared).ok_or(ConfigError::SharedConfiguration)?;
        if shared.publication_control.is_some() {
            return Err(ConfigError::SharedConfiguration);
        }
        shared.publication_control = Some(control);
        Ok(())
    }

    /// Construct transport state for an already-bound TCP listener.
    ///
    /// `server_id` is persisted by the host; `instance_id` is unique to this process start.
    /// `token` authenticates the info and upgrade endpoints and is never returned to clients.
    /// `services` selects implemented methods; catalog placeholders remain negotiable.
    ///
    /// # Errors
    /// Returns an error for unassigned ports or invalid tokens.
    pub fn new(
        address: SocketAddr,
        server_id: String,
        instance_id: String,
        token: SecretString,
        services: Services,
    ) -> Result<Self, ConfigError> {
        validate_listener(address, &token)?;
        let relay = relay::Connector::new(
            address,
            token.clone(),
            server_id.clone(),
            instance_id.clone(),
        );
        let runtime = runtime_info(address, server_id, instance_id, &services);
        let services = composition::Parts::from(services);
        let session_events = services
            .agent_execution
            .as_ref()
            .map(AgentExecution::events)
            .unwrap_or_default();
        compose_automation_events(services.workspace_automation.as_ref(), &session_events)?;
        let creations = creation_receipts(&services);
        let worktrees = services.worktrees;
        let (directory, has_git_fetch) = compose_directory(
            services.directory,
            worktrees.as_ref(),
            services.git_fetch,
            &runtime,
            &session_events,
            services.workspace_automation.is_some(),
        );
        let directory_changes = directory.as_ref().and_then(Directory::changes);
        let metadata = Arc::new(metadata::dispatch::State {
            push_tokens: services.push_tokens.map(shared_service),
            runtime: runtime.clone(),
            daemon: services.daemon.map(shared_service),
            directory: directory.map(shared_service),
            workspace_labels: services.workspace_labels.map(shared_service),
            workspace_automation: services.workspace_automation,
            workspace_state: services.workspace_state.map(shared_service),
            worktrees,
            workspace_recovery: services.workspace_recovery.map(shared_service),
            github_projects: services.github_projects.map(shared_service),
            session_events,
            creations,
            has_agent_execution: services.agent_execution.is_some(),
            has_terminals: services.terminals.is_some(),
            has_git_fetch,
        });
        let filesystem = Arc::new(filesystem::dispatch::State {
            summary_source: services.summary_source.clone(),
            runtime: runtime.clone(),
            checkout: services.checkout.map(shared_service),
            forge: services.forge.map(shared_service),
            files: services.files.map(shared_service),
        });
        let provider = Arc::new(provider::dispatch::State {
            runtime: runtime.clone(),
            agents: services.agents.map(shared_service),
            skills: services.skills.map(shared_service),
            agent_runtime: None,
            agent_execution: services.agent_execution,
            summary_source: services.summary_source.clone(),
            directory_changes,
            has_terminals: services.terminals.is_some(),
        });
        let terminal = terminal_activity::compose(
            &runtime,
            services.terminals,
            address,
            &metadata.session_events,
        );
        let push_delivery = metadata::push::delivery::start(&metadata);
        let api = Self {
            shared: Arc::new(Shared {
                _push_delivery: push_delivery,
                relay,
                managed_relay: false,
                publication_control: None,
                managed_status: Arc::new(Mutex::new(serde_json::json!({"mode":"external"}))),
                schedule: schedule::dispatch::State {
                    schedules: services.schedules,
                },
                bot: services.bots,
                browser: browser::dispatch::State {
                    broker: services.browser,
                },
                voice: Arc::new(voice::dispatch::State {
                    runtime: runtime.clone(),
                    speech: services.speech,
                }),
                workspace_names: services.workspace_names,
                runtime,
                metadata,
                filesystem,
                provider,
                terminal,
                token,
                agent_mcp_token: None,
                browser_auth: browser_auth::BrowserAuth::default(),
                authorities: allowed_authorities(address),
                wildcard_listener: address.ip().is_unspecified(),
                connections: Arc::new(Semaphore::new(model::server::MAX_CONNECTIONS)),
            }),
        };
        api.start_runtime_services()
    }

    fn start_runtime_services(self) -> Result<Self, ConfigError> {
        workspace_cleanup::configure(&self.shared)?;
        terminal::connection::maintain(&self.shared.terminal);
        Ok(self)
    }

    /// Allow browser pages from the given explicit HTTP loopback origins.
    ///
    /// Returns the configured API; configure it before cloning or serving it.
    /// # Errors
    /// Rejects invalid origins or an API that has already been cloned.
    pub fn with_browser_origins(mut self, origins: Vec<String>) -> Result<Self, ConfigError> {
        let browser_auth = browser_auth::BrowserAuth::new(origins)?;
        Arc::get_mut(&mut self.shared)
            .ok_or(ConfigError::SharedConfiguration)?
            .browser_auth = browser_auth;
        Ok(self)
    }

    /// Build routes with origin checks, bounded HTTP handling, and metadata-only tracing.
    pub fn router(&self) -> Router {
        Router::new()
            .route("/healthz", get(health))
            .route("/readyz", get(ready))
            .route("/v1/server/info", get(info))
            .route("/v1/ws", get(upgrade))
            .route(
                "/api/relay/control",
                get(relay_status).put(relay_start).delete(relay_stop),
            )
            .route(
                "/api/hub/control",
                get(hub_status).put(hub_start).delete(hub_stop),
            )
            .route(
                "/api/device/authorization",
                get(publication::http_status)
                    .put(publication::http_authorize)
                    .delete(publication::http_disconnect),
            )
            .route(browser_auth::TICKET_PATH, post(browser_ticket))
            .route("/api/files/download", get(files::download))
            .route(terminal_activity::PATH, post(terminal_activity::report))
            .fallback(|| async { ApiError(StatusCode::NOT_FOUND) })
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(10),
            ))
            .merge(mcp::router())
            .layer(middleware::from_fn_with_state(self.shared.clone(), guard))
            .layer(TraceLayer::new_for_http().make_span_with(
                |request: &Request| tracing::info_span!("server_http", method = %request.method()),
            ))
            .with_state(self.shared.clone())
    }

    /// Atomically reject new upgrades, change readiness, and signal existing connections.
    pub fn begin_shutdown(&self) {
        let _guard = self
            .shared
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.shared.start_draining();
    }

    /// Return the first WebSocket lifecycle request accepted during this run.
    #[must_use]
    pub fn lifecycle_intent(&self) -> Option<LifecycleIntent> {
        self.shared
            .lifecycle_intent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Wait for all admitted upgrades and connections after calling `begin_shutdown`.
    /// The process host must bound this wait with its shutdown deadline.
    pub async fn wait_closed(&self) {
        self.shared.relay.stop().await;
        self.shared.relay.hub().stop().await;
        self.shared.tasks.wait().await;
        if let Some(names) = &self.shared.workspace_names {
            names.wait_closed().await;
        }
        if let Some(schedules) = &self.shared.schedule.schedules
            && schedules.shutdown().await.is_err()
        {
            tracing::error!("schedule shutdown failed");
        }
        if let Some(bots) = &self.shared.bot.bots
            && bots.shutdown().await.is_err()
        {
            tracing::error!("Bot shutdown failed");
        }
        if let Some(terminals) = self.shared.terminal.terminals.clone() {
            let result = tokio::task::spawn_blocking(move || {
                terminals
                    .lock()
                    .map_err(|_| terminal::Error::Io)?
                    .shutdown()
            })
            .await;
            if !matches!(result, Ok(Ok(()))) {
                tracing::error!("terminal shutdown failed");
            }
        }
        if let Some(execution) = &self.shared.provider.agent_execution
            && execution.shutdown().await.is_err()
        {
            tracing::error!("native Provider shutdown failed");
        }
    }

    /// Wait until shutdown has closed admission; useful for bounding the host's drain phase.
    pub async fn wait_draining(&self) {
        self.shared.cancellation.cancelled().await;
    }
}

fn registered_capabilities() -> Vec<String> {
    capabilities::implemented_methods()
        .map(|method| method.name.to_owned())
        .chain(std::iter::once(
            model::server::single::CAPABILITY.to_owned(),
        ))
        .collect()
}

#[derive(Debug)]
struct ApiError(StatusCode);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(
                serde_json::json!({"error": self.0.canonical_reason().unwrap_or("request failed")}),
            ),
        )
            .into_response()
    }
}

async fn guard(
    State(state): State<Arc<Shared>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let destination_authorities;
    let authorities = if state.wildcard_listener {
        let local = request
            .extensions()
            .get::<ConnectInfo<LocalAddress>>()
            .ok_or(ApiError(StatusCode::FORBIDDEN))?;
        destination_authorities = local.0.authorities();
        &destination_authorities
    } else {
        &state.authorities
    };
    auth::validate_source(request.headers(), authorities, &state.browser_auth)?;
    if request.uri().path() == browser_auth::TICKET_PATH {
        let origin = request
            .headers()
            .get("origin")
            .cloned()
            .ok_or(ApiError(StatusCode::FORBIDDEN))?;
        let mut response = if request.uri().query().is_some() {
            ApiError(StatusCode::BAD_REQUEST).into_response()
        } else if request.method() == Method::OPTIONS {
            StatusCode::NO_CONTENT.into_response()
        } else if let Err(error) = auth::authenticate(request.headers(), &state.token) {
            error.into_response()
        } else {
            next.run(request).await
        };
        browser_auth::cors(response.headers_mut(), origin);
        return Ok(response);
    }
    let download = request.method() == Method::GET && request.uri().path() == "/api/files/download";
    let terminal_activity =
        request.method() == Method::POST && request.uri().path() == terminal_activity::PATH;
    if !matches!(request.uri().path(), "/healthz" | "/readyz") && !download && !terminal_activity {
        if request.uri().path() == "/v1/ws" && !request.headers().contains_key("authorization") {
            state.browser_auth.consume(request.headers())?;
        } else {
            let authenticated = auth::authenticate(request.headers(), &state.token);
            if request.uri().path() == mcp::PATH {
                authenticated.or_else(|error| {
                    state.agent_mcp_token.as_ref().map_or(Err(error), |token| {
                        auth::authenticate(request.headers(), token)
                    })
                })?;
            } else {
                authenticated?;
            }
        }
    }
    // Credentials and client state must never be accepted in a URL.
    if request.uri().query().is_some() && !download {
        return Err(ApiError(StatusCode::BAD_REQUEST));
    }
    Ok(next.run(request).await)
}

async fn browser_ticket(
    State(state): State<Arc<Shared>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if state.cancellation.is_cancelled() {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE));
    }
    let origin = auth::single_header(&headers, "origin")?.ok_or(ApiError(StatusCode::FORBIDDEN))?;
    let ticket = state.browser_auth.issue(origin)?;
    Ok(Json(serde_json::json!({"ticket": ticket})).into_response())
}

async fn health() -> Result<Response, ApiError> {
    Ok(Json(serde_json::json!({"status":"alive"})).into_response())
}

async fn relay_status(State(state): State<Arc<Shared>>) -> Json<serde_json::Value> {
    let status = state.relay.status().await;
    let mut value = serde_json::to_value(status).unwrap_or_default();
    value["management"] = state
        .managed_status
        .lock()
        .map_or(serde_json::Value::Null, |s| s.clone());
    Json(value)
}

async fn relay_start(
    State(state): State<Arc<Shared>>,
    Json(grant): Json<relay::ControlGrant>,
) -> Result<StatusCode, ApiError> {
    if state.cancellation.is_cancelled() {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE));
    }
    if state.managed_relay {
        return Err(ApiError(StatusCode::CONFLICT));
    }
    state
        .relay
        .start(grant)
        .await
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST))?;
    Ok(StatusCode::ACCEPTED)
}

async fn relay_stop(State(state): State<Arc<Shared>>) -> Result<StatusCode, ApiError> {
    if let Some(control) = &state.publication_control {
        control
            .disconnect()
            .await
            .map_err(publication::http_error)?;
        return Ok(StatusCode::NO_CONTENT);
    }
    if state.managed_relay {
        return Err(ApiError(StatusCode::CONFLICT));
    }
    state.relay.stop().await;
    Ok(StatusCode::NO_CONTENT)
}

async fn hub_status(State(state): State<Arc<Shared>>) -> Json<serde_json::Value> {
    let mut value = serde_json::to_value(state.relay.hub().status().await).unwrap_or_default();
    value["management"] = state.relay_management();
    Json(value)
}

/// Accept legacy Hub grants only for an unclaimed API; managed publication follows authorization.
async fn hub_start(
    State(state): State<Arc<Shared>>,
    Json(value): Json<serde_json::Value>,
) -> Result<StatusCode, ApiError> {
    if state.cancellation.is_cancelled() {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE));
    }
    if state.managed_relay {
        return Err(ApiError(StatusCode::CONFLICT));
    }
    let grant = serde_json::from_value(value).map_err(|_| ApiError(StatusCode::BAD_REQUEST))?;
    state
        .relay
        .hub()
        .start(grant)
        .await
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST))?;
    Ok(StatusCode::ACCEPTED)
}

async fn hub_stop(State(state): State<Arc<Shared>>) -> Result<StatusCode, ApiError> {
    if let Some(control) = &state.publication_control {
        control
            .disconnect()
            .await
            .map_err(publication::http_error)?;
        return Ok(StatusCode::NO_CONTENT);
    }
    if state.managed_relay {
        return Err(ApiError(StatusCode::CONFLICT));
    }
    state.relay.hub().stop().await;
    Ok(StatusCode::NO_CONTENT)
}

async fn ready(State(state): State<Arc<Shared>>) -> Result<Response, ApiError> {
    if state.cancellation.is_cancelled() {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE));
    }
    Ok(Json(serde_json::json!({"status":"ready"})).into_response())
}

async fn info(State(state): State<Arc<Shared>>) -> Result<Response, ApiError> {
    Ok(Json(state.info()).into_response())
}

async fn upgrade(
    State(state): State<Arc<Shared>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let guard = state
        .admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.cancellation.is_cancelled() {
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE));
    }
    let permit = state
        .connections
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS))?;
    let tracking = state.tasks.token();
    drop(guard);
    let protocol = headers
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with(browser_auth::TICKET_PROTOCOL));
    Ok(ws
        .protocols(protocol.map(str::to_owned))
        .max_message_size(model::server::MAX_MESSAGE_BYTES)
        .max_frame_size(model::server::MAX_MESSAGE_BYTES)
        .write_buffer_size(0)
        .max_write_buffer_size(model::server::MAX_QUEUE_BYTES)
        .on_upgrade(move |socket| async move {
            let (_tracking, _permit) = (tracking, permit);
            Box::pin(connection::serve(socket, state)).await;
        }))
}

#[cfg(test)]
mod tests;

fn compose_directory(
    directory: Option<Directory>,
    worktrees: Option<&Arc<Mutex<Worktrees>>>,
    git_fetch: Option<filesystem::git::service::git_fetch::GitFetch>,
    runtime: &Arc<Runtime>,
    events: &model::session::SessionEvents,
    has_automation: bool,
) -> (Option<Directory>, bool) {
    let has_git_fetch = directory.is_some() && git_fetch.is_some();
    let directory = directory.map(|directory| {
        let project_events = events.clone();
        let directory = directory.with_project_updates(Arc::new(move |mutation| {
            use domain::workspace::registry::MutationKind;
            use domain::session::protocol::SessionEventKind;

            let payload = if mutation.kind == MutationKind::Upsert {
                mutation.project.as_ref().map(|project| {
                    serde_json::json!({
                        "kind": "upsert",
                        "project": domain::workspace::protocol::projection::project_descriptor(project),
                    })
                })
            } else {
                Some(serde_json::json!({
                    "kind": "remove",
                    "projectId": mutation.project_id,
                }))
            };
            if let Some(payload) = payload {
                project_events.publish(SessionEventKind::ProjectUpdate, &payload);
            }
        }));
        let directory = if has_automation {
            let setup_events = events.clone();
            directory.with_workspace_updates(Arc::new(move |mutation| {
                use domain::workspace::registry::MutationKind;
                use domain::session::protocol::SessionEventKind;

                if mutation.kind != MutationKind::Upsert {
                    return;
                }
                let Some(workspace) = &mutation.workspace else {
                    return;
                };
                let Some(source) = &workspace.untrusted_source else {
                    return;
                };
                setup_events.publish(
                    SessionEventKind::WorkspaceSetupProgress,
                    &serde_json::json!({
                        "workspaceId": workspace.workspace_id,
                        "status": "blocked",
                        "detail": {
                            "type": "worktree_setup",
                            "worktreePath": workspace.worktree_root.as_ref().unwrap_or(&workspace.cwd),
                            "branchName": workspace.branch.as_deref().unwrap_or_default(),
                            "log": "",
                            "commands": [],
                        },
                        "error": null,
                        "blockedSource": source,
                    }),
                );
                setup_events.publish(SessionEventKind::WorkspaceSetupStatus, &serde_json::json!({
                    "workspaceId":workspace.workspace_id,"status":"blocked","hasCommands":false,
                }));
            }))
        } else {
            directory
        };
        let directory = if let Some(worktrees) = worktrees {
            directory.with_worktrees(Arc::new(WorkspaceWorktrees::new(worktrees.clone())))
        } else {
            directory
        };
        if let Some(fetch) = git_fetch {
            directory.with_git_observer(fetch.start(runtime, events.clone()))
        } else {
            directory
        }
    });
    (directory, has_git_fetch)
}

fn compose_automation_events(
    automation: Option<&Arc<Mutex<WorkspaceAutomation>>>,
    events: &model::session::SessionEvents,
) -> Result<(), ConfigError> {
    use domain::session::protocol::SessionEventKind;
    use metadata::automation::ports::AutomationEvent;

    let Some(automation) = automation else {
        return Ok(());
    };
    let events = events.clone();
    let summaries = Mutex::new(std::collections::BTreeMap::new());
    automation
        .lock()
        .map_err(|_| ConfigError::ServiceInitialization)?
        .set_event_sink(Arc::new(move |event| {
            let (kind, payload) = match event {
                AutomationEvent::Scripts {
                    workspace_id,
                    scripts,
                } => (
                    SessionEventKind::ScriptStatus,
                    serde_json::json!({
                        "workspaceId": workspace_id,
                        "scripts": scripts
                            .into_iter()
                            .map(metadata::automation::rpc::script)
                            .collect::<Vec<_>>(),
                    }),
                ),
                AutomationEvent::Setup {
                    workspace_id,
                    snapshot,
                } => {
                    let mut payload = serde_json::to_value(
                        metadata::automation::rpc::setup_snapshot(snapshot, None),
                    )
                    .unwrap_or_default();
                    payload["workspaceId"] = serde_json::json!(workspace_id);
                    (SessionEventKind::WorkspaceSetupProgress, payload)
                }
            };
            if kind == SessionEventKind::WorkspaceSetupProgress {
                let mut summary = metadata::automation::rpc::setup_summary(&payload);
                summary["workspaceId"] = payload["workspaceId"].clone();
                let key = payload["workspaceId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let state = (summary["status"].clone(), summary["hasCommands"].clone());
                let changed = {
                    let mut summaries = summaries
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let changed = summaries.get(&key) != Some(&state);
                    if summaries.len() >= 4096 {
                        summaries.pop_first();
                    }
                    summaries.insert(key, state);
                    changed
                };
                if changed {
                    events.publish(SessionEventKind::WorkspaceSetupStatus, &summary);
                }
            }
            events.publish(kind, &payload);
        }));
    Ok(())
}

fn shared_service<S>(service: S) -> Arc<Mutex<S>> {
    Arc::new(Mutex::new(service))
}

fn creation_receipts(services: &composition::Parts) -> model::creation::Creations {
    services
        .agent_execution
        .as_ref()
        .map(AgentExecution::creations)
        .or_else(|| services.directory.as_ref().map(Directory::creations))
        .unwrap_or_default()
}
