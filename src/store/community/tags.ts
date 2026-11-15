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

/** 整合包内容分类；CF 侧按 classId=4471 的 categoryId，MR 侧按 modpack 的 categories。 */
export const modpackTags: CommunityTag[] = [
  { value: "", labelKey: "download.search.all" },
  { value: "4472/technology", labelKey: "download.tags.technology" },
  { value: "4473/magic", labelKey: "download.tags.magic" },
  { value: "4475/adventure", labelKey: "download.tags.adventure" },
  { value: "4484/multiplayer", labelKey: "download.tags.multiplayer" },
  { value: "/optimization", labelKey: "download.tags.optimization" },
  { value: "4479/challenging", labelKey: "download.tags.hardcore" },
  { value: "4483/combat", labelKey: "download.tags.combat" },
  { value: "4478/quests", labelKey: "download.tags.quests" },
  { value: "/kitchen-sink", labelKey: "download.tags.kitchenSink" },
  { value: "4476/", labelKey: "download.tags.exploration" },
  { value: "4477/", labelKey: "download.tags.minigame" },
  { value: "4474/", labelKey: "download.tags.scifi" },
  { value: "4736/", labelKey: "download.tags.skyblock" },
  { value: "5128/", labelKey: "download.tags.vanillaPlus" },
  { value: "4487/", labelKey: "download.tags.ftb" },
  { value: "4480/", labelKey: "download.tags.mapBased" },
  { value: "4481/lightweight", labelKey: "download.tags.lightweight" },
  { value: "4482/", labelKey: "download.tags.large" },
  { value: "7418/", labelKey: "download.tags.horror" },
  { value: "9243/", labelKey: "download.tags.expert" },
  { value: "10683/", labelKey: "download.tags.rlcraft" },
];

/** 当前分类是否被指定来源支持；无斜杠的异常值按“全部”处理。 */
export function tagSupports(tag: string, source: CommunitySource): boolean {
  if (!tag || !tag.includes("/")) return true;
  const [cf, mr] = tag.split("/");
  return source === "curseforge" ? cf !== "" : mr !== "";
}
