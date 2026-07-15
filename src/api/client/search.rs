use anyhow::Result;

use crate::api::{ApiClient, ApiResponse, SearchResponse};

/// Query parameters for GET /api/v1/team/search.
#[derive(Default)]
pub struct SearchQueryParams<'a> {
    pub query: &'a str,
    /// Required by the backend (`組織IDは必須です` if omitted); read from
    /// the query string, not the `x-organization-id` header.
    pub organization_id: &'a str,
    pub limit: Option<u16>,
    pub offset: Option<u16>,
}

fn search_query_suffix(params: &SearchQueryParams<'_>) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("q", params.query);
    query.append_pair("organizationId", params.organization_id);
    if let Some(limit) = params.limit {
        query.append_pair("limit", &limit.to_string());
    }
    if let Some(offset) = params.offset {
        query.append_pair("offset", &offset.to_string());
    }
    query.finish()
}

impl ApiClient {
    /// GET /api/v1/team/search (unified search across objectives/comments/members).
    pub async fn unified_search(&self, params: SearchQueryParams<'_>) -> Result<SearchResponse> {
        let path = format!("/api/v1/team/search?{}", search_query_suffix(&params));
        let resp: ApiResponse<SearchResponse> = self.get(&path).await?;
        Ok(resp.data)
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchQueryParams, search_query_suffix};

    #[test]
    fn search_query_suffix_encodes_query_and_org() {
        let suffix = search_query_suffix(&SearchQueryParams {
            query: "release plan",
            organization_id: "org-1",
            limit: None,
            offset: None,
        });
        assert_eq!(suffix, "q=release+plan&organizationId=org-1");
    }

    #[test]
    fn search_query_suffix_encodes_all_params() {
        let suffix = search_query_suffix(&SearchQueryParams {
            query: "#goal:completed launch",
            organization_id: "org-1",
            limit: Some(10),
            offset: Some(20),
        });
        assert_eq!(
            suffix,
            "q=%23goal%3Acompleted+launch&organizationId=org-1&limit=10&offset=20"
        );
    }
}
