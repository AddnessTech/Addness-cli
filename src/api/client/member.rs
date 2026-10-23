use anyhow::Result;
use serde_json::Value;

use crate::api::{
    ApiClient, ApiResponse, AssignMemberTagRequest, CreateMemberTagRequest, MemberTag,
    MembersListData, PinMemberRequest, SetSourceOrganizationRequest, UpdateMemberRequest,
};

/// Query parameters for `GET /api/v2/members` (`member browse`).
/// Organization is resolved purely from the `X-Organization-ID` header, so it
/// is not part of this struct; callers scope the `ApiClient` beforehand.
#[derive(Default)]
pub struct BrowseMembersParams<'a> {
    pub query: Option<&'a str>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub objective_id: Option<&'a str>,
    pub r#type: Option<&'a str>,
    pub tag_ids: Option<&'a str>,
    pub sort_by: Option<&'a str>,
    pub sort_dir: Option<&'a str>,
    pub target_member_id: Option<&'a str>,
}

impl ApiClient {
    pub async fn get_members(&self, org_id: &str) -> Result<ApiResponse<MembersListData>> {
        let path = format!("/api/v2/organizations/{org_id}/members?pageSize=100");
        self.get(&path).await
    }

    pub async fn update_member(&self, member_id: &str, name: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}");
        let body = UpdateMemberRequest {
            name: name.to_string(),
        };
        self.put_no_content(&path, &body).await
    }

    pub async fn pin_member(&self, member_id: &str, pinned: bool) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/pin");
        let body = PinMemberRequest { pinned };
        self.put_no_content(&path, &body).await
    }

    pub async fn delete_member(&self, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}");
        self.delete_no_body(&path).await
    }

    pub async fn assign_admin(&self, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/admin");
        self.put_empty_no_content(&path).await
    }

    pub async fn revoke_admin(&self, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/admin");
        self.delete_no_body(&path).await
    }

    pub async fn set_member_source_organization(
        &self,
        member_id: &str,
        source_org_id: &str,
    ) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/source-organization");
        let body = SetSourceOrganizationRequest {
            source_organization_id: source_org_id.to_string(),
        };
        self.patch_no_content(&path, &body).await
    }

    /// GET /api/v2/organizations/:id/members/search?name=&limit=
    pub async fn search_members(
        &self,
        org_id: &str,
        name: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Value> {
        let path = format!(
            "/api/v2/organizations/{org_id}/members/search{}",
            member_search_query_suffix(name, limit)
        );
        self.get(&path).await
    }

    /// GET /api/v2/organizations/:id/members/children?depth=
    pub async fn get_member_children(&self, org_id: &str, depth: Option<i64>) -> Result<Value> {
        let suffix = match depth {
            Some(depth) => format!("?depth={depth}"),
            None => String::new(),
        };
        let path = format!("/api/v2/organizations/{org_id}/members/children{suffix}");
        self.get(&path).await
    }

    /// GET /api/v2/organizations/:id/admins
    pub async fn list_organization_admins(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/admins");
        self.get(&path).await
    }

    /// GET /api/v2/members/:id/delete-preview
    /// Organization is resolved purely from the `X-Organization-ID` header.
    pub async fn get_member_delete_preview(&self, member_id: &str) -> Result<Value> {
        let path = format!("/api/v2/members/{member_id}/delete-preview");
        self.get(&path).await
    }

    /// GET /api/v2/members?query=&page=&pageSize=&objective_id=&type=&tag_ids=&sort_by=&sort_dir=&target_member_id=
    /// This is a distinct mount of the same Go handler as `get_members`
    /// (`GET /api/v2/organizations/:id/members`); organization is resolved
    /// purely from the `X-Organization-ID` header here.
    pub async fn browse_members(&self, params: &BrowseMembersParams<'_>) -> Result<Value> {
        let path = format!("/api/v2/members{}", browse_members_query_suffix(params));
        self.get(&path).await
    }

    /// GET /api/v2/members/:id/objectives
    /// Organization is resolved purely from the `X-Organization-ID` header.
    pub async fn get_member_objectives(&self, member_id: &str) -> Result<Value> {
        let path = format!("/api/v2/members/{member_id}/objectives");
        self.get(&path).await
    }

    /// PUT /api/v2/members/:id/avatar (raw file bytes as the request body)
    /// Organization is resolved purely from the `X-Organization-ID` header.
    pub async fn upload_member_avatar(
        &self,
        member_id: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<Value> {
        let path = format!("/api/v2/members/{member_id}/avatar");
        self.put_bytes(&path, bytes, content_type).await
    }

    /// GET /api/v2/members/:id
    /// Organization is resolved purely from the `X-Organization-ID` header.
    pub async fn get_member(&self, member_id: &str) -> Result<Value> {
        let path = format!("/api/v2/members/{member_id}");
        self.get(&path).await
    }

    // ---- Member tags ----

    /// GET /api/v2/organizations/:id/member-tags
    pub async fn list_member_tags(&self, org_id: &str) -> Result<ApiResponse<Vec<MemberTag>>> {
        let path = format!("/api/v2/organizations/{org_id}/member-tags");
        self.get(&path).await
    }

    /// POST /api/v2/organizations/:id/member-tags
    pub async fn create_member_tag(
        &self,
        org_id: &str,
        name: &str,
    ) -> Result<ApiResponse<MemberTag>> {
        let path = format!("/api/v2/organizations/{org_id}/member-tags");
        let body = CreateMemberTagRequest {
            name: name.to_string(),
        };
        self.post(&path, &body).await
    }

    /// DELETE /api/v2/organizations/:id/member-tags/:tagId
    pub async fn delete_member_tag(&self, org_id: &str, tag_id: &str) -> Result<()> {
        let path = format!("/api/v2/organizations/{org_id}/member-tags/{tag_id}");
        self.delete_no_body(&path).await
    }

    /// POST /api/v2/organizations/:id/members/:memberId/tags
    pub async fn assign_member_tag(
        &self,
        org_id: &str,
        member_id: &str,
        tag_id: &str,
    ) -> Result<()> {
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/tags");
        let body = AssignMemberTagRequest {
            tag_id: tag_id.to_string(),
        };
        self.post_no_content(&path, &body).await
    }

    /// GET /api/v2/members/:id/tags
    /// Organization is resolved from the `X-Organization-ID` header (or query/cookie
    /// on the Go side, but this client always sends the header).
    pub async fn list_member_tags_for_member(
        &self,
        member_id: &str,
    ) -> Result<ApiResponse<Vec<MemberTag>>> {
        let path = format!("/api/v2/members/{member_id}/tags");
        self.get(&path).await
    }

    /// DELETE /api/v2/members/:id/tags/:tagId
    /// Organization is resolved purely from the `X-Organization-ID` header.
    pub async fn unassign_member_tag(&self, member_id: &str, tag_id: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/tags/{tag_id}");
        self.delete_no_body(&path).await
    }
}

/// Build the `?name=&limit=` query suffix for `GET /api/v2/organizations/:id/members/search`.
fn member_search_query_suffix(name: Option<&str>, limit: Option<i64>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(name) = name {
            query.append_pair("name", name);
        }
        if let Some(limit) = limit {
            query.append_pair("limit", &limit.to_string());
        }
        query.finish()
    };
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

/// Build the query suffix for `GET /api/v2/members` (`member browse`).
fn browse_members_query_suffix(params: &BrowseMembersParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(q) = params.query {
            query.append_pair("query", q);
        }
        if let Some(page) = params.page {
            query.append_pair("page", &page.to_string());
        }
        if let Some(page_size) = params.page_size {
            query.append_pair("pageSize", &page_size.to_string());
        }
        if let Some(objective_id) = params.objective_id {
            query.append_pair("objective_id", objective_id);
        }
        if let Some(assignment_type) = params.r#type {
            query.append_pair("type", assignment_type);
        }
        if let Some(tag_ids) = params.tag_ids {
            query.append_pair("tag_ids", tag_ids);
        }
        if let Some(sort_by) = params.sort_by {
            query.append_pair("sort_by", sort_by);
        }
        if let Some(sort_dir) = params.sort_dir {
            query.append_pair("sort_dir", sort_dir);
        }
        if let Some(target_member_id) = params.target_member_id {
            query.append_pair("target_member_id", target_member_id);
        }
        query.finish()
    };
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

