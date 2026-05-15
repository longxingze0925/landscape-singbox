use axum::extract::{Path, State};
use landscape_common::api_response::LandscapeApiResp as CommonApiResp;
use landscape_common::config::ConfigId;
use landscape_common::flow::FlowTarget;
use landscape_common::proxy::{
    ProxyBypassRuleSourceKind, ProxyBypassRuleSourcesStatus, ProxyError, ProxyMode,
    ProxyNodeConfig, ProxyNodeRuntimeStatus,
};
use landscape_common::service::controller::ConfigController;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::JsonBody;
use crate::{api::LandscapeApiResp, error::LandscapeApiResult, LandscapeApp};

pub fn get_proxy_paths() -> OpenApiRouter<LandscapeApp> {
    OpenApiRouter::new()
        .routes(routes!(get_proxy_nodes, add_proxy_node))
        .routes(routes!(get_proxy_node, del_proxy_node))
        .routes(routes!(get_proxy_runtime_statuses))
        .routes(routes!(get_proxy_runtime_status))
        .routes(routes!(sync_proxy_runtime))
        .routes(routes!(stop_proxy_runtime))
        .routes(routes!(remove_proxy_runtime))
        .routes(routes!(get_bypass_rule_sources_status))
        .routes(routes!(refresh_bypass_domain_rule_source))
        .routes(routes!(refresh_bypass_ip_rule_source))
        .routes(routes!(refresh_bypass_rule_sources))
}

#[utoipa::path(
    get,
    path = "/nodes",
    tag = "Proxy",
    responses((status = 200, body = CommonApiResp<Vec<ProxyNodeConfig>>))
)]
async fn get_proxy_nodes(
    State(state): State<LandscapeApp>,
) -> LandscapeApiResult<Vec<ProxyNodeConfig>> {
    let mut result = state.proxy_node_service.list().await;
    result.sort_by(|a, b| a.name.cmp(&b.name));
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    get,
    path = "/nodes/{id}",
    tag = "Proxy",
    params(("id" = Uuid, Path, description = "Proxy node config ID")),
    responses(
        (status = 200, body = CommonApiResp<ProxyNodeConfig>),
        (status = 404, description = "Not found")
    )
)]
async fn get_proxy_node(
    State(state): State<LandscapeApp>,
    Path(id): Path<ConfigId>,
) -> LandscapeApiResult<ProxyNodeConfig> {
    let result = state.proxy_node_service.find_by_id(id).await;
    if let Some(config) = result {
        LandscapeApiResp::success(config)
    } else {
        Err(ProxyError::NodeNotFound(id))?
    }
}

#[utoipa::path(
    post,
    path = "/nodes",
    tag = "Proxy",
    request_body = ProxyNodeConfig,
    responses((status = 200, body = CommonApiResp<ProxyNodeConfig>))
)]
async fn add_proxy_node(
    State(state): State<LandscapeApp>,
    JsonBody(proxy_node): JsonBody<ProxyNodeConfig>,
) -> LandscapeApiResult<ProxyNodeConfig> {
    let result = state.proxy_node_service.checked_set(proxy_node).await?;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    delete,
    path = "/nodes/{id}",
    tag = "Proxy",
    params(("id" = Uuid, Path, description = "Proxy node config ID")),
    responses(
        (status = 200, description = "Success"),
        (status = 404, description = "Not found")
    )
)]
async fn del_proxy_node(
    State(state): State<LandscapeApp>,
    Path(id): Path<ConfigId>,
) -> LandscapeApiResult<()> {
    let flows = state
        .flow_rule_service
        .find_by_target(FlowTarget::Proxy { node_id: id, mode: ProxyMode::Global })
        .await?;
    let bypass_flows = state
        .flow_rule_service
        .find_by_target(FlowTarget::Proxy { node_id: id, mode: ProxyMode::BypassChina })
        .await?;
    if let Some(flow) = flows.first().or_else(|| bypass_flows.first()) {
        Err(ProxyError::NodeInUse {
            node_id: id,
            flow_remark: flow.remark.clone(),
            flow_id: flow.flow_id,
        })?;
    }

    state.proxy_node_service.delete(id).await;
    let _ = state.proxy_runtime_service.sync_runtime().await;
    LandscapeApiResp::success(())
}

