// 装饰性资源的本地离线缓存：同步读取供首帧直接渲染，网络刷新在后台替换。
// 只存公开贴图/渲染结果；写失败（配额不足、隐私模式）静默降级为纯内存缓存。
const CACHE_PREFIX = "elytra.skinCache.v1.";

/** 读取缓存；缺失、解析失败或存储不可用时返回 null。 */
export function readCache<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(CACHE_PREFIX + key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

/** 写入缓存；失败不抛出（缓存只是首帧优化，功能继续走网络）。 */
export function writeCache<T>(key: string, value: T): void {
  try {
    localStorage.setItem(CACHE_PREFIX + key, JSON.stringify(value));
  } catch {
    // 配额/隐私模式：忽略
  }
}
