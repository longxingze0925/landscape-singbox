use std::path::PathBuf;
use std::time::Duration;

use bollard::{
    container::LogOutput,
    errors::Error as DockerBollardError,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
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
    flow::{config::FlowConfig, FlowTarget},
    proxy::{
        proxy_container_name, ProxyError, ProxyNodeConfig, ProxyNodeRuntimeStatus,
        ProxyLatencyTestRequest, ProxyLatencyTestResult, ProxyLatencyTestState,
        ProxyLatencyTestTarget, ProxyProtocolConfig, ProxyRuntimeState, PROXY_CONTAINER_NAME_PREFIX,
        PROXY_RUNTIME_CONTAINER_NAME,
    },
    utils::time::get_f64_timestamp,
    NAMESPACE_REGISTER_SOCK_PATH, NAMESPACE_REGISTER_SOCK_PATH_IN_DOCKER,
};
use serde_json::{json, Map, Value};
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::{flow::rule_service::FlowRuleService, proxy::node_service::ProxyNodeService};
use landscape_common::service::controller::ConfigController;

const DEFAULT_PROXY_IMAGE: &str = "ghcr.io/longxingze0925/landscape-singbox:latest";
const FLOW_TPROXY_PORT_BASE: u32 = 12000;
const DEFAULT_FLOW_TPROXY_PORT: u16 = FLOW_TPROXY_PORT_BASE as u16;
const NODE_TEST_PORT_BASE: u32 = 12100;
const LATENCY_TEST_TIMEOUT_SECS: u64 = 8;
const CHINA_LATENCY_TEST_URL: &str = "https://www.baidu.com";
const GLOBAL_LATENCY_TEST_URL: &str = "https://www.gstatic.com/generate_204";

#[derive(Clone)]
pub struct ProxyRuntimeService {
    node_service: ProxyNodeService,
    flow_rule_service: FlowRuleService,
    home_path: PathBuf,
    image: String,
}

impl ProxyRuntimeService {
    pub fn new(
        node_service: ProxyNodeService,
        flow_rule_service: FlowRuleService,
        home_path: PathBuf,
    ) -> Self {
        let image = std::env::var("LANDSCAPE_PROXY_IMAGE")
            .ok()
            .filter(|image| !image.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_PROXY_IMAGE.to_string());
        Self { node_service, flow_rule_service, home_path, image }
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
            self.sync_all_nodes().await?;
            return Err(ProxyError::NodeDisabled(node_id));
        }

