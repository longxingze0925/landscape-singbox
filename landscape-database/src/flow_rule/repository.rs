use std::collections::{HashMap, HashSet};

use landscape_common::config::FlowId;
use landscape_common::error::LdError;
use landscape_common::flow::config::FlowConfig;
use landscape_common::flow::{
    FlowEntryMatchMode, FlowEntryRule, FlowTarget, ResolvedFlowEntryMatchMode,
    ResolvedFlowEntryRule, RuntimeFlowConfig,
};
use landscape_common::proxy::{
    proxy_container_name, proxy_node_id_from_container_name, PROXY_RUNTIME_CONTAINER_NAME,
};
use migration::Expr;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::enrolled_device::repository::EnrolledDeviceRepository;
use crate::flow_rule::entity::Column;
use crate::repository::Repository;
use crate::DBId;

use super::entity::{FlowConfigActiveModel, FlowConfigEntity, FlowConfigModel};

#[derive(Clone)]
pub struct FlowConfigRepository {
    db: DatabaseConnection,
}

impl FlowConfigRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn list_runtime_configs(&self) -> Result<Vec<RuntimeFlowConfig>, LdError> {
        let configs = self.list_all().await?;
        let devices = self.load_devices_for_configs(&configs).await?;
        let mut result = Vec::new();

        for config in configs.into_iter().filter(|config| config.enable) {
            let flow_match_rules = config
                .flow_match_rules
                .into_iter()
                .filter_map(|rule| resolve_flow_entry_rule(rule, &devices))
                .collect();

            result.push(RuntimeFlowConfig { flow_id: config.flow_id, flow_match_rules });
        }

