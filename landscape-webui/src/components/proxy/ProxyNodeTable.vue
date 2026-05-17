<script setup lang="ts">
import { delete_proxy_node } from "@/api/proxy";
import ProxyNodeEditModal from "@/components/proxy/ProxyNodeEditModal.vue";
import { useFrontEndStore } from "@/stores/front_end_config";
import type {
  ProxyLatencyTestResult,
  ProxyLatencyTestState,
  ProxyNodeConfig,
  ProxyNodeRuntimeStatus,
} from "@landscape-router/types/api/schemas";
import type { DataTableColumns } from "naive-ui";
import {
  NButton,
  NEllipsis,
  NFlex,
  NPopconfirm,
  NTag,
  NText,
  NTooltip,
} from "naive-ui";
import { computed, h, ref } from "vue";
import { useI18n } from "vue-i18n";

type ProxyNodeRow = {
  node: ProxyNodeConfig;
  runtimeStatus?: ProxyNodeRuntimeStatus;
  latencyChina?: ProxyLatencyTestResult;
  latencyGlobal?: ProxyLatencyTestResult;
};

const props = defineProps<{
  nodes: ProxyNodeConfig[];
  runtimeStatuses: ProxyNodeRuntimeStatus[];
  latencyResults: ProxyLatencyTestResult[];
  latencyPendingNodeIds: string[];
}>();
const emit = defineEmits(["refresh", "test-latency"]);

const { t } = useI18n();
const frontEndStore = useFrontEndStore();
const show_edit = ref(false);
const editing_node_id = ref<string>();
const latency_map = computed(() => {
  const map = new Map<string, Map<string, ProxyLatencyTestResult>>();
  for (const item of props.latencyResults) {
    const node_map =
      map.get(item.node_id) || new Map<string, ProxyLatencyTestResult>();
    node_map.set(item.target, item);
    map.set(item.node_id, node_map);
  }
  return map;
});

const status_map = computed(
  () =>
    new Map(props.runtimeStatuses.map((status) => [status.node_id, status])),
);
const latency_pending_node_id_set = computed(
  () => new Set(props.latencyPendingNodeIds),
);

const rows = computed<ProxyNodeRow[]>(() =>
  props.nodes.map((node) => ({
    node,
    runtimeStatus: node.id ? status_map.value.get(node.id) : undefined,
    latencyChina: getLatency(node.id, "china"),
    latencyGlobal: getLatency(node.id, "global"),
  })),
);

const columns = computed<DataTableColumns<ProxyNodeRow>>(() => [
  {
    title: t("common.status"),
    key: "enable",
    width: 82,
    render(row) {
      return h(
        NTag,
        {
          size: "small",
          type: row.node.enable ? "success" : "default",
          bordered: false,
        },
        {
          default: () =>
            row.node.enable ? t("common.enable") : t("common.disable"),
        },
      );
    },
  },
  {
    title: t("common.name"),
    key: "name",
    minWidth: 180,
    render(row) {
      return h(
        NEllipsis,
        { style: "max-width: 260px" },
        {
          default: () =>
            frontEndStore.MASK_INFO(row.node.name || t("common.unnamed")),
        },
      );
    },
  },
  {
    title: t("proxy.protocol"),
    key: "protocol",
    width: 120,
    render(row) {
      return h(
        NTag,
        { size: "small", type: "info", bordered: false },
        { default: () => protocolLabel(row.node) },
      );
    },
  },
  {
    title: t("proxy.server"),
    key: "server",
    minWidth: 150,
    render(row) {
      return h(
        NEllipsis,
        { style: "max-width: 220px" },
        { default: () => frontEndStore.MASK_INFO(row.node.server) },
      );
    },
  },
  {
    title: t("proxy.port"),
    key: "port",
    width: 90,
    render(row) {
      return row.node.port;
    },
  },
  {
    title: t("proxy.runtime"),
    key: "runtime",
    width: 110,
    render(row) {
      const state = row.runtimeStatus?.state || "missing";
      return h(
        NTag,
        { size: "small", type: runtimeStateType(state), bordered: false },
        { default: () => t(`proxy.runtime_state.${state}`) },
      );
    },
  },
  {
    title: t("proxy.latency_china"),
    key: "latency_china",
    width: 110,
    render(row) {
      return renderLatency(row.latencyChina);
    },
  },
  {
    title: t("proxy.latency_global"),
    key: "latency_global",
    width: 110,
    render(row) {
      return renderLatency(row.latencyGlobal);
    },
  },
  {
    title: t("proxy.container"),
    key: "container",
    minWidth: 190,
    render(row) {
      return h(
        NEllipsis,
        { style: "max-width: 260px" },
        { default: () => row.runtimeStatus?.container_name || "-" },
      );
    },
  },
  {
    title: t("common.remark"),
    key: "remark",
    minWidth: 160,
    render(row) {
      return h(
        NEllipsis,
        { style: "max-width: 240px" },
        { default: () => row.node.remark || "-" },
      );
    },
  },
  {
    title: t("common.actions"),
    key: "actions",
    width: 180,
    fixed: "right",
    render(row) {
      return h(
        NFlex,
        { size: "small", wrap: false },
        {
          default: () => [
            h(
              NButton,
              {
                secondary: true,
                size: "tiny",
                loading: isLatencyPending(row.node.id),
                disabled: !row.node.id,
                onClick: () => emit("test-latency", [row.node.id]),
              },
              { default: () => t("proxy.latency_test") },
            ),
            h(
              NButton,
              {
                secondary: true,
                size: "tiny",
                onClick: () => edit(row.node),
              },
              { default: () => t("common.edit") },
            ),
            h(
              NPopconfirm,
              { onPositiveClick: () => del(row.node) },
              {
                trigger: () =>
                  h(
                    NButton,
                    {
                      secondary: true,
                      size: "tiny",
                      type: "error",
                      disabled: !row.node.id,
                    },
                    { default: () => t("common.delete") },
                  ),
                default: () => t("common.confirm_delete"),
              },
            ),
          ],
        },
      );
    },
  },
]);

