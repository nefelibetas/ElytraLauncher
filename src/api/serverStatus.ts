// 服务器状态探测 IPC：地址与版本类型原样传后端，Java/基岩识别在 Rust 侧完成。
import { invoke } from "@tauri-apps/api/core";
import type { ServerStatus } from "../types/server_status/ServerStatus";

export type { ServerStatus };
export type ServerEdition = "java" | "bedrock";

export function pingServer(
  address: string,
  edition: ServerEdition | "",
): Promise<ServerStatus> {
  return invoke<ServerStatus>("ping_server", { address, edition: edition || null });
}
