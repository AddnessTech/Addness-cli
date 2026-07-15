use anyhow::Result;

use crate::api::{ApiClient, ApiResponse, InvoiceListInner};

/// Query parameters for GET /api/v1/team/invoices.
#[derive(Default)]
pub struct InvoiceListParams<'a> {
    pub limit: Option<u16>,
    pub offset: Option<u16>,
    /// One of `created_at` / `issued_at` / `due_date`.
    pub sort_by: Option<&'a str>,
    /// `asc` / `desc`.
    pub sort_order: Option<&'a str>,
}

fn invoice_query_suffix(params: &InvoiceListParams<'_>) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    if let Some(limit) = params.limit {
        query.append_pair("limit", &limit.to_string());
    }
    if let Some(offset) = params.offset {
        query.append_pair("offset", &offset.to_string());
    }
    if let Some(sort_by) = params.sort_by {
        query.append_pair("sort_by", sort_by);
    }
    if let Some(sort_order) = params.sort_order {
        query.append_pair("sort_order", sort_order);
    }
    let query = query.finish();
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

impl ApiClient {
    /// GET /api/v1/team/invoices
    /// Requires the `x-organization-id` header (the client sends it
    /// automatically from `org_id`; the caller must resolve `--org` first).
    pub async fn list_invoices(&self, params: InvoiceListParams<'_>) -> Result<InvoiceListInner> {
        let path = format!("/api/v1/team/invoices{}", invoice_query_suffix(&params));
        let resp: ApiResponse<InvoiceListInner> = self.get(&path).await?;
        Ok(resp.data)
    }
}

#[cfg(test)]
mod tests {
    use super::{InvoiceListParams, invoice_query_suffix};

    #[test]
    fn invoice_query_suffix_is_empty_without_params() {
        assert_eq!(invoice_query_suffix(&InvoiceListParams::default()), "");
    }

    #[test]
    fn invoice_query_suffix_encodes_all_params() {
        let suffix = invoice_query_suffix(&InvoiceListParams {
            limit: Some(50),
            offset: Some(10),
            sort_by: Some("issued_at"),
            sort_order: Some("desc"),
        });
        assert_eq!(
            suffix,
            "?limit=50&offset=10&sort_by=issued_at&sort_order=desc"
        );
    }
}
