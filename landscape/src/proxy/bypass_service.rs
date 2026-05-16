use landscape_common::{
    dns::{
        config::{DnsBindConfig, DnsUpstreamConfig},
        rule::{DNSRuleConfig, FilterResult, RuleSource},
    },
    flow::{config::FlowConfig, mark::FlowMark, FlowTarget},
    geo::{
        GeoConfigKey, GeoFileCacheKey, GeoIpFileFormat, GeoIpSource, GeoIpSourceConfig,
        GeoSiteSource, GeoSiteSourceConfig,
    },
    ip_mark::{WanIPRuleSource, WanIpRuleConfig},
    proxy::{
        ProxyBypassRuleSourceKind, ProxyBypassRuleSourceStatus, ProxyBypassRuleSourcesStatus,
        ProxyError, ProxyMode,
    },
    service::controller::{ConfigController, FlowConfigController},
    utils::id::gen_database_uuid,
    utils::time::get_f64_timestamp,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    dns::{rule_service::DNSRuleService, upstream_service::DnsUpstreamService},
    flow::{dst_ip_rule_service::DstIpRuleService, rule_service::FlowRuleService},
    geo::{ip_service::GeoIpService, site_service::GeoSiteService},
};

const GEO_SOURCE_NAME: &str = "proxy-default";
const GEO_CN_KEY: &str = "CN";
const GEO_SITE_SOURCE_URL: &str =
    "https://github.com/v2fly/domain-list-community/releases/latest/download/dlc.dat";
const GEO_IP_SOURCE_URL: &str = "https://github.com/v2fly/geoip/releases/latest/download/geoip.dat";
const AUTO_DNS_RULE_PREFIX: &str = "__landscape_proxy_bypass_china_dns__";
const AUTO_DST_IP_RULE_PREFIX: &str = "__landscape_proxy_bypass_china_ip__";
const AUTO_RULE_INDEX_BASE: u32 = 1000;
const AUTO_RULE_INDEX_SPAN: u32 = 8000;

#[derive(Clone)]
pub struct ProxyBypassService {
    flow_rule_service: FlowRuleService,
    dns_rule_service: DNSRuleService,
    dst_ip_rule_service: DstIpRuleService,
    dns_upstream_service: DnsUpstreamService,
    geo_site_service: GeoSiteService,
    geo_ip_service: GeoIpService,
}

impl ProxyBypassService {
    pub fn new(
        flow_rule_service: FlowRuleService,
        dns_rule_service: DNSRuleService,
        dst_ip_rule_service: DstIpRuleService,
        dns_upstream_service: DnsUpstreamService,
        geo_site_service: GeoSiteService,
        geo_ip_service: GeoIpService,
    ) -> Self {
        Self {
            flow_rule_service,
            dns_rule_service,
            dst_ip_rule_service,
            dns_upstream_service,
            geo_site_service,
            geo_ip_service,
        }
    }

    pub async fn sync_all(&self) -> Result<(), ProxyError> {
        self.sync_flows(self.flow_rule_service.list().await).await
    }

    pub async fn sync_flow(&self, flow: &FlowConfig) -> Result<(), ProxyError> {
        self.sync_flows(vec![flow.clone()]).await
    }

    pub async fn ensure_flow_ready(&self, flow: &FlowConfig) -> Result<(), ProxyError> {
        if flow.enable && flow_has_bypass_proxy_target(flow) {
            self.ensure_geo_sources_and_cache().await?;
        }
        Ok(())
    }

    pub async fn remove_flow(&self, flow_id: u32) {
        self.remove_generated_rules(flow_id).await;
    }

    pub async fn bypass_rule_sources_status(&self) -> ProxyBypassRuleSourcesStatus {
        self.ensure_geo_site_source().await;
        self.ensure_geo_ip_source().await;

        ProxyBypassRuleSourcesStatus {
            domain: self.domain_rule_source_status().await,
            ip: self.ip_rule_source_status().await,
        }
    }

