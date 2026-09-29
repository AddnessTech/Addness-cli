use anyhow::{Result, bail};
use reqwest::Response;
use serde_json::{Value, json};

use crate::api::{ApiClient, ApiResponse};

pub struct FormListParams<'a> {
    pub goal_id: Option<&'a str>,
    pub query: Option<&'a str>,
    pub limit: Option<u16>,
    pub cursor: Option<&'a str>,
    pub trash: bool,
}

pub struct FormResponseListParams<'a> {
    pub limit: Option<u16>,
    pub cursor: Option<&'a str>,
    pub submitted_at_from: Option<&'a str>,
    pub submitted_at_before: Option<&'a str>,
}

fn forms_path(org_id: &str) -> String {
    format!("/api/v2/organizations/{org_id}/forms")
}

fn form_path(org_id: &str, form_id: &str) -> String {
    format!("{}/{form_id}", forms_path(org_id))
}

impl ApiClient {
    pub async fn list_forms(&self, org_id: &str, params: FormListParams<'_>) -> Result<Value> {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("summaryOnly", "true");
        if params.trash {
            query.append_pair("trash", "true");
        }
        if let Some(goal_id) = params.goal_id {
            query.append_pair("goalId", goal_id);
        }
        if let Some(search) = params.query {
            query.append_pair("query", search);
        }
        if let Some(limit) = params.limit {
            query.append_pair("limit", &limit.to_string());
        }
        if let Some(cursor) = params.cursor {
            query.append_pair("cursor", cursor);
        }
        let path = format!("{}?{}", forms_path(org_id), query.finish());
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn create_form(&self, org_id: &str, definition: &Value) -> Result<Value> {
        let resp: ApiResponse<Value> = self.post(&forms_path(org_id), definition).await?;
        Ok(resp.data)
    }

    pub async fn get_form(&self, org_id: &str, form_id: &str) -> Result<Value> {
        let resp: ApiResponse<Value> = self.get(&form_path(org_id, form_id)).await?;
        Ok(resp.data)
    }

    pub async fn replace_form(&self, org_id: &str, form_id: &str, body: &Value) -> Result<Value> {
        let resp: ApiResponse<Value> = self.put(&form_path(org_id, form_id), body).await?;
        Ok(resp.data)
    }

    pub async fn patch_form(&self, org_id: &str, form_id: &str, body: &Value) -> Result<Value> {
        let resp: ApiResponse<Value> = self.patch(&form_path(org_id, form_id), body).await?;
        Ok(resp.data)
    }

    pub async fn delete_form(&self, org_id: &str, form_id: &str) -> Result<()> {
        self.delete_no_body(&form_path(org_id, form_id)).await
    }

    pub async fn restore_form(&self, org_id: &str, form_id: &str) -> Result<Value> {
        let path = format!("{}/restore", form_path(org_id, form_id));
        let resp: ApiResponse<Value> = self.post(&path, &json!({})).await?;
        Ok(resp.data)
    }

    pub async fn permanently_delete_form(&self, org_id: &str, form_id: &str) -> Result<()> {
        let path = format!("{}/permanent", form_path(org_id, form_id));
        self.delete_no_body(&path).await
    }

    pub async fn change_form_state(
        &self,
        org_id: &str,
        form_id: &str,
        action: &str,
        revision: u64,
    ) -> Result<Value> {
        let path = format!("{}/{action}", form_path(org_id, form_id));
        let resp: ApiResponse<Value> = self.post(&path, &json!({ "revision": revision })).await?;
        Ok(resp.data)
    }

    pub async fn list_form_responses(
        &self,
        org_id: &str,
        form_id: &str,
        params: FormResponseListParams<'_>,
    ) -> Result<Value> {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(limit) = params.limit {
            query.append_pair("limit", &limit.to_string());
        }
        if let Some(cursor) = params.cursor {
            query.append_pair("cursor", cursor);
        }
        if let Some(from) = params.submitted_at_from {
            query.append_pair("submittedAtFrom", from);
        }
        if let Some(before) = params.submitted_at_before {
            query.append_pair("submittedAtBefore", before);
        }
        let path = format!(
            "{}/responses?{}",
            form_path(org_id, form_id),
            query.finish()
        );
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn get_form_response(
        &self,
        org_id: &str,
        form_id: &str,
        response_id: &str,
    ) -> Result<Value> {
        let path = format!("{}/responses/{response_id}", form_path(org_id, form_id));
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn delete_form_response(
        &self,
        org_id: &str,
        form_id: &str,
        response_id: &str,
    ) -> Result<()> {
        let path = format!("{}/responses/{response_id}", form_path(org_id, form_id));
        self.delete_no_body(&path).await
    }

    pub async fn delete_all_form_responses(&self, org_id: &str, form_id: &str) -> Result<()> {
        let path = format!("{}/responses", form_path(org_id, form_id));
        self.delete_no_body(&path).await
    }

    pub async fn get_form_summary(
        &self,
        org_id: &str,
        form_id: &str,
        time_zone: Option<&str>,
        submitted_at_from: Option<&str>,
        submitted_at_before: Option<&str>,
    ) -> Result<Value> {
        let mut path = format!("{}/summary", form_path(org_id, form_id));
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(zone) = time_zone {
            if zone.is_empty() || zone.len() > 64 {
                bail!("time-zone must be a nonempty IANA time zone of at most 64 bytes");
            }
            query.append_pair("timeZone", zone);
        }
        if let Some(from) = submitted_at_from {
            query.append_pair("submittedAtFrom", from);
        }
        if let Some(before) = submitted_at_before {
            query.append_pair("submittedAtBefore", before);
        }
        let query = query.finish();
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query);
        }
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn get_form_responses_csv(
        &self,
        org_id: &str,
        form_id: &str,
        max_bytes: u32,
        view: &str,
        submitted_at_from: Option<&str>,
        submitted_at_before: Option<&str>,
    ) -> Result<Response> {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("maxBytes", &max_bytes.to_string());
        match view {
            "raw" => {}
            "labels" => {
                query.append_pair("view", view);
            }
            _ => bail!("Invalid CSV view: {view}"),
        }
        if let Some(from) = submitted_at_from {
            query.append_pair("submittedAtFrom", from);
        }
        if let Some(before) = submitted_at_before {
            query.append_pair("submittedAtBefore", before);
        }
        let path = format!(
            "{}/responses.csv?{}",
            form_path(org_id, form_id),
            query.finish()
        );
        self.get_raw(&path).await
    }
}
