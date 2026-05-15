<script setup lang="ts">
import {
  delete_proxy_node,
  remove_proxy_runtime,
  stop_proxy_runtime,
  sync_proxy_runtime,
} from "@/api/proxy";
import type {
  ProxyNodeConfig,
  ProxyNodeRuntimeStatus,
} from "@landscape-router/types/api/schemas";
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useFrontEndStore } from "@/stores/front_end_config";
import ProxyNodeEditModal from "./ProxyNodeEditModal.vue";

const props = defineProps<{
  node: ProxyNodeConfig;
  runtimeStatus?: ProxyNodeRuntimeStatus;
}>();
const emit = defineEmits(["refresh"]);
const { t } = useI18n();
const frontEndStore = useFrontEndStore();
const show_edit = ref(false);
const runtime_loading = ref(false);

const protocolLabel = computed(() => {
  switch (props.node.protocol.t) {
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
});

const runtimeStateType = computed(() => {
  switch (props.runtimeStatus?.state) {
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
});

async function del() {
  if (!props.node.id) return;
  await delete_proxy_node(props.node.id);
  emit("refresh");
}

async function runRuntimeAction(
  action: (id: string) => Promise<ProxyNodeRuntimeStatus>,
) {
  if (!props.node.id) return;
  runtime_loading.value = true;
  try {
    await action(props.node.id);
    emit("refresh");
  } finally {
    runtime_loading.value = false;
  }
}
</script>

<template>
  <n-card size="small" hoverable embedded :bordered="false">
    <template #header>
      <StatusTitle
        :enable="node.enable"
        :remark="frontEndStore.MASK_INFO(node.name || t('common.unnamed'))"
      />
    </template>
    <template #header-extra>
      <n-flex size="small">
        <n-button secondary size="small" @click="show_edit = true">
          {{ t("common.edit") }}
        </n-button>
        <n-popconfirm @positive-click="del">
          <template #trigger>
            <n-button secondary size="small" type="error">
              {{ t("common.delete") }}
            </n-button>
          </template>
          {{ t("common.confirm_delete") }}
        </n-popconfirm>
      </n-flex>
    </template>

    <n-flex vertical size="small">
      <n-flex>
        <n-tag size="small" type="info" :bordered="false">
          {{ protocolLabel }}
        </n-tag>
        <n-tag size="small" :bordered="false">
          {{ frontEndStore.MASK_INFO(node.server) }}:{{ node.port }}
        </n-tag>
        <n-tag size="small" :type="runtimeStateType" :bordered="false">
          {{ t(`proxy.runtime_state.${runtimeStatus?.state || "missing"}`) }}
        </n-tag>
      </n-flex>
      <n-text depth="3" style="font-size: 12px">
        {{ runtimeStatus?.container_name || "-" }}
      </n-text>
      <n-text depth="3" style="font-size: 13px">
        {{ node.remark || t("common.no_remark") }}
      </n-text>
      <n-flex size="small">
        <n-button
          secondary
          size="tiny"
          type="primary"
          :loading="runtime_loading"
          :disabled="!node.id || !node.enable"
          @click="runRuntimeAction(sync_proxy_runtime)"
        >
          {{ t("proxy.sync_runtime") }}
        </n-button>
        <n-button
          secondary
          size="tiny"
          :loading="runtime_loading"
          :disabled="!node.id"
          @click="runRuntimeAction(stop_proxy_runtime)"
        >
          {{ t("proxy.stop_runtime") }}
        </n-button>
        <n-popconfirm @positive-click="runRuntimeAction(remove_proxy_runtime)">
          <template #trigger>
            <n-button
              secondary
              size="tiny"
              type="warning"
              :loading="runtime_loading"
              :disabled="!node.id"
            >
              {{ t("proxy.remove_runtime") }}
            </n-button>
          </template>
          {{ t("proxy.confirm_remove_runtime") }}
        </n-popconfirm>
      </n-flex>
    </n-flex>

    <ProxyNodeEditModal
      v-if="node.id"
      v-model:show="show_edit"
      :node_id="node.id"
      @refresh="emit('refresh')"
    />
  </n-card>
</template>
