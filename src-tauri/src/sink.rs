//! Tauri 事件出口：把 core 的事件投递到 AppHandle，沿用合并派发线程。

use std::sync::Arc;

use elytra_domain::task::{DispatchSink, EventSink};
use tauri::Emitter;

pub(crate) fn sink(app: &tauri::AppHandle) -> Arc<dyn EventSink> {
    let app = app.clone();
    Arc::new(DispatchSink::new(move |event, payload| {
        let _ = app.emit(event, payload);
    }))
}
