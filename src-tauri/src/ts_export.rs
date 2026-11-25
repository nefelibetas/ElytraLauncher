//! TypeScript 类型导出入口：`cargo run --bin gen_ts --features gen-ts`。
//!
//! 契约类型全部在 `elytra-dto`；本模块只转发，避免 Tauri 壳重复持有 ts-rs 依赖。

pub fn export_all() {
    elytra_dto::export_all();
}
