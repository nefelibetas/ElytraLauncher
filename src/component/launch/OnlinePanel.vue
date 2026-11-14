<script setup lang="ts">
// 联机面板：EasyTier 虚拟局域网（host 开房 / guest 加入 / 停止）；配置在「设置 → 联机设置」。
// host 可先开联机（等待态）再「对局域网开放」，检测到端口自动生成房间码；正版 host 由服务器验证会话。
import { onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { Copy, Link2, Settings2, Unplug } from "@lucide/vue";
import {
  onlineStatus, onlineRoomCode, onlineRoomInput, onlinePanelTab, onlineLocalPort, onlinePlayers,
  onlinePath, onlineLatency, onlineAllowRelay,
  onlineErrorHost, onlineErrorGuest,
  startHost, joinRoom, stopOnline, loadOnlineStatus, ensureOnlineListen,
} from "../../store/config/online";
import { settingsTab } from "../../store/app/settingsTab";
import { showToast } from "../../utils/toast";
import { errMsg } from "../../utils/error";
import { copyText } from "../../utils/clipboard";

const { t } = useI18n();
const router = useRouter();

/** 跳到 设置 → 联机设置（昵称 / 广播名 / 节点）。 */
function openOnlineSettings() {
  settingsTab.value = "online";
  void router.push("/settings");
}

onMounted(() => {
  ensureOnlineListen();
  void loadOnlineStatus();
});

/** 主机 / 加入 二选一，以 tab 呈现；会话进行中自动切到对应页。 */
watch(
  onlineStatus,
  (status) => {
    if (status === "joining") onlinePanelTab.value = "join";
    else if (status === "waiting" || status === "hosting") onlinePanelTab.value = "host";
  },
  { immediate: true },
);

/** 当前 tab 是否被进行中的另一种会话挡住。 */
const hostBlocked = () => onlineStatus.value === "joining";
const joinBlocked = () => onlineStatus.value === "waiting" || onlineStatus.value === "hosting";

const busy = ref(false);

async function handleStartHost() {
  busy.value = true;
  try {
    await startHost();
    showToast(t("tools.online.hostStarted"));
  } catch (e) {
    showToast(errMsg(e));
  } finally {
    busy.value = false;
  }
}

async function handleJoin() {
  if (busy.value) return;
  busy.value = true;
  try {
    await joinRoom(onlineRoomInput.value.trim());
    showToast(t("tools.online.joined"));
  } catch (e) {
    showToast(errMsg(e));
  } finally {
    busy.value = false;
  }
}

async function handleStop() {
  if (busy.value) return;
  busy.value = true;
  try {
    await stopOnline();
    showToast(t("tools.online.stopped"));
  } catch (e) {
    showToast(errMsg(e));
  } finally {
    busy.value = false;
  }
}

async function copyRoomCode() {
  await copyText(onlineRoomCode.value, t("tools.online.roomCodeCopied"));
}
</script>

<template>
  <div class="online-panel">
    <div class="online-status">
      <span class="online-dot" :class="{ active: onlineStatus !== 'idle', pulse: onlineStatus === 'waiting' }" />
      <span class="online-status-text">
        {{
          onlineStatus === "hosting" ? t("tools.online.statusHosting")
          : onlineStatus === "joining" ? t("tools.online.statusJoining")
          : onlineStatus === "waiting" ? t("tools.online.statusWaiting")
          : t("tools.online.statusIdle")
        }}
      </span>
    </div>
    <div class="online-protocol">{{ t("tools.online.protocol") }}</div>

    <div v-if="onlineStatus === 'joining'" class="online-path">
      <span v-if="onlinePath" class="online-path-badge" :class="onlinePath === 'direct' ? 'is-direct' : 'is-relay'">
        {{ onlinePath === "direct" ? t("tools.online.pathDirect") : t("tools.online.pathRelay") }}
        <span v-if="onlineLatency != null">{{ t("tools.online.pathLatency", { ms: onlineLatency }) }}</span>
      </span>
      <span v-else-if="!onlineAllowRelay" class="online-path-hint">
        {{ t("tools.online.directOnlyHint") }}
      </span>
    </div>
    <div v-if="onlineStatus === 'joining' && onlinePath === 'relay'" class="online-relay-warning">
      {{ t("tools.online.relayWarning") }}
    </div>

    <div class="online-tabs" role="tablist">
      <button
        type="button"
        class="online-tab"
        :class="{ active: onlinePanelTab === 'host' }"
        role="tab"
        :aria-selected="onlinePanelTab === 'host'"
        @click="onlinePanelTab = 'host'"
      >
        {{ t("tools.online.hostSection") }}
      </button>
      <button
        type="button"
        class="online-tab"
        :class="{ active: onlinePanelTab === 'join' }"
        role="tab"
        :aria-selected="onlinePanelTab === 'join'"
        @click="onlinePanelTab = 'join'"
      >
        {{ t("tools.online.joinSection") }}
      </button>
    </div>

    <div v-show="onlinePanelTab === 'host'" class="online-sec">
      <p class="online-desc">{{ t("tools.online.hostDesc1") }}</p>
      <p class="online-desc">{{ t("tools.online.hostDesc2") }}</p>
      <button type="button" class="online-settings-link" @click="openOnlineSettings">
        <Settings2 :size="12" />
        {{ t("tools.online.openOnlineSettings") }}
      </button>

      <div v-if="hostBlocked()" class="online-sec-note">{{ t("tools.online.hostingInProgress") }}</div>

      <div v-if="onlineStatus === 'waiting'" class="online-waiting">
        {{ t("tools.online.waitingHint") }}
      </div>

      <div v-if="onlineStatus === 'hosting'" class="online-room">
        <span class="online-room-code">{{ onlineRoomCode }}</span>
      </div>

      <div v-if="onlineErrorHost" class="online-error">{{ errMsg(onlineErrorHost) }}</div>

      <div class="online-actions">
        <button
          v-if="onlineStatus === 'hosting'"
          class="online-btn"
          @click="copyRoomCode"
        >
          <Copy :size="14" />
          {{ t("tools.online.copyRoomCode") }}
        </button>
        <button
          v-if="onlineStatus === 'hosting' || onlineStatus === 'waiting'"
          class="online-btn online-btn-stop"
          @click="handleStop"
        >
          <Unplug :size="14" />
          {{ t("tools.online.stop") }}
        </button>
        <button
          v-else-if="onlineStatus === 'idle'"
          class="online-btn online-btn-primary"
          :disabled="busy"
          @click="handleStartHost"
        >
          <Link2 :size="14" />
          {{ busy ? t("tools.online.startingHost") : t("tools.online.startHost") }}
        </button>
      </div>
    </div>

    <div v-show="onlinePanelTab === 'join'" class="online-sec">
      <p class="online-desc">{{ t("tools.online.joinDesc") }}</p>

      <div v-if="joinBlocked()" class="online-sec-note">{{ t("tools.online.joiningInProgress") }}</div>

      <div class="online-row">
        <span class="online-label">{{ t("tools.online.roomCode") }}</span>
        <input
          v-model="onlineRoomInput"
          class="online-input"
          placeholder="U/XXXX-XXXX-XXXX-XXXX"
          :disabled="onlineStatus !== 'idle' || busy"
          @keyup.enter="handleJoin"
          autocomplete="off"
        />
      </div>

      <div v-if="onlineStatus === 'joining' && onlineLocalPort" class="online-room">
        <span class="online-room-code">{{ t("tools.online.roomDisplayName") }}</span>
        <span class="online-hint">{{ t("tools.online.joiningHint", { port: onlineLocalPort }) }}</span>
      </div>

      <div v-if="onlineErrorGuest" class="online-error">{{ errMsg(onlineErrorGuest) }}</div>

      <div class="online-actions">
        <button
          v-if="onlineStatus === 'joining'"
          class="online-btn online-btn-stop"
          @click="handleStop"
        >
          <Unplug :size="14" />
          {{ t("tools.online.stop") }}
        </button>
        <button
          v-else-if="onlineStatus === 'idle'"
          class="online-btn online-btn-primary"
          :disabled="busy || !onlineRoomInput.trim()"
          @click="handleJoin"
        >
          <Link2 :size="14" />
          {{ busy ? t("tools.online.joining") : t("tools.online.join") }}
        </button>
      </div>
    </div>

    <div v-if="onlinePlayers.length > 0" class="online-sec">
      <div class="online-sec-title">{{ t("tools.online.players", { count: onlinePlayers.length }, onlinePlayers.length) }}</div>
      <ul class="online-players">
        <li v-for="p in onlinePlayers" :key="p.machine_id" class="online-player">
          <span class="online-player-dot" :class="p.kind === 'HOST' ? 'host' : 'guest'" />
          <span class="online-player-name">{{ p.name }}</span>
          <span class="online-player-tag">{{ p.kind === "HOST" ? t("tools.online.roleHost") : p.kind === "LOCAL" ? t("tools.online.roleSelf") : t("tools.online.roleGuest") }}</span>
        </li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.online-panel {
  display: flex;
  flex-direction: column;
}

.online-status {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  padding: 8px 0;
  font-size: var(--fs-base);
  color: var(--text-secondary);
}

.online-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background-color: var(--text-tertiary);
}

