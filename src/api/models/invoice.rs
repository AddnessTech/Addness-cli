use serde::{Deserialize, Serialize};

// Invoice API models (GET /api/v1/team/invoices).
//
// The wire response is double-wrapped: `RespondWithPagination` wraps an
// already-enveloped `resources.InvoiceListResponse` (itself
// `{"data": [...], "pagination": {...}, "message": "success"}`) inside a
// second `{"data": <that>, "pagination": {...}, "message": "success"}`.
// `ApiResponse<InvoiceListInner>` captures only the outer `data` field, and
// `InvoiceListInner` captures the actual array + inner pagination — the
// duplicated outer `pagination`/`message` keys are simply ignored by serde.
// Backend reference: application/resources/invoice_resources.go,
// presentation/handlers/team/base_handler.go.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceListInner {
    #[serde(default)]
    pub data: Vec<Invoice>,
    pub pagination: InvoicePagination,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoicePagination {
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invoice {
    pub id: String,
    pub organization_id: String,
    pub amount: i64,
    pub status: String,
    pub currency: String,
    pub invoice_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment: Option<InvoicePayment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoicePayment {
    pub id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paid_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
