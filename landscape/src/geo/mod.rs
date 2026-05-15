pub mod ip_service;
pub mod site_service;

#[derive(Clone, Debug, Default)]
pub struct GeoRefreshRuntimeStatus {
    pub last_success_at: Option<f64>,
    pub last_error: Option<String>,
}
