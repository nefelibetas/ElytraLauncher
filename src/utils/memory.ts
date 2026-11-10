// 内存滑块共用的口径与换算（全局设置 / 实例设置一致）。

/** 滑块与 -Xms 的下界（MB）。 */
export const MEM_FLOOR_MB = 512;
/** 滑块步进（MB）。 */
export const MEM_STEP_MB = 128;

/**
 * 自动分配基准（MB，0 mod）：可用内存 60%，上限压到 85%，下限 2GB。
 * 与后端 launcher::memory::auto_alloc_memory 同口径（后端再按实际 mod 数上调比例）。
 */
export function autoAllocMb(availMb: number): number {
  const avail = availMb > 0 ? availMb : 4096;
  return Math.floor(Math.min(Math.max(avail * 0.6, 2048), avail * 0.85));
}

/** 滑块轨道上限：可用内存（未就绪按 4GB），按步进对齐，且不低于下界 + 一格。 */
export function memCeilMb(availMb: number): number {
  const raw = Math.max(MEM_FLOOR_MB + MEM_STEP_MB, availMb > 0 ? availMb : 4096);
  const steps = Math.floor((raw - MEM_FLOOR_MB) / MEM_STEP_MB);
  return MEM_FLOOR_MB + steps * MEM_STEP_MB;
}

/** 吸附到步进网格并压进 [下界, 轨道上限]。 */
export function snapMemMb(mb: number, ceilMb: number): number {
  const steps = Math.round((mb - MEM_FLOOR_MB) / MEM_STEP_MB);
  return Math.min(ceilMb, Math.max(MEM_FLOOR_MB, MEM_FLOOR_MB + steps * MEM_STEP_MB));
}
