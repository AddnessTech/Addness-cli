use anyhow::Result;
use serde_json::Value;

use super::{ApiClient, issue::encode_path_segment};

pub struct CrmListParams<'a> {
    pub query: Option<&'a str>,
    pub owner: Option<&'a str>,
    pub folder_path: Option<&'a str>,
    pub tag_names: &'a [String],
    pub cursor: Option<&'a str>,
    pub limit: u16,
}

pub struct CrmRowsParams<'a> {
    pub filter: &'a str,
    pub stage_key: Option<&'a str>,
    pub assignee_member_id: Option<&'a str>,
    pub cursor: Option<&'a str>,
    pub limit: u16,
}

impl ApiClient {
    pub async fn list_crm_lists(&self, params: &CrmListParams<'_>) -> Result<Value> {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("limit", &params.limit.to_string());
        for (name, value) in [
            ("q", params.query),
            ("owner", params.owner),
            ("folder_path", params.folder_path),
            ("cursor", params.cursor),
        ] {
            if let Some(value) = value {
                query.append_pair(name, value);
            }
        }
        for tag in params.tag_names {
            query.append_pair("tag_names", tag);
        }
        self.get(&format!("/api/v2/crm/lists?{}", query.finish()))
            .await
    }

    pub async fn get_crm_list(&self, id: &str) -> Result<Value> {
        let id = encode_path_segment(id);
        self.get(&format!("/api/v2/crm/lists/{id}")).await
    }

    pub async fn get_crm_rows(&self, id: &str, params: &CrmRowsParams<'_>) -> Result<Value> {
        let id = encode_path_segment(id);
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("limit", &params.limit.to_string());
        query.append_pair("filter", params.filter);
        for (name, value) in [
            ("stage_key", params.stage_key),
            ("assignee_member_id", params.assignee_member_id),
            ("cursor", params.cursor),
        ] {
            if let Some(value) = value {
                query.append_pair(name, value);
            }
        }
        self.get(&format!("/api/v2/crm/lists/{id}/rows?{}", query.finish()))
            .await
    }
}
