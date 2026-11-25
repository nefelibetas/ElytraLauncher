use elytra_domain::error::CoreError;
use elytra_domain::game::loader::version_query;

#[tauri::command]
pub async fn get_forge_versions(mc_version: String) -> Result<Vec<String>, CoreError> {
    version_query::forge_versions(&mc_version).await
}

#[tauri::command]
pub async fn get_fabric_versions(mc_version: String) -> Result<Vec<String>, CoreError> {
    version_query::fabric_versions(&mc_version).await
}

#[tauri::command]
pub async fn get_quilt_versions(mc_version: String) -> Result<Vec<String>, CoreError> {
    version_query::quilt_versions(&mc_version).await
}

#[tauri::command]
pub async fn get_neoforge_versions(mc_version: String) -> Result<Vec<String>, CoreError> {
    version_query::neoforge_versions(&mc_version).await
}

#[tauri::command]
pub async fn get_optifine_versions(mc_version: String) -> Result<Vec<String>, CoreError> {
    version_query::optifine_versions(&mc_version).await
}

#[tauri::command]
pub async fn get_fabric_api_versions(
    mc_version: String,
) -> Result<Vec<version_query::FabricApiVersion>, CoreError> {
    version_query::fabric_api_versions(&mc_version).await
}
