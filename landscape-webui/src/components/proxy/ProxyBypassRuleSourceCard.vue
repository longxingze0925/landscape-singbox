<script setup lang="ts">
import type {
  ProxyBypassRuleSourceStatus,
  ProxyBypassRuleSourcesStatus,
} from "@landscape-router/types/api/schemas";
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

const props = defineProps<{
  status?: ProxyBypassRuleSourcesStatus;
  loading?: boolean;
}>();

const emit = defineEmits<{
  refreshDomain: [];
  refreshIp: [];
  refreshAll: [];
}>();

const { t } = useI18n();
const expanded = ref(false);

const sources = computed(() => {
  if (!props.status) return [];
  return [
    { title: t("proxy.domain_rule_source"), value: props.status.domain },
    { title: t("proxy.ip_rule_source"), value: props.status.ip },
  ];
});

const ready = computed(() =>
  sources.value.every(({ value }) => value.cache_exists),
);
const totalItems = computed(() =>
  sources.value.reduce((total, { value }) => total + value.item_count, 0),
);

function formatTime(ts?: number | null): string {
  if (!ts) return "-";
  return new Date(ts * 1000).toLocaleString();
}

function refreshSource(source: ProxyBypassRuleSourceStatus) {
  if (source.kind === "domain") {
    emit("refreshDomain");
    return;
  }
  emit("refreshIp");
}
</script>

<template>
  <n-card size="small" embedded :bordered="false">
    <template #header>
      <n-flex align="center" size="small">
        <span>{{ t("proxy.bypass_rule_sources") }}</span>
        <n-tag
          v-if="status"
          size="small"
          :type="ready ? 'success' : 'warning'"
          :bordered="false"
        >
          {{
            ready
              ? t("proxy.rule_sources_ready")
              : t("proxy.rule_sources_missing")
          }}
        </n-tag>
      </n-flex>
    </template>
    <template #header-extra>
      <n-flex size="small">
        <n-button text size="small" @click="expanded = !expanded">
          {{
            expanded
              ? t("proxy.collapse_rule_sources")
              : t("proxy.expand_rule_sources")
          }}
        </n-button>
        <n-button
          secondary
          size="small"
          :loading="loading"
          @click="emit('refreshAll')"
        >
          {{ t("proxy.refresh_all_rules") }}
        </n-button>
      </n-flex>
    </template>

    <n-skeleton v-if="!status" text :repeat="2" />
    <n-flex v-else vertical size="small">
      <n-alert v-if="!ready" type="warning" :bordered="false">
        {{ t("proxy.rule_sources_missing_tip") }}
      </n-alert>

      <n-flex size="small">
        <n-tag size="small" :bordered="false">
          {{ t("proxy.domain_rule_source") }}:
          {{ status.domain.cache_exists ? status.domain.item_count : "-" }}
        </n-tag>
        <n-tag size="small" :bordered="false">
          {{ t("proxy.ip_rule_source") }}:
          {{ status.ip.cache_exists ? status.ip.item_count : "-" }}
        </n-tag>
        <n-tag size="small" type="info" :bordered="false">
          {{ t("proxy.total_rule_items", { count: totalItems }) }}
        </n-tag>
      </n-flex>

      <n-grid v-if="expanded" x-gap="12" y-gap="10" cols="1 900:2">
        <n-grid-item v-for="source in sources" :key="source.value.kind">
          <n-card size="small" :title="source.title">
            <template #header-extra>
              <n-button
                secondary
                size="tiny"
                :loading="loading"
                @click="refreshSource(source.value)"
              >
                {{ t("proxy.refresh_this_rule_source") }}
              </n-button>
            </template>

            <n-descriptions
              bordered
              label-placement="left"
              :column="1"
              size="small"
            >
              <n-descriptions-item :label="t('proxy.rule_source_key')">
                {{ source.value.name }}/{{ source.value.key }}
              </n-descriptions-item>
              <n-descriptions-item label="URL">
                <n-ellipsis style="max-width: 520px">
                  {{ source.value.url || "-" }}
                </n-ellipsis>
              </n-descriptions-item>
              <n-descriptions-item :label="t('proxy.cache_status')">
                <n-tag
                  size="small"
                  :type="source.value.cache_exists ? 'success' : 'warning'"
                  :bordered="false"
                >
                  {{
                    source.value.cache_exists
                      ? t("proxy.cache_ready")
                      : t("proxy.cache_missing")
                  }}
                </n-tag>
              </n-descriptions-item>
              <n-descriptions-item :label="t('proxy.item_count')">
                {{ source.value.item_count }}
              </n-descriptions-item>
              <n-descriptions-item :label="t('proxy.next_update_at')">
                {{ formatTime(source.value.next_update_at) }}
              </n-descriptions-item>
              <n-descriptions-item :label="t('proxy.last_success_at')">
                {{ formatTime(source.value.last_success_at) }}
              </n-descriptions-item>
              <n-descriptions-item :label="t('proxy.last_error')">
                <n-text v-if="source.value.last_error" type="error">
                  {{ source.value.last_error }}
                </n-text>
                <span v-else>-</span>
              </n-descriptions-item>
            </n-descriptions>
          </n-card>
        </n-grid-item>
      </n-grid>
    </n-flex>
  </n-card>
</template>
