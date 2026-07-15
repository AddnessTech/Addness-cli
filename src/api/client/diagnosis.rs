use anyhow::Result;
use serde_json::Value;

use crate::api::{
    ApiClient, ApiResponse, DiagnosisMemberProfile, DiagnosisMemberProfilesData,
    DiagnosisMyResultsData, DiagnosisResultByKindData, DiagnosisResultSummary, DiagnosisSaveData,
    DiagnosisSaveRequest, DiagnosisStats, DiagnosisVisibility, DiagnosisVisibilityRequest,
};

impl ApiClient {
    /// POST /api/v2/me/diagnosis-results (personal scope, no organization needed)
    pub async fn save_diagnosis_result(
        &self,
        diagnosis_kind: &str,
        schema_version: &str,
        result: Value,
    ) -> Result<DiagnosisSaveData> {
        let body = DiagnosisSaveRequest {
            diagnosis_kind: diagnosis_kind.to_string(),
            schema_version: schema_version.to_string(),
            result,
        };
        let resp: ApiResponse<DiagnosisSaveData> = self
            .post_without_org("/api/v2/me/diagnosis-results", &body)
            .await?;
        Ok(resp.data)
    }

    /// GET /api/v2/me/diagnosis-results (latest result per kind)
    pub async fn list_my_diagnosis_results(&self) -> Result<Vec<DiagnosisResultSummary>> {
        let resp: ApiResponse<DiagnosisMyResultsData> =
            self.get_without_org("/api/v2/me/diagnosis-results").await?;
        Ok(resp.data.results)
    }

    /// GET /api/v2/me/diagnosis-results/:kind
    pub async fn get_my_diagnosis_result(&self, kind: &str) -> Result<DiagnosisResultSummary> {
        let path = format!("/api/v2/me/diagnosis-results/{kind}");
        let resp: ApiResponse<DiagnosisResultByKindData> = self.get_without_org(&path).await?;
        Ok(resp.data.result)
    }

    /// GET /api/v1/public/diagnosis-results/stats (no auth required; anonymous
    /// aggregate). Only `goal_style`/`values` kinds are accepted by the backend.
    pub async fn get_diagnosis_stats(&self, kind: &str) -> Result<DiagnosisStats> {
        let path = format!("/api/v1/public/diagnosis-results/stats?diagnosis_kind={kind}");
        let resp: ApiResponse<DiagnosisStats> = self.get_without_org(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/me/diagnosis-visibility
    pub async fn get_diagnosis_visibility(&self, org_id: &str) -> Result<DiagnosisVisibility> {
        let path = format!("/api/v2/organizations/{org_id}/me/diagnosis-visibility");
        let resp: ApiResponse<DiagnosisVisibility> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// PATCH /api/v2/organizations/:id/me/diagnosis-visibility
    pub async fn update_diagnosis_visibility(
        &self,
        org_id: &str,
        visibilities: std::collections::HashMap<String, bool>,
    ) -> Result<DiagnosisVisibility> {
        let path = format!("/api/v2/organizations/{org_id}/me/diagnosis-visibility");
        let body = DiagnosisVisibilityRequest { visibilities };
        let resp: ApiResponse<DiagnosisVisibility> = self.patch(&path, &body).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/member-diagnosis-profiles?member_ids=a,b,c
    pub async fn list_member_diagnosis_profiles(
        &self,
        org_id: &str,
        member_ids: &[String],
    ) -> Result<Vec<DiagnosisMemberProfile>> {
        let path = format!(
            "/api/v2/organizations/{org_id}/member-diagnosis-profiles?member_ids={}",
            member_ids.join(",")
        );
        let resp: ApiResponse<DiagnosisMemberProfilesData> = self.get(&path).await?;
        Ok(resp.data.profiles)
    }

    /// GET /api/v2/organizations/:id/members/:memberId/diagnosis-profile
    pub async fn get_member_diagnosis_profile(
        &self,
        org_id: &str,
        member_id: &str,
    ) -> Result<DiagnosisMemberProfile> {
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/diagnosis-profile");
        let resp: ApiResponse<DiagnosisMemberProfile> = self.get(&path).await?;
        Ok(resp.data)
    }
}