#[utoipa::path(
    get,
    path = "/runtime/status",
    tag = "Proxy",
    responses((status = 200, body = CommonApiResp<Vec<ProxyNodeRuntimeStatus>>))
)]
async fn get_proxy_runtime_statuses(
    State(state): State<LandscapeApp>,
) -> LandscapeApiResult<Vec<ProxyNodeRuntimeStatus>> {
    let result = state.proxy_runtime_service.list_status().await?;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    get,
    path = "/runtime/status/{id}",
    tag = "Proxy",
    params(("id" = Uuid, Path, description = "Proxy node config ID")),
    responses((status = 200, body = CommonApiResp<ProxyNodeRuntimeStatus>))
)]
async fn get_proxy_runtime_status(
    State(state): State<LandscapeApp>,
    Path(id): Path<ConfigId>,
) -> LandscapeApiResult<ProxyNodeRuntimeStatus> {
    let result = state.proxy_runtime_service.status(id).await?;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    post,
    path = "/runtime/sync/{id}",
    tag = "Proxy",
    params(("id" = Uuid, Path, description = "Proxy node config ID")),
    responses((status = 200, body = CommonApiResp<ProxyNodeRuntimeStatus>))
)]
async fn sync_proxy_runtime(
    State(state): State<LandscapeApp>,
    Path(id): Path<ConfigId>,
) -> LandscapeApiResult<ProxyNodeRuntimeStatus> {
    let result = state.proxy_runtime_service.sync_node(id).await?;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    post,
    path = "/runtime/stop/{id}",
    tag = "Proxy",
    params(("id" = Uuid, Path, description = "Proxy node config ID")),
    responses((status = 200, body = CommonApiResp<ProxyNodeRuntimeStatus>))
)]
async fn stop_proxy_runtime(
    State(state): State<LandscapeApp>,
    Path(id): Path<ConfigId>,
) -> LandscapeApiResult<ProxyNodeRuntimeStatus> {
    let result = state.proxy_runtime_service.stop_node(id).await?;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    delete,
    path = "/runtime/container/{id}",
    tag = "Proxy",
    params(("id" = Uuid, Path, description = "Proxy node config ID")),
    responses((status = 200, body = CommonApiResp<ProxyNodeRuntimeStatus>))
)]
async fn remove_proxy_runtime(
    State(state): State<LandscapeApp>,
    Path(id): Path<ConfigId>,
) -> LandscapeApiResult<ProxyNodeRuntimeStatus> {
    let result = state.proxy_runtime_service.remove_node_container(id).await?;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    get,
    path = "/bypass/rule-sources/status",
    tag = "Proxy",
    responses((status = 200, body = CommonApiResp<ProxyBypassRuleSourcesStatus>))
)]
async fn get_bypass_rule_sources_status(
    State(state): State<LandscapeApp>,
) -> LandscapeApiResult<ProxyBypassRuleSourcesStatus> {
    let result = state.proxy_bypass_service.bypass_rule_sources_status().await;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    post,
    path = "/bypass/rule-sources/refresh/domain",
    tag = "Proxy",
    responses((status = 200, body = CommonApiResp<ProxyBypassRuleSourcesStatus>))
)]
async fn refresh_bypass_domain_rule_source(
    State(state): State<LandscapeApp>,
) -> LandscapeApiResult<ProxyBypassRuleSourcesStatus> {
    let result = state
        .proxy_bypass_service
        .refresh_bypass_rule_sources(Some(ProxyBypassRuleSourceKind::Domain))
        .await?;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    post,
    path = "/bypass/rule-sources/refresh/ip",
    tag = "Proxy",
    responses((status = 200, body = CommonApiResp<ProxyBypassRuleSourcesStatus>))
)]
async fn refresh_bypass_ip_rule_source(
    State(state): State<LandscapeApp>,
) -> LandscapeApiResult<ProxyBypassRuleSourcesStatus> {
    let result = state
        .proxy_bypass_service
        .refresh_bypass_rule_sources(Some(ProxyBypassRuleSourceKind::Ip))
        .await?;
    LandscapeApiResp::success(result)
}

#[utoipa::path(
    post,
    path = "/bypass/rule-sources/refresh/all",
    tag = "Proxy",
    responses((status = 200, body = CommonApiResp<ProxyBypassRuleSourcesStatus>))
)]
async fn refresh_bypass_rule_sources(
    State(state): State<LandscapeApp>,
) -> LandscapeApiResult<ProxyBypassRuleSourcesStatus> {
    let result = state.proxy_bypass_service.refresh_bypass_rule_sources(None).await?;
    LandscapeApiResp::success(result)
}
