// 社区资源内容分类：value = "CurseForge 分类 ID/Modrinth 分类 slug"；空侧表示该源无对应分类。
import type { CommunitySource } from "./rank";

export interface CommunityTag {
  value: string;
  labelKey: string;
}

/** Mod 内容分类；顺序按常用度排列，首项为“全部”。 */
export const modTags: CommunityTag[] = [
  { value: "", labelKey: "download.search.all" },
  { value: "412/technology", labelKey: "download.tags.technology" },
  { value: "419/magic", labelKey: "download.tags.magic" },
  { value: "422/adventure", labelKey: "download.tags.adventure" },
  { value: "424/decoration", labelKey: "download.tags.decoration" },
  { value: "420/storage", labelKey: "download.tags.storage" },
  { value: "5191/utility", labelKey: "download.tags.utility" },
  { value: "6814/optimization", labelKey: "download.tags.optimization" },
  { value: "406/worldgen", labelKey: "download.tags.worldgen" },
  { value: "407/", labelKey: "download.tags.biomes" },
  { value: "410/", labelKey: "download.tags.dimensions" },
  { value: "408/", labelKey: "download.tags.ores" },
  { value: "409/", labelKey: "download.tags.structures" },
  { value: "415/", labelKey: "download.tags.pipes" },
  { value: "4843/", labelKey: "download.tags.automation" },
  { value: "417/", labelKey: "download.tags.energy" },
  { value: "4558/", labelKey: "download.tags.redstone" },
  { value: "436/food", labelKey: "download.tags.food" },
  { value: "416/", labelKey: "download.tags.farming" },
  { value: "/game-mechanics", labelKey: "download.tags.gameMechanics" },
  { value: "414/transportation", labelKey: "download.tags.transportation" },
  { value: "411/mobs", labelKey: "download.tags.mobs" },
  { value: "434/equipment", labelKey: "download.tags.equipment" },
  { value: "9026/", labelKey: "download.tags.creative" },
  { value: "423/", labelKey: "download.tags.info" },
  { value: "435/social", labelKey: "download.tags.server" },
  { value: "421/library", labelKey: "download.tags.library" },
];

/** 当前分类是否被指定来源支持；无斜杠的异常值按“全部”处理。 */
export function tagSupports(tag: string, source: CommunitySource): boolean {
  if (!tag || !tag.includes("/")) return true;
  const [cf, mr] = tag.split("/");
  return source === "curseforge" ? cf !== "" : mr !== "";
}