#[cfg(test)]
mod tests {
    use super::{BrowseMembersParams, browse_members_query_suffix, member_search_query_suffix};

    #[test]
    fn member_search_query_suffix_is_empty_without_params() {
        assert_eq!(member_search_query_suffix(None, None), "");
    }

    #[test]
    fn member_search_query_suffix_encodes_all_params() {
        assert_eq!(
            member_search_query_suffix(Some("Jane Doe"), Some(10)),
            "?name=Jane+Doe&limit=10"
        );
    }

    #[test]
    fn browse_members_query_suffix_is_empty_without_params() {
        assert_eq!(
            browse_members_query_suffix(&BrowseMembersParams::default()),
            ""
        );
    }

    #[test]
    fn browse_members_query_suffix_encodes_all_params() {
        let suffix = browse_members_query_suffix(&BrowseMembersParams {
            query: Some("acme"),
            page: Some(2),
            page_size: Some(20),
            objective_id: Some("obj-1"),
            r#type: Some("member,editor"),
            tag_ids: Some("tag-1,tag-2"),
            sort_by: Some("name"),
            sort_dir: Some("asc"),
            target_member_id: Some("mem-1"),
        });
        assert_eq!(
            suffix,
            "?query=acme&page=2&pageSize=20&objective_id=obj-1&type=member%2Ceditor&tag_ids=tag-1%2Ctag-2&sort_by=name&sort_dir=asc&target_member_id=mem-1"
        );
    }
}
