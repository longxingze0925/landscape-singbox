use std::path::PathBuf;

use bollard::{
    errors::Error as DockerBollardError,
    query_parameters::{
        CreateContainerOptions, CreateImageOptions, InspectContainerOptions,
        RemoveContainerOptions, StartContainerOptions, StopContainerOptions,
    },
    secret::{
        ContainerCreateBody, ContainerStateStatusEnum, HostConfig, Mount, MountTypeEnum,
        RestartPolicy, RestartPolicyNameEnum,
    },
    Docker,
};
use landscape_common::{
    proxy::{
        proxy_container_name, ProxyError, ProxyNodeConfig, ProxyNodeRuntimeStatus,
        ProxyProtocolConfig, ProxyRuntimeState,
    },
    NAMESPACE_REGISTER_SOCK_PATH, NAMESPACE_REGISTER_SOCK_PATH_IN_DOCKER,
};
use serde_json::{json, Map, Value};
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::proxy::node_service::ProxyNodeService;
use landscape_common::service::controller::ConfigController;

const DEFAULT_PROXY_IMAGE: &str = "ghcr.io/longxingze0925/landscape-singbox:latest";
const PROXY_TPROXY_PORT: u16 = 12345;

#[derive(Clone)]
pub struct ProxyRuntimeService {
    node_service: ProxyNodeService,
    home_path: PathBuf,
    image: String,
}

impl ProxyRuntimeService {
    pub fn new(node_service: ProxyNodeService, home_path: PathBuf) -> Self {
        let image = std::env::var("LANDSCAPE_PROXY_IMAGE")
            .ok()
            .filter(|image| !image.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_PROXY_IMAGE.to_string());
        Self { node_service, home_path, image }
    }

    pub async fn list_status(&self) -> Result<Vec<ProxyNodeRuntimeStatus>, ProxyError> {
        let mut result = Vec::new();
        for node in self.node_service.list().await {
            result.push(self.status_for_node(&node).await?);
        }
        result.sort_by(|a, b| a.container_name.cmp(&b.container_name));
        Ok(result)
    }

    pub async fn status(&self, node_id: Uuid) -> Result<ProxyNodeRuntimeStatus, ProxyError> {
        let node =
            self.node_service.find_by_id(node_id).await.ok_or(ProxyError::NodeNotFound(node_id))?;
        self.status_for_node(&node).await
    }

    pub async fn sync_node(&self, node_id: Uuid) -> Result<ProxyNodeRuntimeStatus, ProxyError> {
        let node =
            self.node_service.find_by_id(node_id).await.ok_or(ProxyError::NodeNotFound(node_id))?;

        if !node.enable {
            self.stop_node(node_id).await?;
            return Err(ProxyError::NodeDisabled(node_id));
        }

        self.write_config(&node).await?;
        self.recreate_container(&node).await?;
        self.status_for_node(&node).await
    }

    pub async fn stop_node(&self, node_id: Uuid) -> Result<ProxyNodeRuntimeStatus, ProxyError> {
        let node =
            self.node_service.find_by_id(node_id).await.ok_or(ProxyError::NodeNotFound(node_id))?;
        let container_name = proxy_container_name(node.id);

        let docker = docker()?;
        match docker.stop_container(&container_name, None::<StopContainerOptions>).await {
            Ok(()) => {}
            Err(DockerBollardError::DockerResponseServerError { status_code, .. })
                if status_code == 304 || status_code == 404 => {}
            Err(err) => return Err(ProxyError::RuntimeDockerError(err.to_string())),
        }

        self.status_for_node(&node).await
    }

    pub async fn remove_node_container(
        &self,
        node_id: Uuid,
    ) -> Result<ProxyNodeRuntimeStatus, ProxyError> {
        let node =
            self.node_service.find_by_id(node_id).await.ok_or(ProxyError::NodeNotFound(node_id))?;
        let container_name = proxy_container_name(node.id);

        let docker = docker()?;
        remove_container_if_exists(&docker, &container_name).await?;

        self.status_for_node(&node).await
    }