.online-dot.active {
  background-color: var(--color-brand);
  box-shadow: 0 0 6px var(--color-brand);
}

.online-dot.pulse {
  animation: online-pulse 1.2s ease-in-out infinite;
}

@keyframes online-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

.online-status-text { font-weight: 500; }

.online-protocol {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  text-align: right;
  margin: -2px 0 6px;
}

.online-tabs {
  display: flex;
  gap: 4px;
  padding: 4px;
  margin: 4px 0 2px;
  background: var(--surface-hover);
  border-radius: var(--radius-md);
}

.online-tab {
  flex: 1;
  height: 30px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--fs-base);
  font-weight: 500;
  cursor: pointer;
  transition: background var(--dur-fast), color var(--dur-fast);
}

.online-tab:hover {
  color: var(--text-primary);
}

.online-tab.active {
  color: var(--color-brand);
  background: var(--surface-card);
  box-shadow: var(--shadow-sm);
}

.online-sec {
  padding: 12px 0;
  border-top: 1px solid var(--border-default);
}

.online-sec-title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 4px;
}

.online-sec-note {
  margin: 8px 0 2px;
  padding: 6px 8px;
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  background: var(--surface-hover);
  border-radius: var(--radius-sm);
}

.online-desc {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  line-height: 1.5;
  margin: 0 0 10px;
}