    pub async fn refresh_bypass_rule_sources(
        &self,
        kind: Option<ProxyBypassRuleSourceKind>,
    ) -> Result<ProxyBypassRuleSourcesStatus, ProxyError> {
        self.ensure_geo_site_source().await;
        self.ensure_geo_ip_source().await;

        match kind {
            Some(ProxyBypassRuleSourceKind::Domain) => self.geo_site_service.refresh(true).await,
            Some(ProxyBypassRuleSourceKind::Ip) => self.geo_ip_service.refresh(true).await,
            None => {
                self.geo_site_service.refresh(true).await;
                self.geo_ip_service.refresh(true).await;
            }
        }

        Ok(self.bypass_rule_sources_status().await)
    }

    async fn sync_flows(&self, flows: Vec<FlowConfig>) -> Result<(), ProxyError> {
        let mut need_geo = false;
        for flow in &flows {
            if flow.enable && flow_has_bypass_proxy_target(flow) {
                need_geo = true;
                break;
            }
        }

        if need_geo {
            self.ensure_geo_sources_and_cache().await?;
        }

        for flow in flows {
            self.sync_flow_inner(&flow).await?;
        }

        Ok(())
    }

    async fn sync_flow_inner(&self, flow: &FlowConfig) -> Result<(), ProxyError> {
        self.remove_generated_rules(flow.flow_id).await;

        if !flow.enable || !flow_has_bypass_proxy_target(flow) {
            return Ok(());
        }

        let upstream_id = self.ensure_dns_upstream().await;
        let geo_key = GeoConfigKey {
            name: GEO_SOURCE_NAME.to_string(),
            key: GEO_CN_KEY.to_string(),
            inverse: false,
            attribute_key: None,
        };
        let index = AUTO_RULE_INDEX_BASE.saturating_add(flow.flow_id % AUTO_RULE_INDEX_SPAN);

        self.dns_rule_service
            .set(DNSRuleConfig {
                id: deterministic_uuid("dns", flow.flow_id),
                name: format!("{AUTO_DNS_RULE_PREFIX} flow={}", flow.flow_id),
                index,
                enable: true,
                filter: FilterResult::default(),
                upstream_id,
                bind_config: DnsBindConfig::default(),
                mark: FlowMark::direct(),
                source: vec![RuleSource::GeoKey(geo_key.clone())],
                flow_id: flow.flow_id,
                update_at: get_f64_timestamp(),
            })
            .await;

        self.dst_ip_rule_service
            .set(WanIpRuleConfig {
                id: deterministic_uuid("ip", flow.flow_id),
                index,
                enable: true,
                mark: FlowMark::direct(),
                source: vec![WanIPRuleSource::GeoKey(geo_key)],
                remark: format!("{AUTO_DST_IP_RULE_PREFIX} flow={}", flow.flow_id),
                flow_id: flow.flow_id,
                override_dns: true,
                update_at: get_f64_timestamp(),
            })
            .await;

        Ok(())
    }

    async fn remove_generated_rules(&self, flow_id: u32) {
        for rule in self.dns_rule_service.list_flow_configs(flow_id).await {
            if rule.name.starts_with(AUTO_DNS_RULE_PREFIX) {
                self.dns_rule_service.delete(rule.id).await;
            }
        }

        for rule in self.dst_ip_rule_service.list_flow_configs(flow_id).await {
            if rule.remark.starts_with(AUTO_DST_IP_RULE_PREFIX) {
                self.dst_ip_rule_service.delete(rule.id).await;
            }
        }
    }

    async fn ensure_dns_upstream(&self) -> Uuid {
        if let Some(upstream) = self.dns_upstream_service.list().await.into_iter().next() {
            return upstream.id;
        }

        let upstream = DnsUpstreamConfig::default();
        let id = upstream.id;
        self.dns_upstream_service.set(upstream).await;
        id
    }

