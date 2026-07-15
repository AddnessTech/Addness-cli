use serde::{Deserialize, Serialize};

// GET /api/v2/organizations/me
// Response: { "data": { "organizations": [ { "id": "...", "name": "...", ... } ] }, "message": "success" }
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub context_text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationsData {
    pub organizations: Vec<Organization>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationsResponse {
    pub data: OrganizationsData,
}

// POST /api/v1/team/organizations
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrganizationRequest {
    pub name: String,
    /// PERSONAL or BUSINESS. Backend requires this field.
    #[serde(rename = "type")]
    pub organization_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    /// Required when organization_type is BUSINESS. One of SOLO, 2_5, 6_20, 21_50, 50_PLUS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_scale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub industry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_timezone: Option<String>,
}

// PUT/PATCH /api/v2/organizations/:id
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrganizationRequest {
    pub name: String,
}

// PATCH /api/v2/organizations/:id/context
#[derive(Debug, Serialize)]
pub struct UpdateContextRequest {
    pub context_text: String,
}

// POST /api/v1/team/organizations/:id/push_tokens
// Backend binds `{"token": "..."}` (PushTokenRegisterRequest).
#[derive(Debug, Serialize)]
pub struct PushTokenRegisterRequest {
    pub token: String,
}

// POST /api/v1/team/organization_subscriptions/register
// Backend binds `{"univapaySubscriptionId": "..."}` (RegisterSubscriptionRequest).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterSubscriptionRequest {
    pub univapay_subscription_id: String,
}

// PUT/PATCH /api/v2/organizations/:id/default-timezone
// Backend binds `{"default_timezone": "..."}` (snake_case, unlike most v2 bodies).
#[derive(Debug, Serialize)]
pub struct UpdateDefaultTimezoneRequest {
    pub default_timezone: String,
}

// PUT /api/v2/organizations/:id/ai-schedule-settings
// PUT /api/v2/organizations/:id/ad-settings
// Both bind `{"enabled": <bool>}`; the backend treats the field as required so it
// can distinguish "omitted" from an explicit `false`.
#[derive(Debug, Serialize)]
pub struct EnabledFlagRequest {
    pub enabled: bool,
}

// PUT /api/v2/organizations/:id/ad-settings/me
// Backend binds `{"enabled"?: bool, "hiddenUntil"?: RFC3339}` and requires at
// least one of the two fields to be present.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyAdSettingRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_until: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        EnabledFlagRequest, MyAdSettingRequest, PushTokenRegisterRequest,
        RegisterSubscriptionRequest, UpdateDefaultTimezoneRequest,
    };

    #[test]
    fn register_subscription_request_uses_camel_case_key() {
        let json = serde_json::to_value(RegisterSubscriptionRequest {
            univapay_subscription_id: "sub_123".to_string(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"univapaySubscriptionId": "sub_123"})
        );
    }

    #[test]
    fn update_default_timezone_request_uses_snake_case_key() {
        let json = serde_json::to_value(UpdateDefaultTimezoneRequest {
            default_timezone: "Asia/Tokyo".to_string(),
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({"default_timezone": "Asia/Tokyo"}));
    }

    #[test]
    fn enabled_flag_request_serializes_boolean() {
        let json = serde_json::to_value(EnabledFlagRequest { enabled: false }).unwrap();
        assert_eq!(json, serde_json::json!({"enabled": false}));
    }

    #[test]
    fn push_token_request_serializes_token() {
        let json = serde_json::to_value(PushTokenRegisterRequest {
            token: "tok_abc".to_string(),
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({"token": "tok_abc"}));
    }

    #[test]
    fn my_ad_setting_request_omits_unset_fields() {
        let json = serde_json::to_value(MyAdSettingRequest {
            enabled: Some(true),
            hidden_until: None,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({"enabled": true}));
    }

    #[test]
    fn my_ad_setting_request_renames_hidden_until() {
        let json = serde_json::to_value(MyAdSettingRequest {
            enabled: None,
            hidden_until: Some("2026-07-16T00:00:00Z".to_string()),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"hiddenUntil": "2026-07-16T00:00:00Z"})
        );
    }
}
