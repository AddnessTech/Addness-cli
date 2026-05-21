use crate::api::{ApiClient, ApiResponse, CreateKpiRequest, Kpi, UpdateKpiRequest};
use anyhow::Result;

impl ApiClient {
    pub async fn create_kpi(
        &self,
        goal_id: &str,
        req: &CreateKpiRequest,
    ) -> Result<ApiResponse<Kpi>> {
        let path = format!("/api/v2/objectives/{goal_id}/kpis");
        self.post(&path, req).await
    }

    pub async fn update_kpi(
        &self,
        kpi_id: &str,
        req: &UpdateKpiRequest,
    ) -> Result<ApiResponse<Kpi>> {
        let path = format!("/api/v2/objective-kpis/{kpi_id}");
        self.patch(&path, req).await
    }

    pub async fn delete_kpi(&self, kpi_id: &str) -> Result<()> {
        let path = format!("/api/v2/objective-kpis/{kpi_id}");
        self.delete_no_body(&path).await
    }
}
