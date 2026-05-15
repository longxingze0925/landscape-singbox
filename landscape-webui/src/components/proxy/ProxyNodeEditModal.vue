<script setup lang="ts">
import ConfigModal from "@/components/common/ConfigModal.vue";
import { get_proxy_node, push_proxy_node } from "@/api/proxy";
import type {
  ProxyNodeConfig,
  ProxyProtocolConfig,
} from "@landscape-router/types/api/schemas";
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

type Props = {
  node_id?: string;
};

const props = defineProps<Props>();
const emit = defineEmits(["refresh"]);
const show = defineModel<boolean>("show", { required: true });
const { t } = useI18n();

const node = ref<ProxyNodeConfig>();
const origin_json = ref("");
const commit_spin = ref(false);

const isModified = computed(
  () => JSON.stringify(node.value) !== origin_json.value,
);
const node_enabled = computed({
  get() {
    return node.value?.enable ?? false;
  },
  set(value: boolean) {
    if (node.value) node.value.enable = value;
  },
});

const protocolOptions = [
  { label: "VLESS", value: "vless" },
  { label: "VMess", value: "vmess" },
  { label: "Shadowsocks", value: "shadowsocks" },
  { label: "SOCKS5", value: "socks5" },
];

const securityOptions = [
  { label: "auto", value: "auto" },
  { label: "none", value: "none" },
  { label: "zero", value: "zero" },
  { label: "aes-128-gcm", value: "aes-128-gcm" },
  { label: "chacha20-poly1305", value: "chacha20-poly1305" },
];

const ssMethodOptions = [
  { label: "2022-blake3-aes-128-gcm", value: "2022-blake3-aes-128-gcm" },
  { label: "2022-blake3-aes-256-gcm", value: "2022-blake3-aes-256-gcm" },
  { label: "aes-128-gcm", value: "aes-128-gcm" },
  { label: "aes-256-gcm", value: "aes-256-gcm" },
  { label: "chacha20-ietf-poly1305", value: "chacha20-ietf-poly1305" },
];

function defaultProtocol(tpe = "vless"): ProxyProtocolConfig {
  switch (tpe) {
    case "vmess":
      return {
        t: "vmess",
        uuid: "",
        alter_id: 0,
        security: "auto",
        tls: false,
      };
    case "shadowsocks":
      return {
        t: "shadowsocks",
        method: "2022-blake3-aes-128-gcm",
        password: "",
      };
    case "socks5":
      return { t: "socks5", username: null, password: null };
    default:
      return {
        t: "vless",
        uuid: "",
        flow: null,
        tls: false,
        server_name: null,
        reality: false,
        reality_public_key: null,
        reality_short_id: null,
        utls_fingerprint: null,
      };
  }
}

function defaultNode(): ProxyNodeConfig {
  return {
    enable: true,
    name: "",
    server: "",
    port: 443,
    protocol: defaultProtocol(),
    remark: "",
  };
}

function onProtocolChange(value: string) {
  if (!node.value) return;
  node.value.protocol = defaultProtocol(value);
}

async function enter() {
  node.value = props.node_id
    ? await get_proxy_node(props.node_id)
    : defaultNode();
  origin_json.value = JSON.stringify(node.value);
}

function exit() {
  node.value = defaultNode();
  origin_json.value = JSON.stringify(node.value);
}

async function saveNode() {
  if (!node.value) return;
  commit_spin.value = true;
  try {
    await push_proxy_node(node.value);
    show.value = false;
    emit("refresh");
  } finally {
    commit_spin.value = false;
  }
}
</script>

