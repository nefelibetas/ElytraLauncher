// 社区详情的目标实例兼容判定（纯函数，供页面与 node:test 共用）。
import type { InstanceModTarget } from "../../types/game/InstanceModTarget";
import { loaderAccepts } from "./resourceKind.ts";

/** 版本文件/依赖项目的最小兼容字段。 */
export interface CompatFile {
  loaders: string[];
  game_versions: string[];
}

/** 文件自身声明的 loader 是否覆盖目标实例；空声明视为未知，不据此隐藏。 */
export function fileLoadersApply(file: CompatFile, target: InstanceModTarget): boolean {
  if (file.loaders.length === 0) return true;
  return file.loaders.some(loader => loaderAccepts(target.loader, loader));
}

/** 文件能否装入目标实例（loader + MC）；空字段按未知放行。 */
export function fileAppliesToInstance(file: CompatFile, target: InstanceModTarget): boolean {
  const mcOk =
    file.game_versions.length === 0 || file.game_versions.includes(target.mc_version);
  return fileLoadersApply(file, target) && mcOk;
}

/** 版本文件是否声明了 incompatible 依赖；没有时无需发起冲突检查。 */
export function hasDeclaredIncompat(dependencies: { relation: string }[]): boolean {
  return dependencies.some(dependency => dependency.relation === "incompatible");
}

/**
 * 依赖项目是否适用于目标实例。
 * 文件自身不适用时按声明原样展示（预览）：不能因为当前实例装不了该文件就把依赖一起藏掉。
 */
export function dependencyAppliesToInstance(
  depLoaders: string[],
  file: CompatFile,
  target: InstanceModTarget,
): boolean {
  if (!target.loader || depLoaders.length === 0 || !fileAppliesToInstance(file, target)) {
    return true;
  }
  return depLoaders.some(loader => loaderAccepts(target.loader, loader));
}