        self.sync_all_nodes().await?;
        self.status_for_node(&node).await
    }

    pub async fn stop_node(&self, node_id: Uuid) -> Result<ProxyNodeRuntimeStatus, ProxyError> {
        let node =
            self.node_service.find_by_id(node_id).await.ok_or(ProxyError::NodeNotFound(node_id))?;
        let container_name = PROXY_RUNTIME_CONTAINER_NAME;

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
        let container_name = PROXY_RUNTIME_CONTAINER_NAME;

        let docker = docker()?;
        remove_container_if_exists(&docker, &container_name).await?;

        self.status_for_node(&node).await
    }

    pub async fn sync_runtime(&self) -> Result<(), ProxyError> {
        self.sync_all_nodes().await
    }

    pub async fn test_latency(
        &self,
        request: ProxyLatencyTestRequest,
    ) -> Result<Vec<ProxyLatencyTestResult>, ProxyError> {
        let mut nodes = self.sorted_nodes().await;
        if !request.node_ids.is_empty() {
            let ids: std::collections::HashSet<Uuid> = request.node_ids.into_iter().collect();
            nodes.retain(|node| ids.contains(&node.id));
        }

        let targets = if request.targets.is_empty() {
            vec![ProxyLatencyTestTarget::China, ProxyLatencyTestTarget::Global]
        } else {
            request.targets
        };

        let enabled_nodes = self.enabled_sorted_nodes().await;
        let mut port_map = std::collections::HashMap::new();
        for (index, node) in enabled_nodes.iter().enumerate() {
            port_map.insert(node.id, node_test_port(index)?);
        }
        let mut results = Vec::new();
        let docker = match docker() {
            Ok(docker) => docker,
            Err(err) => {
                for node in nodes {
                    for target in &targets {
                        results.push(latency_result(
                            &node,
                            target.clone(),
                            ProxyLatencyTestState::RuntimeMissing,
                            None,
                            Some(err.to_string()),
                        ));
                    }
                }
                return Ok(results);
            }
        };

        let runtime_state = self.runtime_container_state(&docker).await?;
        if runtime_state != ProxyRuntimeState::Running {
            for node in nodes {
                for target in &targets {
                    results.push(latency_result(
                        &node,
                        target.clone(),
                        ProxyLatencyTestState::RuntimeMissing,
                        None,
                        Some(format!("proxy runtime is {runtime_state:?}")),
                    ));
                }
            }
            return Ok(results);
        }

        for node in nodes {
            if !node.enable {
                for target in &targets {
                    results.push(latency_result(
                        &node,
                        target.clone(),
                        ProxyLatencyTestState::Disabled,
                        None,
                        None,
                    ));
                }
                continue;
            }

            let Some(&port) = port_map.get(&node.id) else {
                for target in &targets {
                    results.push(latency_result(
                        &node,
                        target.clone(),
                        ProxyLatencyTestState::Failed,
                        None,
                        Some("missing test port mapping".to_string()),
                    ));
                }
                continue;
            };

            for target in &targets {
                let result =
                    self.test_node_target_latency(&docker, &node, port, target.clone()).await;
                results.push(result);
            }
        }

        Ok(results)
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

    async fn sync_all_nodes(&self) -> Result<(), ProxyError> {
        let nodes: Vec<ProxyNodeConfig> =
            self.enabled_sorted_nodes().await;
        if nodes.is_empty() {
            self.remove_runtime_container_and_legacy_containers().await?;
            return Ok(());
        }

        let flows = self.flow_rule_service.list().await;
        self.write_config(&nodes, &flows).await?;
        self.recreate_container().await?;
        Ok(())
    }

    async fn runtime_container_state(&self, docker: &Docker) -> Result<ProxyRuntimeState, ProxyError> {
        match docker
            .inspect_container(PROXY_RUNTIME_CONTAINER_NAME, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => {
                let status = info.state.as_ref().and_then(|state| state.status);
                Ok(match status {
                    Some(ContainerStateStatusEnum::RUNNING) => ProxyRuntimeState::Running,
                    Some(ContainerStateStatusEnum::CREATED) => ProxyRuntimeState::Created,
                    Some(ContainerStateStatusEnum::EXITED)
                    | Some(ContainerStateStatusEnum::DEAD) => ProxyRuntimeState::Exited,
                    _ => ProxyRuntimeState::Unknown,
                })
            }
            Err(DockerBollardError::DockerResponseServerError { status_code, .. })
                if status_code == 404 =>
            {
                Ok(ProxyRuntimeState::Missing)
            }
            Err(err) => Err(ProxyError::RuntimeDockerError(err.to_string())),
        }
    }

    async fn test_node_target_latency(
        &self,
        docker: &Docker,
        node: &ProxyNodeConfig,
        port: u16,
        target: ProxyLatencyTestTarget,
    ) -> ProxyLatencyTestResult {
        let url = latency_target_url(&target);
        let proxy = format!("http://127.0.0.1:{port}");
        let cmd = vec![
            "curl".to_string(),
            "-L".to_string(),
            "-sS".to_string(),
            "-o".to_string(),
            "/dev/null".to_string(),
            "-w".to_string(),
            "%{time_total}".to_string(),
            "--max-time".to_string(),
            LATENCY_TEST_TIMEOUT_SECS.to_string(),
            "-x".to_string(),
            proxy,
            url.to_string(),
        ];

        match tokio::time::timeout(
            Duration::from_secs(LATENCY_TEST_TIMEOUT_SECS + 2),
            exec_container_command(docker, PROXY_RUNTIME_CONTAINER_NAME, cmd),
        )
        .await
        {
            Err(_) => latency_result(
                node,
                target,
                ProxyLatencyTestState::Timeout,
                None,
                Some("latency test timed out".to_string()),
            ),
            Ok(Err(err)) => latency_result(
                node,
                target,
                ProxyLatencyTestState::Failed,
                None,
                Some(err.to_string()),
            ),
            Ok(Ok(output)) => parse_curl_latency_output(node, target, output),
        }
    }

    async fn enabled_sorted_nodes(&self) -> Vec<ProxyNodeConfig> {
        let mut nodes: Vec<ProxyNodeConfig> =
            self.node_service.list().await.into_iter().filter(|node| node.enable).collect();
        nodes.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        nodes
    }

    async fn sorted_nodes(&self) -> Vec<ProxyNodeConfig> {
        let mut nodes = self.node_service.list().await;
        nodes.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        nodes
    }

    async fn recreate_container(&self) -> Result<(), ProxyError> {
        let container_name = PROXY_RUNTIME_CONTAINER_NAME;
        let docker = docker()?;

        ensure_image_available(&docker, &self.image).await?;
        self.remove_legacy_node_containers(&docker).await?;
        remove_container_if_exists(&docker, container_name).await?;

        let container_config = self.container_config()?;
        docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(container_name.to_string()),
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

    fn container_config(&self) -> Result<ContainerCreateBody, ProxyError> {
        let config_path = self.config_path();
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
                format!("LAND_PROXY_SERVER_PORT={DEFAULT_FLOW_TPROXY_PORT}"),
                "LAND_PROXY_HANDLE_MODE=multiple_tproxy".to_string(),
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

    async fn write_config(
        &self,
        nodes: &[ProxyNodeConfig],
        flows: &[FlowConfig],
    ) -> Result<(), ProxyError> {
        let dir = self.config_dir();
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|err| ProxyError::RuntimeConfigError(err.to_string()))?;
        let config = build_sing_box_config(nodes, flows)?;
        let bytes = serde_json::to_vec_pretty(&config)
            .map_err(|err| ProxyError::RuntimeConfigError(err.to_string()))?;
        tokio::fs::write(self.config_path(), bytes)
            .await
            .map_err(|err| ProxyError::RuntimeConfigError(err.to_string()))?;
        Ok(())
    }

    fn config_dir(&self) -> PathBuf {
        self.home_path.join("proxy")
    }

    fn config_path(&self) -> PathBuf {
        self.config_dir().join("runtime.json")
    }

    async fn remove_runtime_container_and_legacy_containers(&self) -> Result<(), ProxyError> {
        let docker = docker()?;
        self.remove_legacy_node_containers(&docker).await?;
        remove_container_if_exists(&docker, PROXY_RUNTIME_CONTAINER_NAME).await?;
        Ok(())
    }

    async fn remove_legacy_node_containers(&self, docker: &Docker) -> Result<(), ProxyError> {
        for node in self.node_service.list().await {
            let legacy_name = format!("{PROXY_CONTAINER_NAME_PREFIX}{}", node.id.simple());
            if legacy_name != PROXY_RUNTIME_CONTAINER_NAME {
                remove_container_if_exists(docker, &legacy_name).await?;
            }
        }
        Ok(())
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

fn build_sing_box_config(
    nodes: &[ProxyNodeConfig],
    flows: &[FlowConfig],
) -> Result<Value, ProxyError> {
    let mut outbounds = Vec::with_capacity(nodes.len() + 2);
    let mut proxy_tags = Vec::with_capacity(nodes.len());
    let node_tags: std::collections::HashMap<Uuid, String> =
        nodes.iter().map(|node| (node.id, node_outbound_tag(node.id))).collect();

    for node in nodes {
        let tag = node_outbound_tag(node.id);
        proxy_tags.push(tag.clone());
        outbounds.push(build_outbound(node, &tag)?);
    }

    outbounds.push(json!({
        "type": "selector",
        "tag": "proxy",
        "outbounds": proxy_tags,
        "default": node_outbound_tag(nodes[0].id)
    }));
    outbounds.push(json!({
        "type": "direct",
        "tag": "direct"
    }));

    let mut inbounds = vec![json!({
        "type": "tproxy",
        "tag": "landscape-in",
        "listen": "::",
        "listen_port": DEFAULT_FLOW_TPROXY_PORT
    })];
    let mut route_rules = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let inbound_tag = node_test_inbound_tag(node.id);
        inbounds.push(json!({
            "type": "mixed",
            "tag": inbound_tag,
            "listen": "127.0.0.1",
            "listen_port": node_test_port(index)?
        }));
        route_rules.push(json!({
            "inbound": [inbound_tag],
            "outbound": node_outbound_tag(node.id)
        }));
    }

    for flow in flows.iter().filter(|flow| flow.enable) {
        let flow_proxy_tags = proxy_tags_for_flow(flow, &node_tags);
        if flow_proxy_tags.is_empty() {
            continue;
        }

        let inbound_tag = flow_inbound_tag(flow.flow_id);
        let listen_port = flow_tproxy_port(flow.flow_id)?;
        inbounds.push(json!({
            "type": "tproxy",
            "tag": inbound_tag,
            "listen": "::",
            "listen_port": listen_port
        }));

        let outbound = if flow_proxy_tags.len() == 1 {
            flow_proxy_tags[0].clone()
        } else {
            let selector_tag = flow_selector_tag(flow.flow_id);
            outbounds.push(json!({
                "type": "selector",
                "tag": selector_tag,
                "outbounds": flow_proxy_tags,
                "default": flow_proxy_tags[0]
            }));
            selector_tag
        };
        route_rules.push(json!({
            "inbound": [inbound_tag],
            "outbound": outbound
        }));
    }

    Ok(json!({
        "log": {
            "level": "info",
            "timestamp": true
        },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "route": {
            "rules": route_rules,
            "final": "proxy"
        }
    }))
}

fn flow_tproxy_port(flow_id: u32) -> Result<u16, ProxyError> {
    if flow_id > u8::MAX as u32 {
        return Err(ProxyError::RuntimeConfigError(format!(
            "flow id {flow_id} is too large for proxy tproxy routing"
        )));
    }

    let port = FLOW_TPROXY_PORT_BASE + flow_id;
    u16::try_from(port).map_err(|_| {
        ProxyError::RuntimeConfigError(format!("flow id {flow_id} maps to invalid proxy port"))
    })
}

fn node_test_port(index: usize) -> Result<u16, ProxyError> {
    let port = NODE_TEST_PORT_BASE
        + u32::try_from(index).map_err(|_| {
            ProxyError::RuntimeConfigError("proxy node index overflow".to_string())
        })?;
    u16::try_from(port).map_err(|_| {
        ProxyError::RuntimeConfigError(format!("proxy node index {index} maps to invalid test port"))
    })
}

fn flow_inbound_tag(flow_id: u32) -> String {
    format!("flow-{flow_id}")
}

fn flow_selector_tag(flow_id: u32) -> String {
    format!("flow-proxy-{flow_id}")
}

fn node_outbound_tag(node_id: Uuid) -> String {
    format!("proxy-{}", node_id.simple())
}

fn node_test_inbound_tag(node_id: Uuid) -> String {
    format!("test-{}", node_id.simple())
}

fn proxy_tags_for_flow(
    flow: &FlowConfig,
    node_tags: &std::collections::HashMap<Uuid, String>,
) -> Vec<String> {
    flow.flow_targets
        .iter()
        .filter(|target| target.weight > 0)
        .filter_map(|target| match target.target {
            FlowTarget::Proxy { node_id, .. } => node_tags.get(&node_id).cloned(),
            _ => None,
        })
        .collect()
}

fn build_outbound(node: &ProxyNodeConfig, tag: &str) -> Result<Value, ProxyError> {
    let mut base = Map::from_iter([
        ("tag".to_string(), json!(tag)),
        ("server".to_string(), json!(node.server)),
        ("server_port".to_string(), json!(node.port)),
    ]);

    match &node.protocol {
        ProxyProtocolConfig::Vless {
            uuid,
            flow,
            tls,
            server_name,
            reality,
            reality_public_key,
            reality_short_id,
            utls_fingerprint,
        } => {
            base.insert("type".to_string(), json!("vless"));
            base.insert("uuid".to_string(), json!(uuid));
            if let Some(flow) = non_empty_opt(flow) {
                base.insert("flow".to_string(), json!(flow));
            }
            if *tls || *reality {
                base.insert(
                    "tls".to_string(),
                    tls_config(
                        server_name,
                        *reality,
                        reality_public_key,
                        reality_short_id,
                        utls_fingerprint,
                    ),
                );
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
                base.insert("tls".to_string(), tls_config(server_name, false, &None, &None, &None));
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

fn latency_target_url(target: &ProxyLatencyTestTarget) -> &'static str {
    match target {
        ProxyLatencyTestTarget::China => CHINA_LATENCY_TEST_URL,
        ProxyLatencyTestTarget::Global => GLOBAL_LATENCY_TEST_URL,
    }
}

fn latency_result(
    node: &ProxyNodeConfig,
    target: ProxyLatencyTestTarget,
    state: ProxyLatencyTestState,
    latency_ms: Option<u32>,
    error: Option<String>,
) -> ProxyLatencyTestResult {
    ProxyLatencyTestResult {
        node_id: node.id,
        node_name: node.name.clone(),
        target,
        state,
        latency_ms,
        tested_at: get_f64_timestamp(),
        error,
    }
}

fn parse_curl_latency_output(
    node: &ProxyNodeConfig,
    target: ProxyLatencyTestTarget,
    output: String,
) -> ProxyLatencyTestResult {
    let trimmed = output.trim();
    match trimmed.parse::<f64>() {
        Ok(seconds) => latency_result(
            node,
            target,
            ProxyLatencyTestState::Success,
            Some((seconds * 1000.0).round() as u32),
            None,
        ),
        Err(_) => {
            let lower = trimmed.to_lowercase();
            let state = if lower.contains("timed out") || lower.contains("timeout") {
                ProxyLatencyTestState::Timeout
            } else {
                ProxyLatencyTestState::Failed
            };
            latency_result(node, target, state, None, Some(trimmed.to_string()))
        }
    }
}

async fn exec_container_command(
    docker: &Docker,
    container_name: &str,
    cmd: Vec<String>,
) -> Result<String, ProxyError> {
    let exec = docker
        .create_exec(
            container_name,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(cmd),
                ..Default::default()
            },
        )
        .await
        .map_err(|err| ProxyError::LatencyTestError(err.to_string()))?;

    let start = docker
        .start_exec(&exec.id, None::<StartExecOptions>)
        .await
        .map_err(|err| ProxyError::LatencyTestError(err.to_string()))?;

    let StartExecResults::Attached { mut output, .. } = start else {
        return Err(ProxyError::LatencyTestError("docker exec did not attach output".into()));
    };

    let mut bytes = Vec::new();
    while let Some(chunk) = output.next().await {
        let chunk = chunk.map_err(|err| ProxyError::LatencyTestError(err.to_string()))?;
        match chunk {
            LogOutput::StdOut { message }
            | LogOutput::StdErr { message }
            | LogOutput::Console { message } => bytes.extend_from_slice(&message),
            LogOutput::StdIn { .. } => {}
        }
    }

    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn non_empty_opt(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|value| !value.is_empty())
}

fn tls_config(
    server_name: &Option<String>,
    reality: bool,
    reality_public_key: &Option<String>,
    reality_short_id: &Option<String>,
    utls_fingerprint: &Option<String>,
) -> Value {
    let mut tls = Map::from_iter([("enabled".to_string(), json!(true))]);
    if let Some(server_name) = non_empty_opt(server_name) {
        tls.insert("server_name".to_string(), json!(server_name));
    }
    if reality {
        let mut reality_config = Map::from_iter([("enabled".to_string(), json!(true))]);
        if let Some(public_key) = non_empty_opt(reality_public_key) {
            reality_config.insert("public_key".to_string(), json!(public_key));
        }
        if let Some(short_id) = non_empty_opt(reality_short_id) {
            reality_config.insert("short_id".to_string(), json!(short_id));
        }
        tls.insert("reality".to_string(), Value::Object(reality_config));
    }
    if let Some(fingerprint) = non_empty_opt(utls_fingerprint) {
        tls.insert(
            "utls".to_string(),
            json!({
                "enabled": true,
                "fingerprint": fingerprint,
            }),
        );
    }
    Value::Object(tls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use landscape_common::{flow::WeightedFlowTarget, proxy::ProxyMode};

    #[test]
    fn sing_box_config_routes_flow_inbound_to_selected_proxy_node() {
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let nodes = vec![proxy_node(first_id, "first"), proxy_node(second_id, "second")];
        let flows = vec![FlowConfig {
            id: Uuid::new_v4(),
            enable: true,
            flow_id: 7,
            flow_match_rules: vec![],
            flow_targets: vec![WeightedFlowTarget::new(
                FlowTarget::Proxy { node_id: second_id, mode: ProxyMode::Global },
                1,
            )],
            remark: String::new(),
            update_at: 0.0,
        }];

        let config = build_sing_box_config(&nodes, &flows).expect("build sing-box config");

        let inbounds = config["inbounds"].as_array().expect("inbounds");
        assert!(inbounds.iter().any(|inbound| {
            inbound["tag"] == "flow-7" && inbound["listen_port"] == FLOW_TPROXY_PORT_BASE + 7
        }));

        let route_rules = config["route"]["rules"].as_array().expect("route rules");
        assert!(route_rules.iter().any(|rule| {
            rule["inbound"] == json!(["flow-7"]) && rule["outbound"] == node_outbound_tag(second_id)
        }));
    }

    fn proxy_node(id: Uuid, name: &str) -> ProxyNodeConfig {
        ProxyNodeConfig {
            id,
            name: name.to_string(),
            enable: true,
            server: "127.0.0.1".to_string(),
            port: 1080,
            protocol: ProxyProtocolConfig::Socks5 { username: None, password: None },
            remark: String::new(),
            update_at: 0.0,
        }
    }
}