function protocolLabel(node: ProxyNodeConfig) {
  switch (node.protocol.t) {
    case "vless":
      return "VLESS";
    case "vmess":
      return "VMess";
    case "shadowsocks":
      return "Shadowsocks";
    case "socks5":
      return "SOCKS5";
    default:
      return "Unknown";
  }
}

function runtimeStateType(state: string) {
  switch (state) {
    case "running":
      return "success";
    case "created":
      return "info";
    case "exited":
      return "warning";
    case "missing":
      return "default";
    default:
      return "error";
  }
}

function latencyStateType(state?: ProxyLatencyTestState) {
  switch (state) {
    case "success":
      return "success";
    case "timeout":
      return "warning";
    case "failed":
      return "error";
    case "disabled":
    case "runtime_missing":
      return "default";
    default:
      return "default";
  }
}

function renderLatency(latency?: ProxyLatencyTestResult) {
  if (!latency) {
    return h(NText, { depth: 3 }, { default: () => "-" });
  }
  if (latency.state === "success" && latency.latency_ms != null) {
    return h(
      NTag,
      { size: "small", type: "success", bordered: false },
      { default: () => `${latency.latency_ms} ms` },
    );
  }
  const label = latencyLabel(latency);
  return h(
    NTooltip,
    { trigger: "hover", disabled: !latency.error },
    {
      trigger: () =>
        h(
          NTag,
          {
            size: "small",
            type: latencyStateType(latency.state),
            bordered: false,
          },
          { default: () => latencyLabel(latency) },
        ),
      default: () => latency.error || label,
    },
  );
}

function getLatency(node_id?: string, target?: "china" | "global") {
  if (!node_id || !target) return undefined;
  return latency_map.value.get(node_id)?.get(target);
}

function isLatencyPending(node_id?: string) {
  return !!node_id && latency_pending_node_id_set.value.has(node_id);
}

function latencyLabel(latency: ProxyLatencyTestResult) {
  if (latency.state === "success" && latency.latency_ms != null) {
    return `${latency.latency_ms} ms`;
  }
  if (latency.state === "timeout") return t("proxy.latency_state.timeout");
  if (latency.state === "disabled") return t("proxy.latency_state.disabled");
  if (latency.state === "runtime_missing") {
    return t("proxy.latency_state.runtime_missing");
  }
  return t("proxy.latency_state.failed");
}

function edit(node: ProxyNodeConfig) {
  if (!node.id) return;
  editing_node_id.value = node.id;
  show_edit.value = true;
}

async function del(node: ProxyNodeConfig) {
  if (!node.id) return;
  await delete_proxy_node(node.id);
  emit("refresh");
}
</script>

<template>
  <n-data-table
    :columns="columns"
    :data="rows"
    :bordered="false"
    size="small"
    :scroll-x="1520"
  />
  <ProxyNodeEditModal
    v-if="editing_node_id"
    v-model:show="show_edit"
    :node_id="editing_node_id"
    @refresh="emit('refresh')"
  />
</template>
