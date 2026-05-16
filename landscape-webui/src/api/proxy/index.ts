import {
  addProxyNode,
  delProxyNode,
  getBypassRuleSourcesStatus,
  getProxyNode,
  getProxyNodes,
  getProxyRuntimeStatuses,
  refreshBypassDomainRuleSource,
  refreshBypassIpRuleSource,
  refreshBypassRuleSources,
  removeProxyRuntime,
  stopProxyRuntime,
  syncProxyRuntime,
  testProxyLatency,
} from "@landscape-router/types/api/proxy/proxy";
import type {
  ProxyBypassRuleSourcesStatus,
  ProxyLatencyTestRequest,
  ProxyLatencyTestResult,
  ProxyNodeConfig,
  ProxyNodeRuntimeStatus,
} from "@landscape-router/types/api/schemas";

export async function get_proxy_nodes(): Promise<ProxyNodeConfig[]> {
  return getProxyNodes();
}

export async function get_proxy_node(id: string): Promise<ProxyNodeConfig> {
  return getProxyNode(id);
}

export async function push_proxy_node(
  config: ProxyNodeConfig,
): Promise<ProxyNodeConfig> {
  return await addProxyNode(config);
}

export async function delete_proxy_node(id: string): Promise<void> {
  await delProxyNode(id);
}

export async function get_proxy_runtime_statuses(): Promise<
  ProxyNodeRuntimeStatus[]
> {
  return getProxyRuntimeStatuses();
}

export async function sync_proxy_runtime(
  id: string,
): Promise<ProxyNodeRuntimeStatus> {
  return syncProxyRuntime(id);
}

export async function stop_proxy_runtime(
  id: string,
): Promise<ProxyNodeRuntimeStatus> {
  return stopProxyRuntime(id);
}

export async function remove_proxy_runtime(
  id: string,
): Promise<ProxyNodeRuntimeStatus> {
  return removeProxyRuntime(id);
}

export async function get_proxy_bypass_rule_sources_status(): Promise<ProxyBypassRuleSourcesStatus> {
  return getBypassRuleSourcesStatus();
}

export async function refresh_proxy_bypass_domain_rule_source(): Promise<ProxyBypassRuleSourcesStatus> {
  return refreshBypassDomainRuleSource();
}

export async function refresh_proxy_bypass_ip_rule_source(): Promise<ProxyBypassRuleSourcesStatus> {
  return refreshBypassIpRuleSource();
}

export async function refresh_proxy_bypass_rule_sources(): Promise<ProxyBypassRuleSourcesStatus> {
  return refreshBypassRuleSources();
}

export async function test_proxy_latency(
  request: ProxyLatencyTestRequest,
): Promise<ProxyLatencyTestResult[]> {
  return testProxyLatency(request);
}
