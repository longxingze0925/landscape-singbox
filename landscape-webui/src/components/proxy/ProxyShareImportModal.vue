<script setup lang="ts">
import { push_proxy_node } from "@/api/proxy";
import {
  parseProxyShareLinks,
  type ParsedProxyShareLink,
} from "@/lib/proxyShareLink";
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

const emit = defineEmits(["refresh"]);
const show = defineModel<boolean>("show", { required: true });
const { t } = useI18n();

type ImportRow = {
  index: number;
  node?: ParsedProxyShareLink["node"];
  name: string;
  protocol: string;
  server: string;
  port: number | string;
  features: string;
  status?: string;
};

const input = ref("");
const parsed = ref<ParsedProxyShareLink[]>([]);
const checked = ref<number[]>([]);
const importing = ref(false);

const importableIndexes = computed(() =>
  parsed.value
    .map((item, index) => (item.node ? index : -1))
    .filter((index) => index >= 0),
);

const tableRows = computed<ImportRow[]>(() =>
  parsed.value.map((item, index) => ({
    index,
    node: item.node,
    name: item.node?.name || "-",
    protocol: protocolLabel(item),
    server: item.node?.server || "-",
    port: item.node?.port || "-",
    features: featureLabel(item),
    status: item.node ? t("proxy.import_ready") : item.error,
  })),
);

function parseInput() {
  parsed.value = parseProxyShareLinks(input.value);
  checked.value = importableIndexes.value;
}

function reset() {
  input.value = "";
  parsed.value = [];
  checked.value = [];
  importing.value = false;
}

function protocolLabel(item: ParsedProxyShareLink) {
  const protocol = item.node?.protocol.t;
  if (!protocol) return "-";
  if (protocol === "shadowsocks") return "Shadowsocks";
  if (protocol === "socks5") return "SOCKS5";
  return protocol.toUpperCase();
}

function featureLabel(item: ParsedProxyShareLink) {
  const protocol = item.node?.protocol;
  if (!protocol) return item.error || "-";
  if (protocol.t === "vless") {
    const features = [];
    if (protocol.tls) features.push("TLS");
    if (protocol.reality) features.push("Reality");
    if (protocol.flow) features.push(protocol.flow);
    return features.join(" / ") || "-";
  }
  if (protocol.t === "vmess") return protocol.tls ? "TLS" : "-";
  if (protocol.t === "shadowsocks") return protocol.method;
  return protocol.username ? "Auth" : "-";
}

async function importSelected() {
  importing.value = true;
  try {
    for (const index of checked.value) {
      const node = parsed.value[index]?.node;
      if (node) await push_proxy_node(node);
    }
    show.value = false;
    emit("refresh");
  } finally {
    importing.value = false;
  }
}
</script>

<template>
  <n-modal
    v-model:show="show"
    preset="card"
    :title="t('proxy.import_share_links')"
    style="width: min(920px, 92vw)"
    @after-leave="reset"
  >
    <n-flex vertical>
      <n-input
        v-model:value="input"
        type="textarea"
        :autosize="{ minRows: 6, maxRows: 12 }"
        :placeholder="t('proxy.share_link_placeholder')"
      />
      <n-flex justify="space-between" align="center">
        <n-text depth="3">{{ t("proxy.share_link_supported") }}</n-text>
        <n-button secondary @click="parseInput">
          {{ t("proxy.parse_share_links") }}
        </n-button>
      </n-flex>

      <n-data-table
        v-if="parsed.length"
        v-model:checked-row-keys="checked"
        :row-key="(row: ImportRow) => row.index"
        :columns="[
          { type: 'selection', disabled: (row: ImportRow) => !row.node },
          { title: t('common.name'), key: 'name' },
          { title: t('proxy.protocol'), key: 'protocol' },
          { title: t('proxy.server'), key: 'server' },
          { title: t('proxy.port'), key: 'port' },
          { title: t('proxy.import_features'), key: 'features' },
          { title: t('proxy.import_status'), key: 'status' },
        ]"
        :data="tableRows"
        size="small"
      />
    </n-flex>

    <template #footer>
      <n-flex justify="space-between">
        <n-button @click="show = false">{{ t("common.cancel") }}</n-button>
        <n-button
          type="primary"
          :disabled="checked.length === 0"
          :loading="importing"
          @click="importSelected"
        >
          {{ t("proxy.import_selected_nodes", { count: checked.length }) }}
        </n-button>
      </n-flex>
    </template>
  </n-modal>
</template>