    async fn ensure_geo_sources_and_cache(&self) -> Result<(), ProxyError> {
        self.ensure_geo_site_source().await;
        self.ensure_geo_ip_source().await;

        let cache_key = geo_cn_cache_key();
        if self.geo_site_service.get_cache_value_by_key(&cache_key).await.is_none()
            || self.geo_ip_service.get_cache_value_by_key(&cache_key).await.is_none()
        {
            self.geo_site_service.refresh(true).await;
            self.geo_ip_service.refresh(true).await;
        }

        if self.geo_site_service.get_cache_value_by_key(&cache_key).await.is_none()
            || self.geo_ip_service.get_cache_value_by_key(&cache_key).await.is_none()
        {
            return Err(ProxyError::BypassGeoCacheMissing);
        }

        Ok(())
    }

    async fn ensure_geo_site_source(&self) {
        let exists = self
            .geo_site_service
            .query_geo_by_name(Some(GEO_SOURCE_NAME.to_string()))
            .await
            .into_iter()
            .any(|config| config.name == GEO_SOURCE_NAME);
        if exists {
            self.repair_geo_site_source().await;
            return;
        }

        self.geo_site_service
            .set(GeoSiteSourceConfig {
                id: gen_database_uuid(),
                update_at: get_f64_timestamp(),
                name: GEO_SOURCE_NAME.to_string(),
                enable: true,
                source: GeoSiteSource::Url {
                    url: GEO_SITE_SOURCE_URL.to_string(),
                    next_update_at: 0.0,
                    geo_keys: vec![GEO_CN_KEY.to_string()],
                },
            })
            .await;
    }

    async fn ensure_geo_ip_source(&self) {
        let exists = self
            .geo_ip_service
            .query_geo_by_name(Some(GEO_SOURCE_NAME.to_string()))
            .await
            .into_iter()
            .any(|config| config.name == GEO_SOURCE_NAME);
        if exists {
            self.repair_geo_ip_source().await;
            return;
        }

        self.geo_ip_service
            .set(GeoIpSourceConfig {
                id: gen_database_uuid(),
                update_at: get_f64_timestamp(),
                name: GEO_SOURCE_NAME.to_string(),
                enable: true,
                source: GeoIpSource::Url {
                    url: GEO_IP_SOURCE_URL.to_string(),
                    next_update_at: 0.0,
                    format: GeoIpFileFormat::Dat,
                    txt_key: None,
                },
            })
            .await;
    }

    async fn repair_geo_site_source(&self) {
        let mut configs = self
            .geo_site_service
            .query_geo_by_name(Some(GEO_SOURCE_NAME.to_string()))
            .await
            .into_iter()
            .filter(|config| config.name == GEO_SOURCE_NAME)
            .collect::<Vec<_>>();
        if let Some(mut config) = configs.pop() {
            let needs_fix = !matches!(
                &config.source,
                GeoSiteSource::Url { url, geo_keys, .. }
                    if url == GEO_SITE_SOURCE_URL
                        && geo_keys.len() == 1
                        && geo_keys.first().map(|value| value.as_str()) == Some(GEO_CN_KEY)
            );
            if needs_fix {
                config.source = GeoSiteSource::Url {
                    url: GEO_SITE_SOURCE_URL.to_string(),
                    next_update_at: 0.0,
                    geo_keys: vec![GEO_CN_KEY.to_string()],
                };
                config.update_at = get_f64_timestamp();
                self.geo_site_service.set(config).await;
            }
        }
    }

    async fn repair_geo_ip_source(&self) {
        let mut configs = self
            .geo_ip_service
            .query_geo_by_name(Some(GEO_SOURCE_NAME.to_string()))
            .await
            .into_iter()
            .filter(|config| config.name == GEO_SOURCE_NAME)
            .collect::<Vec<_>>();
        if let Some(mut config) = configs.pop() {
            let needs_fix = !matches!(
                &config.source,
                GeoIpSource::Url {
                    url,
                    format,
                    txt_key,
                    ..
                } if url == GEO_IP_SOURCE_URL
                    && *format == GeoIpFileFormat::Dat
                    && txt_key.is_none()
            );
            if needs_fix {
                config.source = GeoIpSource::Url {
                    url: GEO_IP_SOURCE_URL.to_string(),
                    next_update_at: 0.0,
                    format: GeoIpFileFormat::Dat,
                    txt_key: None,
                };
                config.update_at = get_f64_timestamp();
                self.geo_ip_service.set(config).await;
            }
        }
    }