<template>
  <ConfigModal
    v-model:show="show"
    v-model:enabled="node_enabled"
    :switch-disabled="!node"
    :title="t('proxy.edit_title')"
    width="660px"
    @after-enter="enter"
    @after-leave="exit"
  >
    <n-form v-if="node" label-placement="top">
      <n-grid :cols="2" :x-gap="12">
        <n-form-item-gi :label="t('common.name')">
          <n-input v-model:value="node.name" />
        </n-form-item-gi>
        <n-form-item-gi :label="t('proxy.protocol')">
          <n-select
            v-model:value="node.protocol.t"
            :options="protocolOptions"
            @update:value="onProtocolChange"
          />
        </n-form-item-gi>
        <n-form-item-gi :label="t('proxy.server')">
          <n-input v-model:value="node.server" />
        </n-form-item-gi>
        <n-form-item-gi :label="t('proxy.port')">
          <n-input-number
            v-model:value="node.port"
            :min="1"
            :max="65535"
            style="width: 100%"
          />
        </n-form-item-gi>
      </n-grid>

      <template v-if="node.protocol.t === 'vless'">
        <n-form-item label="UUID">
          <n-input v-model:value="node.protocol.uuid" />
        </n-form-item>
        <n-grid :cols="3" :x-gap="12">
          <n-form-item-gi label="TLS">
            <n-switch v-model:value="node.protocol.tls" />
          </n-form-item-gi>
          <n-form-item-gi label="Reality">
            <n-switch
              v-model:value="node.protocol.reality"
              @update:value="node.protocol.tls = node.protocol.tls || $event"
            />
          </n-form-item-gi>
          <n-form-item-gi label="uTLS">
            <n-input
              v-model:value="node.protocol.utls_fingerprint"
              placeholder="chrome"
              clearable
            />
          </n-form-item-gi>
        </n-grid>
        <n-grid :cols="3" :x-gap="12">
          <n-form-item-gi label="SNI">
            <n-input v-model:value="node.protocol.server_name" clearable />
          </n-form-item-gi>
          <n-form-item-gi label="Flow">
            <n-input
              v-model:value="node.protocol.flow"
              placeholder="xtls-rprx-vision"
              clearable
            />
          </n-form-item-gi>
          <n-form-item-gi label="Reality Public Key">
            <n-input
              v-model:value="node.protocol.reality_public_key"
              clearable
            />
          </n-form-item-gi>
        </n-grid>
        <n-form-item label="Reality Short ID">
          <n-input v-model:value="node.protocol.reality_short_id" clearable />
        </n-form-item>
      </template>

      <template v-else-if="node.protocol.t === 'vmess'">
        <n-form-item label="UUID">
          <n-input v-model:value="node.protocol.uuid" />
        </n-form-item>
        <n-grid :cols="4" :x-gap="12">
          <n-form-item-gi label="Alter ID">
            <n-input-number
              v-model:value="node.protocol.alter_id"
              :min="0"
              style="width: 100%"
            />
          </n-form-item-gi>
          <n-form-item-gi label="Security">
            <n-select
              v-model:value="node.protocol.security"
              :options="securityOptions"
            />
          </n-form-item-gi>
          <n-form-item-gi label="TLS">
            <n-switch v-model:value="node.protocol.tls" />
          </n-form-item-gi>
          <n-form-item-gi label="SNI">
            <n-input v-model:value="node.protocol.server_name" clearable />
          </n-form-item-gi>
        </n-grid>
      </template>

      <template v-else-if="node.protocol.t === 'shadowsocks'">
        <n-grid :cols="2" :x-gap="12">
          <n-form-item-gi :label="t('proxy.method')">
            <n-select
              v-model:value="node.protocol.method"
              filterable
              tag
              :options="ssMethodOptions"
            />
          </n-form-item-gi>
          <n-form-item-gi :label="t('common.password')">
            <n-input
              v-model:value="node.protocol.password"
              type="password"
              show-password-on="click"
            />
          </n-form-item-gi>
        </n-grid>
      </template>

      <template v-else-if="node.protocol.t === 'socks5'">
        <n-grid :cols="2" :x-gap="12">
          <n-form-item-gi :label="t('common.username')">
            <n-input v-model:value="node.protocol.username" clearable />
          </n-form-item-gi>
          <n-form-item-gi :label="t('common.password')">
            <n-input
              v-model:value="node.protocol.password"
              type="password"
              show-password-on="click"
              clearable
            />
          </n-form-item-gi>
        </n-grid>
      </template>

      <n-form-item :label="t('common.remark')">
        <n-input v-model:value="node.remark" type="textarea" />
      </n-form-item>
    </n-form>
    <template #footer>
      <n-flex justify="space-between">
        <n-button @click="show = false">{{ t("common.cancel") }}</n-button>
        <n-button
          :disabled="!isModified"
          :loading="commit_spin"
          type="primary"
          @click="saveNode"
        >
          {{ t("common.save") }}
        </n-button>
      </n-flex>
    </template>
  </ConfigModal>
</template>
