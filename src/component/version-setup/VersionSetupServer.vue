<script setup lang="ts">
// 版本设置-服务器：快速进服（--server/--port），per-version 存储，启动时优先于全局配置。
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { loadVersionServer, saveVersionServer } from "../../store/task/versionSetup";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import SetupCard from "../common/SetupCard.vue";

const route = useRoute();
const { t } = useI18n();
const versionName = computed(() => String(route.query.name ?? ""));

const server = ref("");
const port = ref("");
const saved = ref(false);

onMounted(async () => {
  try {
    const cfg = await loadVersionServer(versionName.value);
    server.value = cfg.server ?? "";
    port.value = cfg.port != null ? String(cfg.port) : "";
  } catch {
    // 无配置保持空值
  }
});

async function save() {
  try {
    await saveVersionServer(
      versionName.value,
      server.value.trim() || null,
      port.value.trim() ? Number(port.value.trim()) : null,
    );
    saved.value = true;
    setTimeout(() => (saved.value = false), 1500);
  } catch (e) {
    showToast(errMsg(e));
  }
}
</script>

<template>
  <div class="vs-page">
    <SetupCard
      :title="t('settings.version.setup.server.title')"
      :desc="t('settings.version.setup.server.subtitle')"
    >
      <div class="vssrv">
        <div class="vssrv-row">
          <label class="vssrv-label" for="vssrv-server">{{ t("settings.version.setup.server.address") }}</label>
          <input
            id="vssrv-server"
            v-model="server"
            class="vssrv-input"
            type="text"
            :placeholder="t('settings.version.setup.server.addressPlaceholder')"
            maxlength="128"
            @change="save"
            autocomplete="off"
          />
        </div>
        <div class="vssrv-row">
          <label class="vssrv-label" for="vssrv-port">{{ t("settings.version.setup.server.port") }}</label>
          <input
            id="vssrv-port"
            v-model="port"
            class="vssrv-input"
            type="number"
            min="1"
            max="65535"
            placeholder="25565"
            @change="save"
            autocomplete="off"
          />
          <span v-if="saved" class="vssrv-saved">{{ t("settings.version.setup.server.saved") }}</span>
        </div>
        <p class="vssrv-note">
          {{ t("settings.version.setup.server.note") }}
        </p>
      </div>
    </SetupCard>
  </div>
</template>

<style scoped>

.vssrv {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.vssrv-row {
  display: flex;
  align-items: center;
  gap: 12px;
}
.vssrv-label {
  flex: none;
  width: 72px;
  font-size: var(--fs-base);
  color: var(--text-secondary);
}
.vssrv-input {
  flex: 1;
  height: 32px;
  padding: 0 10px;
  font-size: var(--fs-base);
  color: var(--text-primary);
  background: var(--surface-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  outline: none;
}
.vssrv-input:focus {
  border-color: var(--color-brand);
}
.vssrv-saved {
  flex: none;
  font-size: var(--fs-sm);
  color: var(--color-brand);
}
.vssrv-note {
  margin: 0;
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-tertiary);
}
</style>
