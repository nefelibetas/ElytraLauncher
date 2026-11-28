// 存档玩家最后位置（后端 elytra_map::players::MapPlayer，按最后游玩时间降序）。
export type MapPlayer = {
  uuid: string;
  name: string;
  x: number;
  z: number;
  /** 与 MapWorldInfo.dimensions 一致的维度 id。 */
  dimension: string;
  /** 最后保存时间（Unix 毫秒）；无文件时间戳时为 null。 */
  last_played: number | null;
};