        Ok(result)
    }

    async fn load_devices_for_configs(
        &self,
        configs: &[FlowConfig],
    ) -> Result<DevicesById, LdError> {
        let mut device_ids = HashSet::new();
        for config in configs {
            for rule in &config.flow_match_rules {
                if let FlowEntryMatchMode::Device { device_id } = rule.mode {
                    device_ids.insert(device_id);
                }
            }
        }

        let devices = EnrolledDeviceRepository::new(self.db.clone())
            .find_by_ids(device_ids.into_iter().collect())
            .await;
        Ok(devices.into_iter().map(|device| (device.id, device)).collect())
    }

    pub async fn find_by_flow_id(&self, flow_id: FlowId) -> Result<Option<FlowConfig>, LdError> {
        let result =
            FlowConfigEntity::find().filter(Column::FlowId.eq(flow_id)).one(&self.db).await?;

        Ok(result.map(From::from))
    }

    /// 查询是否有其他 flow config（排除 exclude_id）包含相同的入口匹配规则
    pub async fn find_conflict_by_entry_mode(
        &self,
        exclude_id: DBId,
        mode: &FlowEntryMatchMode,
    ) -> Result<Option<FlowConfig>, LdError> {
        let (condition_sql, params) = match mode {
            FlowEntryMatchMode::Mac { mac_addr } => (
                "json_extract(json_each.value, '$.mode.t') = 'mac' AND json_extract(json_each.value, '$.mode.mac_addr') = ?",
                vec![sea_orm::Value::String(Some(Box::new(mac_addr.to_string())))],
            ),
            FlowEntryMatchMode::Ip { ip, prefix_len } => (
                "json_extract(json_each.value, '$.mode.t') = 'ip' AND json_extract(json_each.value, '$.mode.ip') = ? AND json_extract(json_each.value, '$.mode.prefix_len') = ?",
                vec![
                    sea_orm::Value::String(Some(Box::new(ip.to_string()))),
                    sea_orm::Value::Int(Some(*prefix_len as i32)),
                ],
            ),
            FlowEntryMatchMode::Device { device_id } => (
                "json_extract(json_each.value, '$.mode.t') = 'device' AND json_extract(json_each.value, '$.mode.device_id') = ?",
                vec![sea_orm::Value::String(Some(Box::new(device_id.to_string())))],
            ),
        };

        let full_sql = format!(
            "EXISTS (
            SELECT 1 FROM json_each(flow_match_rules)
            WHERE {}
        )",
            condition_sql
        );

        let expr = Expr::cust_with_values(&full_sql, params);

        let result = FlowConfigEntity::find()
            .filter(Column::Id.ne(exclude_id))
            .filter(expr)
            .one(&self.db)
            .await?;

        Ok(result.map(From::from))
    }

    pub async fn find_by_target(&self, t: FlowTarget) -> Result<Vec<FlowConfig>, LdError> {
        // 构造条件 SQL 和参数
        let (condition_sql, params) = build_target_match_condition(t);

        let full_sql = format!(
            "EXISTS (
            SELECT 1 FROM json_each(packet_handle_iface_name)
            WHERE {}
        )",
            condition_sql
        );

        let expr = Expr::cust_with_values(&full_sql, params);

        // 查询执行
        let result = FlowConfigEntity::find().filter(expr).all(&self.db).await?;

        Ok(result.into_iter().map(From::from).collect())
    }

    pub async fn find_resolved_conflict_by_entry_mode(
        &self,
        exclude_id: DBId,
        mode: &FlowEntryMatchMode,
    ) -> Result<Option<FlowConfig>, LdError> {
        let configs = self.list_all().await?;
        let mut devices = self.load_devices_for_configs(&configs).await?;
        if let FlowEntryMatchMode::Device { device_id } = mode {
            if !devices.contains_key(device_id) {
                if let Some(device) =
                    EnrolledDeviceRepository::new(self.db.clone()).find_by_id(*device_id).await?
                {
                    devices.insert(device.id, device);
                }
            }
        }
        let Some(target_mode) = resolve_flow_entry_mode(mode.clone(), &devices) else {
            return Ok(None);
        };

        for config in configs {
            if config.id == exclude_id {
                continue;
            }

            for rule in &config.flow_match_rules {
                if let Some(mode) = resolve_flow_entry_mode(rule.mode.clone(), &devices) {
                    if mode == target_mode {
                        return Ok(Some(config));
                    }
                }
            }
        }

        Ok(None)
    }

    pub async fn find_resolved_conflict_for_modes(
        &self,
        exclude_id: DBId,
        modes: &[FlowEntryMatchMode],
    ) -> Result<Option<(FlowEntryMatchMode, FlowConfig)>, LdError> {
        let configs = self.list_all().await?;
        let mut devices = self.load_devices_for_configs(&configs).await?;
        devices.extend(self.load_devices_for_modes(modes).await?);

        if let Some(device_id) = find_missing_device_id(modes.iter(), &devices) {
            return Err(LdError::ConfigError(format!("flow device target {device_id} not found")));
        }

        for mode in modes {
            let Some(resolved) = resolve_flow_entry_mode(mode.clone(), &devices) else {
                continue;
            };
            for config in &configs {
                if config.id == exclude_id {
                    continue;
                }
                for rule in &config.flow_match_rules {
                    if let Some(rule_mode) = resolve_flow_entry_mode(rule.mode.clone(), &devices) {
                        if rule_mode == resolved {
                            return Ok(Some((mode.clone(), config.clone())));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    pub async fn resolve_modes(
        &self,
        modes: &[FlowEntryMatchMode],
    ) -> Result<Vec<(FlowEntryMatchMode, ResolvedFlowEntryMatchMode)>, LdError> {
        let devices = self.load_devices_for_modes(modes).await?;

        if let Some(device_id) = find_missing_device_id(modes.iter(), &devices) {
            return Err(LdError::ConfigError(format!("flow device target {device_id} not found")));
        }

        Ok(modes
            .iter()
            .filter_map(|mode| {
                resolve_flow_entry_mode(mode.clone(), &devices).and_then(|resolved| {
                    into_resolved_match_mode(resolved).map(|resolved| (mode.clone(), resolved))
                })
            })
            .collect())
    }

    async fn load_devices_for_modes(
        &self,
        modes: &[FlowEntryMatchMode],
    ) -> Result<DevicesById, LdError> {
        let device_ids = collect_device_ids(modes.iter());
        let devices = EnrolledDeviceRepository::new(self.db.clone())
            .find_by_ids(device_ids.into_iter().collect())
            .await;
        Ok(devices.into_iter().map(|device| (device.id, device)).collect())
    }

    pub async fn validate_modes_resolvable(
        &self,
        modes: &[FlowEntryMatchMode],
    ) -> Result<(), LdError> {
        let devices = self.load_devices_for_modes(modes).await?;

        if let Some(device_id) = find_missing_device_id(modes.iter(), &devices) {
            return Err(LdError::ConfigError(format!("flow device target {device_id} not found")));
        }

        Ok(())
    }
}

type DevicesById =
    std::collections::HashMap<DBId, landscape_common::enrolled_device::EnrolledDevice>;

fn collect_device_ids<'a>(
    modes: impl IntoIterator<Item = &'a FlowEntryMatchMode>,
) -> HashSet<DBId> {
    let mut device_ids = HashSet::new();
    for mode in modes {
        if let FlowEntryMatchMode::Device { device_id } = mode {
            device_ids.insert(*device_id);
        }
    }
    device_ids
}

fn find_missing_device_id<'a>(
    modes: impl IntoIterator<Item = &'a FlowEntryMatchMode>,
    devices: &DevicesById,
) -> Option<DBId> {
    for mode in modes {
        if let FlowEntryMatchMode::Device { device_id } = mode {
            if !devices.contains_key(device_id) {
                return Some(*device_id);
            }
        }
    }

    None
}

fn resolve_flow_entry_rule(
    rule: FlowEntryRule,
    devices: &DevicesById,
) -> Option<ResolvedFlowEntryRule> {
    match rule.mode {
        FlowEntryMatchMode::Device { device_id } => {
            let device = devices.get(&device_id)?;
            Some(ResolvedFlowEntryRule {
                qos: rule.qos,
                mode: ResolvedFlowEntryMatchMode::Mac { mac_addr: device.mac },
            })
        }
        FlowEntryMatchMode::Mac { mac_addr } => Some(ResolvedFlowEntryRule {
            qos: rule.qos,
            mode: ResolvedFlowEntryMatchMode::Mac { mac_addr },
        }),
        FlowEntryMatchMode::Ip { ip, prefix_len } => Some(ResolvedFlowEntryRule {
            qos: rule.qos,
            mode: ResolvedFlowEntryMatchMode::Ip { ip, prefix_len },
        }),
    }
}

fn resolve_flow_entry_mode(
    mode: FlowEntryMatchMode,
    devices: &DevicesById,
) -> Option<FlowEntryMatchMode> {
    match mode {
        FlowEntryMatchMode::Device { device_id } => {
            let device = devices.get(&device_id)?;
            Some(FlowEntryMatchMode::Mac { mac_addr: device.mac })
        }
        mode => Some(mode),
    }
}

fn into_resolved_match_mode(mode: FlowEntryMatchMode) -> Option<ResolvedFlowEntryMatchMode> {
    match mode {
        FlowEntryMatchMode::Mac { mac_addr } => Some(ResolvedFlowEntryMatchMode::Mac { mac_addr }),
        FlowEntryMatchMode::Ip { ip, prefix_len } => {
            Some(ResolvedFlowEntryMatchMode::Ip { ip, prefix_len })
        }
        FlowEntryMatchMode::Device { .. } => None,
    }
}

pub fn find_duplicate_resolved_modes(
    resolved_modes: &[(FlowEntryMatchMode, ResolvedFlowEntryMatchMode)],
) -> Option<FlowEntryMatchMode> {
    let mut seen = HashMap::new();
    for (original_mode, resolved_mode) in resolved_modes {
        if seen.insert(resolved_mode.clone(), original_mode.clone()).is_some() {
            return Some(original_mode.clone());
        }
    }

    None
}

fn build_target_match_condition(t: FlowTarget) -> (String, Vec<sea_orm::Value>) {
    match t {
        FlowTarget::Interface { name } => (
            "json_extract(json_each.value, '$.target.t') = 'interface' AND json_extract(json_each.value, '$.target.name') = ?".to_string(),
            vec![sea_orm::Value::String(Some(Box::new(name)))],
        ),
        FlowTarget::Netns { container_name } => {
            if let Some(node_id) = proxy_node_id_from_container_name(&container_name) {
                (
                    "(
                        (
                            json_extract(json_each.value, '$.target.t') = 'netns'
                            AND json_extract(json_each.value, '$.target.container_name') = ?
                        )
                        OR
                        (
                            json_extract(json_each.value, '$.target.t') = 'proxy'
                            AND json_extract(json_each.value, '$.target.node_id') = ?
                        )
                    )"
                    .to_string(),
                    vec![
                        sea_orm::Value::String(Some(Box::new(container_name))),
                        sea_orm::Value::String(Some(Box::new(node_id.to_string()))),
                    ],
                )
            } else if container_name == PROXY_RUNTIME_CONTAINER_NAME {
                // Proxy runtime uses one shared container for all proxy targets.
                (
                    "(
                        (
                            json_extract(json_each.value, '$.target.t') = 'netns'
                            AND json_extract(json_each.value, '$.target.container_name') = ?
                        )
                        OR
                        (
                            json_extract(json_each.value, '$.target.t') = 'proxy'
                        )
                    )"
                    .to_string(),
                    vec![sea_orm::Value::String(Some(Box::new(container_name)))],
                )
            } else {
                (
                    "json_extract(json_each.value, '$.target.t') = 'netns' AND json_extract(json_each.value, '$.target.container_name') = ?".to_string(),
                    vec![sea_orm::Value::String(Some(Box::new(container_name)))],
                )
            }
        }
        FlowTarget::Proxy { node_id, .. } => (
            "json_extract(json_each.value, '$.target.t') = 'proxy' AND json_extract(json_each.value, '$.target.node_id') = ?".to_string(),
            vec![sea_orm::Value::String(Some(Box::new(node_id.to_string())))],
        ),
    }
}

pub fn resolve_flow_target_for_route(target: &FlowTarget) -> FlowTarget {
    match target {
        FlowTarget::Proxy { node_id, .. } => {
            FlowTarget::Netns { container_name: proxy_container_name(*node_id) }
        }
        target => target.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{find_duplicate_resolved_modes, find_missing_device_id, DevicesById};
    use crate::provider::LandscapeDBServiceProvider;
    use landscape_common::database::LandscapeStore;
    use landscape_common::enrolled_device::EnrolledDevice;
    use landscape_common::flow::{
        config::FlowConfig, FlowEntryMatchMode, FlowTarget, ResolvedFlowEntryMatchMode,
        WeightedFlowTarget,
    };
    use landscape_common::net::MacAddr;
    use landscape_common::proxy::{
        ProxyMode, PROXY_CONTAINER_NAME_PREFIX, PROXY_RUNTIME_CONTAINER_NAME,
    };
    use sea_orm::prelude::Uuid;
    use std::collections::HashMap;

    #[test]
    fn detects_duplicate_resolved_modes() {
        let mac_addr = MacAddr::new(0x00, 0x11, 0x22, 0x33, 0x44, 0x55);
        let resolved = vec![
            (
                FlowEntryMatchMode::Device { device_id: Uuid::new_v4() },
                ResolvedFlowEntryMatchMode::Mac { mac_addr },
            ),
            (FlowEntryMatchMode::Mac { mac_addr }, ResolvedFlowEntryMatchMode::Mac { mac_addr }),
        ];

        let duplicate = find_duplicate_resolved_modes(&resolved);

        assert!(matches!(duplicate, Some(FlowEntryMatchMode::Mac { .. })));
    }

    #[test]
    fn reports_missing_device_targets() {
        let device_id = Uuid::new_v4();
        let modes = vec![FlowEntryMatchMode::Device { device_id }];

        assert_eq!(find_missing_device_id(modes.iter(), &HashMap::new()), Some(device_id));
    }

    #[test]
    fn accepts_known_device_targets() {
        let device_id = Uuid::new_v4();
        let modes = vec![FlowEntryMatchMode::Device { device_id }];
        let devices: DevicesById = HashMap::from([(
            device_id,
            EnrolledDevice {
                id: device_id,
                update_at: 0.0,
                iface_name: None,
                name: "device".to_string(),
                fake_name: None,
                remark: None,
                mac: MacAddr::new(0x00, 0x11, 0x22, 0x33, 0x44, 0x55),
                ipv4: None,
                ipv6: None,
                tag: vec![],
                dhcp_custom_options: vec![],
                dhcp_filter_options: vec![],
            },
        )]);

        assert_eq!(find_missing_device_id(modes.iter(), &devices), None);
    }

    #[test]
    fn proxy_runtime_container_matches_proxy_targets() {
        let (sql, params) = super::build_target_match_condition(FlowTarget::Netns {
            container_name: PROXY_RUNTIME_CONTAINER_NAME.to_string(),
        });

        assert!(sql.contains("json_extract(json_each.value, '$.target.t') = 'proxy'"));
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn ordinary_netns_target_does_not_match_proxy_targets() {
        let (sql, params) = super::build_target_match_condition(FlowTarget::Netns {
            container_name: "ordinary-container".to_string(),
        });

        assert!(!sql.contains("json_extract(json_each.value, '$.target.t') = 'proxy'"));
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn legacy_per_node_proxy_container_matches_that_proxy_node() {
        let node_id = Uuid::new_v4();
        let (sql, params) = super::build_target_match_condition(FlowTarget::Netns {
            container_name: format!("{PROXY_CONTAINER_NAME_PREFIX}{node_id}"),
        });

        assert!(sql.contains("json_extract(json_each.value, '$.target.t') = 'proxy'"));
        assert!(sql.contains("json_extract(json_each.value, '$.target.node_id') = ?"));
        assert_eq!(params.len(), 2);
    }

    #[tokio::test]
    async fn find_by_target_runtime_container_loads_proxy_flow_configs() {
        let provider = LandscapeDBServiceProvider::mem_test_db().await;
        let repo = provider.flow_rule_store();
        let proxy_flow = FlowConfig {
            id: Uuid::new_v4(),
            enable: true,
            flow_id: 7,
            flow_match_rules: vec![],
            flow_targets: vec![WeightedFlowTarget::new(
                FlowTarget::Proxy { node_id: Uuid::new_v4(), mode: ProxyMode::Global },
                1,
            )],
            remark: "proxy".to_string(),
            update_at: 0.0,
        };
        let ordinary_netns_flow = FlowConfig {
            id: Uuid::new_v4(),
            enable: true,
            flow_id: 8,
            flow_match_rules: vec![],
            flow_targets: vec![WeightedFlowTarget::new(
                FlowTarget::Netns { container_name: "ordinary-container".to_string() },
                1,
            )],
            remark: "netns".to_string(),
            update_at: 0.0,
        };

        repo.set(proxy_flow.clone()).await.expect("insert proxy flow");
        repo.set(ordinary_netns_flow).await.expect("insert netns flow");

        let matches = repo
            .find_by_target(FlowTarget::Netns {
                container_name: PROXY_RUNTIME_CONTAINER_NAME.to_string(),
            })
            .await
            .expect("find by proxy runtime container");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, proxy_flow.id);
    }
}

crate::impl_repository!(
    FlowConfigRepository,
    FlowConfigModel,
    FlowConfigEntity,
    FlowConfigActiveModel,
    FlowConfig,
    DBId
);

crate::impl_flow_store!(FlowConfigRepository, FlowConfigModel, FlowConfigEntity);
