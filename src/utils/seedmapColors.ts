// 种子预览结构候选色：画布散点与图例共用，避免两处色值漂移。
// 结构色属于地图数据语义，不随主题变化。

export const STRUCTURE_COLORS: Record<string, string> = {
  village: "#f2c14e",
  desert_pyramid: "#e0b15a",
  jungle_temple: "#5fbf77",
  swamp_hut: "#7a9e5a",
  igloo: "#bfe6ff",
  ocean_ruin: "#7bc8d9",
  shipwreck: "#b08b62",
  monument: "#59b6a5",
  mansion: "#b28bd6",
  outpost: "#db7b6b",
  ruined_portal: "#c084fc",
  ancient_city: "#6d7cff",
  treasure: "#f59e0b",
  trail_ruins: "#d4a373",
  trial_chambers: "#e879f9",
  abandoned_camp: "#f97316",
};

/** 未知结构类型回退中性灰。 */
export function structureColor(kind: string): string {
  return STRUCTURE_COLORS[kind] ?? "#e5e7eb";
}
