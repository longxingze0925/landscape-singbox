<script setup lang="ts">
import {
  delete_proxy_node,
  remove_proxy_runtime,
  stop_proxy_runtime,
  sync_proxy_runtime,
} from "@/api/proxy";
import ProxyNodeEditModal from "@/components/proxy/ProxyNodeEditModal.vue";
import { useFrontEndStore } from "@/stores/front_end_config";
import type {
  ProxyNodeConfig,
  ProxyNodeRuntimeStatus,
} from "@landscape-router/types/api/schemas";
import type { DataTableColumns } from "naive-ui";
import { NButton, NEllipsis, NFlex, NPopconfirm, NTag, NText } from "naive-ui";
import { computed, h, ref } from "vue";
import { useI18n } from "vue-i18n";

type ProxyNodeRow = {
  node: ProxyNodeConfig;
  runtimeStatus?: ProxyNodeRuntimeStatus;
};

const props = defineProps<{
  nodes: ProxyNodeConfig[];
  runtimeStatuses: ProxyNodeRuntimeStatus[];
}>();
const emit = defineEmits(["refresh"]);

const { t } = useI18n();
const frontEndStore = useFrontEndStore();
const show_edit = ref(false);
const editing_node_id = ref<string>();
const loading_node_id = ref<string>();

const status_map = computed(
  () =>
    new Map(props.runtimeStatuses.map((status) => [status.node_id, status])),
);

const rows = computed<ProxyNodeRow[]>(() =>
  props.nodes.map((node) => ({
    node,
    runtimeStatus: node.id ? status_map.value.get(node.id) : undefined,
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
    width: 360,
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
                onClick: () => edit(row.node),
              },
              { default: () => t("common.edit") },
            ),
            actionButton(
              row,
              sync_proxy_runtime,
              t("proxy.sync_runtime"),
              "primary",
              !row.node.enable,
            ),
            actionButton(row, stop_proxy_runtime, t("proxy.stop_runtime")),
            h(
              NPopconfirm,
              {
                onPositiveClick: () =>
                  runRuntimeAction(row, remove_proxy_runtime),
              },
              {
                trigger: () =>
                  h(
                    NButton,
                    {
                      secondary: true,
                      size: "tiny",
                      type: "warning",
                      loading: loading_node_id.value === row.node.id,
                      disabled: !row.node.id,
                    },
                    { default: () => t("proxy.remove_runtime") },
                  ),
                default: () => t("proxy.confirm_remove_runtime"),
              },
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

function actionButton(
  row: ProxyNodeRow,
  action: (id: string) => Promise<ProxyNodeRuntimeStatus>,
  label: string,
  type?: "primary" | "warning" | "error",
  disabled = false,
) {
  return h(
    NButton,
    {
      secondary: true,
      size: "tiny",
      type,
      loading: loading_node_id.value === row.node.id,
      disabled: disabled || !row.node.id,
      onClick: () => runRuntimeAction(row, action),
    },
    { default: () => label },
  );
}

async function runRuntimeAction(
  row: ProxyNodeRow,
  action: (id: string) => Promise<ProxyNodeRuntimeStatus>,
) {
  if (!row.node.id) return;
  loading_node_id.value = row.node.id;
  try {
    await action(row.node.id);
    emit("refresh");
  } finally {
    loading_node_id.value = undefined;
  }
}
</script>

<template>
  <n-data-table
    :columns="columns"
    :data="rows"
    :bordered="false"
    size="small"
    :scroll-x="1280"
  />
  <ProxyNodeEditModal
    v-if="editing_node_id"
    v-model:show="show_edit"
    :node_id="editing_node_id"
    @refresh="emit('refresh')"
  />
</template>
