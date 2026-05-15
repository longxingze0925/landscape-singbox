<script setup lang="ts">
import {
  get_proxy_bypass_rule_sources_status,
  get_proxy_nodes,
  get_proxy_runtime_statuses,
  refresh_proxy_bypass_domain_rule_source,
  refresh_proxy_bypass_ip_rule_source,
  refresh_proxy_bypass_rule_sources,
} from "@/api/proxy";
import ProxyBypassRuleSourceCard from "@/components/proxy/ProxyBypassRuleSourceCard.vue";
import ProxyNodeCard from "@/components/proxy/ProxyNodeCard.vue";
import ProxyNodeEditModal from "@/components/proxy/ProxyNodeEditModal.vue";
import ProxyShareImportModal from "@/components/proxy/ProxyShareImportModal.vue";
import type {
  ProxyBypassRuleSourcesStatus,
  ProxyNodeConfig,
  ProxyNodeRuntimeStatus,
} from "@landscape-router/types/api/schemas";
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

const { t } = useI18n();
const nodes = ref<ProxyNodeConfig[]>([]);
const runtime_statuses = ref<ProxyNodeRuntimeStatus[]>([]);
const bypass_rule_sources_status = ref<ProxyBypassRuleSourcesStatus>();
const show_edit = ref(false);
const show_import = ref(false);
const active_tab = ref("nodes");
const rule_sources_loading = ref(false);
const status_map = computed(
  () =>
    new Map(runtime_statuses.value.map((status) => [status.node_id, status])),
);
const rule_sources_ready = computed(
  () =>
    bypass_rule_sources_status.value?.domain.cache_exists &&
    bypass_rule_sources_status.value?.ip.cache_exists,
);
const total_rule_items = computed(() => {
  const status = bypass_rule_sources_status.value;
  if (!status) return 0;
  return status.domain.item_count + status.ip.item_count;
});

async function refresh() {
  const [node_list, status_list, rule_sources_status] = await Promise.all([
    get_proxy_nodes(),
    get_proxy_runtime_statuses(),
    get_proxy_bypass_rule_sources_status(),
  ]);
  nodes.value = node_list;
  runtime_statuses.value = status_list;
  bypass_rule_sources_status.value = rule_sources_status;
}

async function runRuleSourceRefresh(
  action: () => Promise<ProxyBypassRuleSourcesStatus>,
) {
  rule_sources_loading.value = true;
  try {
    bypass_rule_sources_status.value = await action();
  } finally {
    rule_sources_loading.value = false;
  }
}

onMounted(refresh);
</script>

<template>
  <n-layout :native-scrollbar="false" content-style="padding: 10px;">
    <n-flex vertical style="flex: 1">
      <n-tabs v-model:value="active_tab" type="line" animated>
        <n-tab-pane name="nodes" :tab="t('proxy.nodes_tab')">
          <n-flex vertical>
            <n-alert type="info" :bordered="false">
              {{ t("proxy.runtime_notice") }}
            </n-alert>
            <n-alert
              type="success"
              :bordered="false"
              style="cursor: pointer"
              @click="active_tab = 'rule_sources'"
            >
              {{
                t("proxy.rule_sources_summary", {
                  status: rule_sources_ready
                    ? t("proxy.rule_sources_ready")
                    : t("proxy.rule_sources_missing"),
                  count: total_rule_items,
                })
              }}
            </n-alert>
            <n-flex justify="space-between" align="center">
              <n-flex>
                <n-button type="primary" @click="show_edit = true">
                  {{ t("proxy.create_node") }}
                </n-button>
                <n-button secondary @click="show_import = true">
                  {{ t("proxy.import_share_links") }}
                </n-button>
              </n-flex>
              <n-button secondary @click="refresh">
                {{ t("common.refresh") }}
              </n-button>
            </n-flex>
            <n-empty
              v-if="nodes.length === 0"
              :description="t('proxy.no_nodes')"
            />
            <n-grid v-else x-gap="12" y-gap="10" cols="1 700:2 1100:3 1500:4">
              <n-grid-item v-for="node in nodes" :key="node.id">
                <ProxyNodeCard
                  :node="node"
                  :runtime-status="
                    node.id ? status_map.get(node.id) : undefined
                  "
                  @refresh="refresh"
                />
              </n-grid-item>
            </n-grid>
          </n-flex>
        </n-tab-pane>
        <n-tab-pane name="rule_sources" :tab="t('proxy.rule_sources_tab')">
          <ProxyBypassRuleSourceCard
            :status="bypass_rule_sources_status"
            :loading="rule_sources_loading"
            default-expanded
            hide-toggle
            @refresh-domain="
              runRuleSourceRefresh(refresh_proxy_bypass_domain_rule_source)
            "
            @refresh-ip="
              runRuleSourceRefresh(refresh_proxy_bypass_ip_rule_source)
            "
            @refresh-all="
              runRuleSourceRefresh(refresh_proxy_bypass_rule_sources)
            "
          />
        </n-tab-pane>
      </n-tabs>
    </n-flex>
    <ProxyNodeEditModal v-model:show="show_edit" @refresh="refresh" />
    <ProxyShareImportModal v-model:show="show_import" @refresh="refresh" />
  </n-layout>
</template>
