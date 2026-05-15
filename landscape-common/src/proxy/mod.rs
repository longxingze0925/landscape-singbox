use landscape_macro::LdApiError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::ConfigId;
use crate::database::repository::LandscapeDBStore;
use crate::utils::id::gen_database_uuid;
use crate::utils::time::get_f64_timestamp;

pub const PROXY_CONTAINER_NAME_PREFIX: &str = "landscape_proxy_";

#[derive(thiserror::Error, Debug, LdApiError)]
#[api_error(crate_path = "crate")]
pub enum ProxyError {
    #[error("Proxy node '{0}' not found")]
    #[api_error(id = "proxy.node_not_found", status = 404)]
    NodeNotFound(ConfigId),

    #[error("Proxy node '{node_id}' is used by flow '{flow_remark}' (ID: {flow_id})")]
    #[api_error(id = "proxy.node_in_use", status = 400)]
    NodeInUse { node_id: ConfigId, flow_remark: String, flow_id: u32 },

    #[error("Proxy node '{0}' is disabled")]
    #[api_error(id = "proxy.node_disabled", status = 400)]
    NodeDisabled(ConfigId),

    #[error("Proxy runtime Docker operation failed: {0}")]
    #[api_error(id = "proxy.runtime_docker_error", status = 500)]
    RuntimeDockerError(String),

    #[error("Proxy runtime config error: {0}")]
    #[api_error(id = "proxy.runtime_config_error", status = 400)]
    RuntimeConfigError(String),

    #[error("Proxy bypass China Geo cache is missing")]
    #[api_error(id = "proxy.bypass_geo_cache_missing", status = 400)]
    BypassGeoCacheMissing,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    Global,
    BypassChina,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "t")]
#[serde(rename_all = "snake_case")]
pub enum ProxyProtocolConfig {
    Vless {
        uuid: String,
        #[serde(default)]
        flow: Option<String>,
        #[serde(default)]
        tls: bool,
        #[serde(default)]
        server_name: Option<String>,
        #[serde(default)]
        reality: bool,
        #[serde(default)]
        reality_public_key: Option<String>,
        #[serde(default)]
        reality_short_id: Option<String>,
        #[serde(default)]
        utls_fingerprint: Option<String>,
    },
    Vmess {
        uuid: String,
        #[serde(default)]
        alter_id: u16,
        #[serde(default)]
        security: Option<String>,
        #[serde(default)]
        tls: bool,
        #[serde(default)]
        server_name: Option<String>,
    },
    Shadowsocks {
        method: String,
        password: String,
    },
    Socks5 {
        #[serde(default)]
        username: Option<String>,
        #[serde(default)]
        password: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProxyNodeConfig {
    #[serde(default = "gen_database_uuid")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub id: Uuid,
    pub name: String,
    pub enable: bool,
    pub server: String,
    pub port: u16,
    pub protocol: ProxyProtocolConfig,
    pub remark: String,
    #[serde(default = "get_f64_timestamp")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub update_at: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ProxyRuntimeState {
    Missing,
    Created,
    Running,
    Exited,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProxyNodeRuntimeStatus {
    pub node_id: Uuid,
    pub container_name: String,
    pub state: ProxyRuntimeState,
    pub image: String,
    #[cfg_attr(feature = "openapi", schema(required = true, nullable = true))]
    pub status: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ProxyBypassRuleSourceKind {
    Domain,
    Ip,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProxyBypassRuleSourceStatus {
    pub kind: ProxyBypassRuleSourceKind,
    pub name: String,
    pub key: String,
    #[cfg_attr(feature = "openapi", schema(required = true, nullable = true))]
    pub url: Option<String>,
    #[cfg_attr(feature = "openapi", schema(required = true, nullable = true))]
    pub next_update_at: Option<f64>,
    pub cache_exists: bool,
    pub item_count: usize,
    #[cfg_attr(feature = "openapi", schema(required = true, nullable = true))]
    pub last_success_at: Option<f64>,
    #[cfg_attr(feature = "openapi", schema(required = true, nullable = true))]
    pub last_error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProxyBypassRuleSourcesStatus {
    pub domain: ProxyBypassRuleSourceStatus,
    pub ip: ProxyBypassRuleSourceStatus,
}

impl LandscapeDBStore<Uuid> for ProxyNodeConfig {
    fn get_id(&self) -> Uuid {
        self.id
    }

    fn get_update_at(&self) -> f64 {
        self.update_at
    }

    fn set_update_at(&mut self, ts: f64) {
        self.update_at = ts;
    }
}

pub fn proxy_container_name(node_id: Uuid) -> String {
    format!("{PROXY_CONTAINER_NAME_PREFIX}{}", node_id.simple())
}

pub fn proxy_node_id_from_container_name(container_name: &str) -> Option<Uuid> {
    let raw_id = container_name.strip_prefix(PROXY_CONTAINER_NAME_PREFIX)?;
    Uuid::parse_str(raw_id).ok()
}