    async fn status_for_node(
        &self,
        node: &ProxyNodeConfig,
    ) -> Result<ProxyNodeRuntimeStatus, ProxyError> {
        let container_name = proxy_container_name(node.id);
        let docker = match docker() {
            Ok(docker) => docker,
            Err(err) => {
                return Ok(ProxyNodeRuntimeStatus {
                    node_id: node.id,
                    container_name,
                    state: ProxyRuntimeState::Unknown,
                    image: self.image.clone(),
                    status: Some(err.to_string()),
                });
            }
        };
        let inspect =
            docker.inspect_container(&container_name, None::<InspectContainerOptions>).await;

        let (state, status) = match inspect {
            Ok(info) => {
                let status = info.state.as_ref().and_then(|state| state.status);
                let runtime_state = match status {
                    Some(ContainerStateStatusEnum::CREATED) => ProxyRuntimeState::Created,
                    Some(ContainerStateStatusEnum::RUNNING) => ProxyRuntimeState::Running,
                    Some(ContainerStateStatusEnum::EXITED)
                    | Some(ContainerStateStatusEnum::DEAD) => ProxyRuntimeState::Exited,
                    Some(_) => ProxyRuntimeState::Unknown,
                    None => ProxyRuntimeState::Unknown,
                };
                (runtime_state, status.map(|status| status.to_string()))
            }
            Err(DockerBollardError::DockerResponseServerError { status_code, .. })
                if status_code == 404 =>
            {
                (ProxyRuntimeState::Missing, None)
            }
            Err(err) => return Err(ProxyError::RuntimeDockerError(err.to_string())),
        };

        Ok(ProxyNodeRuntimeStatus {
            node_id: node.id,
            container_name,
            state,
            image: self.image.clone(),
            status,
        })
    }

