// 前置 Mod 解析（镜像 Rust game::community::ModDependencyItem/Plan；命令 resolve_mod_dependencies）。
import { invoke } from "@tauri-apps/api/core";

export interface ModDependencyVersion {
	version_id: string;
	version_label: string;
	version_type: string;
}

export interface ModDependencyItem {
	project_id: string;
	version_id: string;
	name: string;
	version_label: string;
	loader: string;
	game_version: string;
	url: string;
	filename: string;
	relation: "required" | "optional" | string;
	root_optional: string | null;
	installed: boolean;
	installed_version_label: string | null;
	installed_files: string[];
	error: string | null;
	candidates: ModDependencyVersion[];
	candidate_total: number;
}

export interface ModDependencyPlan {
	instance_mc: string;
	instance_loader: string;
	items: ModDependencyItem[];
	errors: string[];
	installed_files: string[];
}

/** 目标实例中已安装的冲突 Mod（作者声明 incompatible 且 mods/ 中确实存在）。 */
export interface ModConflictItem {
	project_id: string;
	name: string;
	version_label: string;
	installed_files: string[];
}

export interface ModConflictReport {
	instance_mc: string;
	instance_loader: string;
	/** 选中文件是否适用于目标实例的 MC / ModLoader */
	version_applicable: boolean;
	conflicts: ModConflictItem[];
	/** 单个冲突项目查询失败；非空表示结果不完整 */
	errors: string[];
	/** 非失败提示（如冲突项被截断） */
	notices: string[];
}

export function fetchModConflicts(payload: {
	source: string;
	projectId: string;
	versionId: string;
	versionDirName: string;
}): Promise<ModConflictReport> {
	return invoke<ModConflictReport>("get_mod_conflicts", payload);
}

export function resolveModDependencies(payload: {
	source: string;
	projectId: string;
	versionId: string;
	versionDirName: string;
	versionOverrides?: Record<string, string>;
}): Promise<ModDependencyPlan> {
	return invoke<ModDependencyPlan>("resolve_mod_dependencies", payload);
}