    async fn domain_rule_source_status(&self) -> ProxyBypassRuleSourceStatus {
        let source_config = self
            .geo_site_service
            .query_geo_by_name(Some(GEO_SOURCE_NAME.to_string()))
            .await
            .into_iter()
            .find(|config| config.name == GEO_SOURCE_NAME);
        let cache = self.geo_site_service.get_cache_value_by_key(&geo_cn_cache_key()).await;
        let refresh_status = self.geo_site_service.get_refresh_status(GEO_SOURCE_NAME).await;

        let (url, next_update_at) = match source_config.as_ref().map(|config| &config.source) {
            Some(GeoSiteSource::Url { url, next_update_at, .. })
            | Some(GeoSiteSource::AdguardHome { url, next_update_at, .. }) => {
                (Some(url.clone()), Some(*next_update_at))
            }
            Some(GeoSiteSource::Direct { .. }) | None => (None, None),
        };
        let item_count = cache.as_ref().map(|cache| cache.values.len()).unwrap_or_default();

        ProxyBypassRuleSourceStatus {
            kind: ProxyBypassRuleSourceKind::Domain,
            name: GEO_SOURCE_NAME.to_string(),
            key: GEO_CN_KEY.to_string(),
            url,
            next_update_at,
            cache_exists: cache.is_some(),
            item_count,
            last_success_at: refresh_status.last_success_at,
            last_error: refresh_status.last_error,
        }
    }

    async fn ip_rule_source_status(&self) -> ProxyBypassRuleSourceStatus {
        let source_config = self
            .geo_ip_service
            .query_geo_by_name(Some(GEO_SOURCE_NAME.to_string()))
            .await
            .into_iter()
            .find(|config| config.name == GEO_SOURCE_NAME);
        let cache = self.geo_ip_service.get_cache_value_by_key(&geo_cn_cache_key()).await;
        let refresh_status = self.geo_ip_service.get_refresh_status(GEO_SOURCE_NAME).await;

        let (url, next_update_at) = match source_config.as_ref().map(|config| &config.source) {
            Some(GeoIpSource::Url { url, next_update_at, .. }) => {
                (Some(url.clone()), Some(*next_update_at))
            }
            Some(GeoIpSource::Direct { .. }) | None => (None, None),
        };
        let item_count = cache.as_ref().map(|cache| cache.values.len()).unwrap_or_default();

        ProxyBypassRuleSourceStatus {
            kind: ProxyBypassRuleSourceKind::Ip,
            name: GEO_SOURCE_NAME.to_string(),
            key: GEO_CN_KEY.to_string(),
            url,
            next_update_at,
            cache_exists: cache.is_some(),
            item_count,
            last_success_at: refresh_status.last_success_at,
            last_error: refresh_status.last_error,
        }
    }
}

fn flow_has_bypass_proxy_target(flow: &FlowConfig) -> bool {
    flow.flow_targets.iter().any(|target| {
        matches!(target.target, FlowTarget::Proxy { mode: ProxyMode::BypassChina, .. })
            && target.weight > 0
    })
}

fn deterministic_uuid(kind: &str, flow_id: u32) -> Uuid {
    let digest = Sha256::digest(format!("landscape-proxy-bypass-{kind}-{flow_id}"));
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn geo_cn_cache_key() -> GeoFileCacheKey {
    GeoFileCacheKey {
        name: GEO_SOURCE_NAME.to_string(),
        key: GEO_CN_KEY.to_string(),
    }
}