.online-settings-link {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin: 0 0 10px;
  padding: 4px 8px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: color var(--dur-fast), border-color var(--dur-fast), background var(--dur-fast);
}

.online-settings-link:hover {
  color: var(--color-brand);
  border-color: var(--color-brand);
  background: var(--color-brand-bg);
}

.online-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 5px 0;
}

.online-label {
  flex: 0 0 64px;
  font-size: var(--fs-base);
  color: var(--text-secondary);
}

.online-input {
  flex: 1;
  min-width: 0;
  height: 32px;
  padding: 0 10px;
  font-size: var(--fs-base);
  color: var(--text-primary);
  background-color: var(--surface-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
}

.online-input:focus {
  border-color: var(--color-brand);
  outline: none;
}

.online-room {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  padding: 8px 0;
}

.online-room-code {
  font-family: "Cascadia Mono", "Consolas", monospace;
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--color-brand);
  letter-spacing: 0.5px;
  user-select: none;
}

.online-hint {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}

.online-waiting {
  padding: 8px 0;
  font-size: var(--fs-base);
  color: var(--text-tertiary);
  font-style: italic;
}

.online-actions {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 8px;
  padding-top: 8px;
}

.online-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 14px;
  font-size: var(--fs-base);
  font-weight: 500;
  color: var(--text-secondary);
  background-color: var(--surface-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all var(--dur-fast);
}

.online-btn:hover:not(:disabled) {
  border-color: var(--color-brand);
  color: var(--text-primary);
}

.online-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.online-btn-stop {
  color: var(--color-danger);
}

.online-btn-primary {
  color: var(--text-inverse);
  background-color: var(--color-brand);
  border-color: var(--color-brand);
}

.online-btn-primary:hover:not(:disabled) {
  background-color: var(--color-brand);
  color: var(--text-inverse);
  filter: brightness(1.08);
}

.online-error {
  margin-top: 8px;
  padding: 8px 10px;
  font-size: var(--fs-sm);
  line-height: 1.5;
  color: var(--color-danger);
  background-color: var(--color-danger-bg);
  background-color: color-mix(in srgb, var(--color-danger) 8%, transparent);
  border-radius: var(--radius-sm);
  white-space: pre-wrap;
  word-break: break-all;
}

.online-players {
  list-style: none;
  margin: 4px 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.online-player {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 8px;
  border-radius: var(--radius-sm);
  background-color: var(--surface-page);
  background-color: color-mix(in srgb, var(--surface-page) 60%, transparent);
}

.online-player-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background-color: var(--text-tertiary);
}

.online-player-dot.host {
  background-color: var(--color-brand);
}

.online-player-dot.guest {
  background-color: var(--text-secondary);
}

.online-player-name {
  flex: 1;
  min-width: 0;
  font-size: var(--fs-base);
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.online-player-tag {
  flex: 0 0 auto;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.online-path {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
}

.online-path-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 1px 8px;
  font-size: var(--fs-xs);
  font-weight: 600;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-pill);
  background-color: var(--surface-page);
}

.online-path-badge.is-direct {
  color: var(--color-brand);
}

.online-path-badge.is-relay {
  color: var(--color-danger);
}

.online-path-hint {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.online-relay-warning {
  margin-top: 6px;
  font-size: var(--fs-xs);
  line-height: 1.5;
  color: var(--color-danger);
}
</style>