    async fn recreate_container(&self, node: &ProxyNodeConfig) -> Result<(), ProxyError> {
        let container_name = proxy_container_name(node.id);
        let docker = docker()?;

        ensure_image_available(&docker, &self.image).await?;
        remove_container_if_exists(&docker, &container_name).await?;

        let container_config = self.container_config(node)?;
        docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(container_name.clone()),
                    platform: String::new(),
                }),
                container_config,
            )
            .await
            .map_err(|err| ProxyError::RuntimeDockerError(err.to_string()))?;

        docker
            .start_container(&container_name, None::<StartContainerOptions>)
            .await
            .map_err(|err| ProxyError::RuntimeDockerError(err.to_string()))?;

        Ok(())
    }

    fn container_config(&self, node: &ProxyNodeConfig) -> Result<ContainerCreateBody, ProxyError> {
        let config_path = self.config_path(node.id);
        let config_bind = config_path
            .to_str()
            .ok_or_else(|| ProxyError::RuntimeConfigError("invalid proxy config path".into()))?
            .to_string();
        let socket_dir = self.home_path.join(NAMESPACE_REGISTER_SOCK_PATH);
        let socket_bind = socket_dir
            .to_str()
            .ok_or_else(|| ProxyError::RuntimeConfigError("invalid unix socket path".into()))?
            .to_string();

        Ok(ContainerCreateBody {
            image: Some(self.image.clone()),
            env: Some(vec![
                "LAND_PROXY_SERVER_ADDR=0.0.0.0".to_string(),
                "LAND_PROXY_SERVER_ADDR_V6=::".to_string(),
                format!("LAND_PROXY_SERVER_PORT={PROXY_TPROXY_PORT}"),
                "LAND_PROXY_HANDLE_MODE=tproxy".to_string(),
            ]),
            labels: Some(std::collections::HashMap::from([(
                "ld_flow_edge".to_string(),
                "proxy".to_string(),
            )])),
            host_config: Some(HostConfig {
                restart_policy: Some(RestartPolicy {
                    name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                    maximum_retry_count: Some(0),
                }),
                cap_add: Some(vec![
                    "NET_ADMIN".to_string(),
                    "BPF".to_string(),
                    "PERFMON".to_string(),
                ]),
                sysctls: Some(std::collections::HashMap::from([(
                    "net.ipv4.conf.lo.accept_local".to_string(),
                    "1".to_string(),
                )])),
                mounts: Some(vec![
                    Mount {
                        target: Some("/etc/sing-box/config.json".to_string()),
                        source: Some(config_bind),
                        typ: Some(MountTypeEnum::BIND),
                        read_only: Some(true),
                        ..Default::default()
                    },
                    Mount {
                        target: Some(format!("/{NAMESPACE_REGISTER_SOCK_PATH_IN_DOCKER}")),
                        source: Some(socket_bind),
                        typ: Some(MountTypeEnum::BIND),
                        read_only: Some(true),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    async fn write_config(&self, node: &ProxyNodeConfig) -> Result<(), ProxyError> {
        let dir = self.config_dir();
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|err| ProxyError::RuntimeConfigError(err.to_string()))?;
        let config = build_sing_box_config(node)?;
        let bytes = serde_json::to_vec_pretty(&config)
            .map_err(|err| ProxyError::RuntimeConfigError(err.to_string()))?;
        tokio::fs::write(self.config_path(node.id), bytes)
            .await
            .map_err(|err| ProxyError::RuntimeConfigError(err.to_string()))?;
        Ok(())
    }

    fn config_dir(&self) -> PathBuf {
        self.home_path.join("proxy")
    }

    fn config_path(&self, node_id: Uuid) -> PathBuf {
        self.config_dir().join(format!("{}.json", node_id.simple()))
    }
}

fn docker() -> Result<Docker, ProxyError> {
    Docker::connect_with_socket_defaults()
        .map_err(|err| ProxyError::RuntimeDockerError(err.to_string()))
}

async fn ensure_image_available(docker: &Docker, image: &str) -> Result<(), ProxyError> {
    let pull_policy = std::env::var("LANDSCAPE_PROXY_IMAGE_PULL_POLICY")
        .unwrap_or_else(|_| "missing".to_string());

    if pull_policy.eq_ignore_ascii_case("never") {
        return Ok(());
    }

    if !pull_policy.eq_ignore_ascii_case("always") {
        match docker.inspect_image(image).await {
            Ok(_) => return Ok(()),
            Err(DockerBollardError::DockerResponseServerError { status_code, .. })
                if status_code == 404 => {}
            Err(err) => return Err(ProxyError::RuntimeDockerError(err.to_string())),
        }
    }

    pull_image(docker, image).await
}

async fn pull_image(docker: &Docker, image: &str) -> Result<(), ProxyError> {
    let options = create_image_options(image);
    let mut stream = docker.create_image(Some(options), None, None);

    while let Some(result) = stream.next().await {
        result.map_err(|err| ProxyError::RuntimeDockerError(err.to_string()))?;
    }

    Ok(())
}

fn create_image_options(image: &str) -> CreateImageOptions {
    if image.contains('@') {
        return CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };
    }

    let last_slash = image.rfind('/');
    let last_colon = image.rfind(':');
    let (from_image, tag) = match last_colon {
        Some(colon) if last_slash.map_or(true, |slash| colon > slash) => {
            (image[..colon].to_string(), image[colon + 1..].to_string())
        }
        _ => (image.to_string(), "latest".to_string()),
    };

    CreateImageOptions {
        from_image: Some(from_image),
        tag: Some(tag),
        ..Default::default()
    }
}

async fn remove_container_if_exists(
    docker: &Docker,
    container_name: &str,
) -> Result<(), ProxyError> {
    let options = RemoveContainerOptions { force: true, v: true, link: false };
    match docker.remove_container(container_name, Some(options)).await {
        Ok(()) => Ok(()),
        Err(DockerBollardError::DockerResponseServerError { status_code, .. })
            if status_code == 404 =>
        {
            Ok(())
        }
        Err(err) => Err(ProxyError::RuntimeDockerError(err.to_string())),
    }
}

fn build_sing_box_config(node: &ProxyNodeConfig) -> Result<Value, ProxyError> {
    let outbound = build_outbound(node)?;
    Ok(json!({
        "log": {
            "level": "info",
            "timestamp": true
        },
        "inbounds": [
            {
                "type": "tproxy",
                "tag": "landscape-in",
                "listen": "::",
                "listen_port": PROXY_TPROXY_PORT
            }
        ],
        "outbounds": [
            outbound,
            {
                "type": "direct",
                "tag": "direct"
            }
        ],
        "route": {
            "final": "proxy"
        }
    }))
}

fn build_outbound(node: &ProxyNodeConfig) -> Result<Value, ProxyError> {
    let mut base = Map::from_iter([
        ("tag".to_string(), json!("proxy")),
        ("server".to_string(), json!(node.server)),
        ("server_port".to_string(), json!(node.port)),
    ]);

    match &node.protocol {
        ProxyProtocolConfig::Vless { uuid, flow, tls, server_name } => {
            base.insert("type".to_string(), json!("vless"));
            base.insert("uuid".to_string(), json!(uuid));
            if let Some(flow) = non_empty_opt(flow) {
                base.insert("flow".to_string(), json!(flow));
            }
            if *tls {
                base.insert("tls".to_string(), tls_config(server_name));
            }
        }
        ProxyProtocolConfig::Vmess { uuid, alter_id, security, tls, server_name } => {
            base.insert("type".to_string(), json!("vmess"));
            base.insert("uuid".to_string(), json!(uuid));
            base.insert("alter_id".to_string(), json!(alter_id));
            if let Some(security) = non_empty_opt(security) {
                base.insert("security".to_string(), json!(security));
            }
            if *tls {
                base.insert("tls".to_string(), tls_config(server_name));
            }
        }
        ProxyProtocolConfig::Shadowsocks { method, password } => {
            base.insert("type".to_string(), json!("shadowsocks"));
            base.insert("method".to_string(), json!(method));
            base.insert("password".to_string(), json!(password));
        }
        ProxyProtocolConfig::Socks5 { username, password } => {
            base.insert("type".to_string(), json!("socks"));
            if let Some(username) = non_empty_opt(username) {
                base.insert("username".to_string(), json!(username));
            }
            if let Some(password) = non_empty_opt(password) {
                base.insert("password".to_string(), json!(password));
            }
        }
    }

    Ok(Value::Object(base))
}

fn non_empty_opt(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

fn tls_config(server_name: &Option<String>) -> Value {
    let mut tls = Map::from_iter([("enabled".to_string(), json!(true))]);
    if let Some(server_name) = non_empty_opt(server_name) {
        tls.insert("server_name".to_string(), json!(server_name));
    }
    Value::Object(tls)
}
