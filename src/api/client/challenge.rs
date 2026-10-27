use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api::ApiClient;

fn base(org: &str, platform: &str) -> Result<String> {
    Uuid::parse_str(org).context("Invalid organization ID")?;
    if !matches!(platform, "pc" | "mobile") {
        bail!("platform must be pc or mobile");
    }
    Ok(format!("/api/v2/organizations/{org}/challenges"))
}

fn key_path(org: &str, key: &str, platform: &str) -> Result<String> {
    if key.is_empty()
        || !key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        bail!("Invalid challenge key");
    }
    Ok(format!("{}/{key}", base(org, platform)?))
}

fn run_path(org: &str, key: &str, run: &str, platform: &str) -> Result<String> {
    Uuid::parse_str(run).context("Invalid run ID")?;
    Ok(format!("{}/runs/{run}", key_path(org, key, platform)?))
}

impl ApiClient {
    pub async fn challenge_list(
        &self,
        org: &str,
        platform: &str,
        defs: u32,
        summary: bool,
    ) -> Result<Value> {
        let suffix = if summary { "/summary" } else { "" };
        self.get(&format!(
            "{}{suffix}?platform={platform}&defs={defs}",
            base(org, platform)?
        ))
        .await
    }

    pub async fn challenge_read(&self, org: &str, platform: &str, resource: &str) -> Result<Value> {
        self.get(&format!(
            "{}/{resource}?platform={platform}",
            base(org, platform)?
        ))
        .await
    }

    pub async fn challenge_start(&self, org: &str, key: &str, platform: &str) -> Result<Value> {
        self.post_empty(&format!(
            "{}/runs?platform={platform}",
            key_path(org, key, platform)?
        ))
        .await
    }

    pub async fn challenge_ticket(
        &self,
        org: &str,
        key: &str,
        platform: &str,
        source: &str,
    ) -> Result<Value> {
        self.post(
            &format!(
                "{}/tickets?platform={platform}",
                key_path(org, key, platform)?
            ),
            &json!({"source": source}),
        )
        .await
    }

    pub async fn challenge_verify(
        &self,
        org: &str,
        key: &str,
        run: &str,
        platform: &str,
        watch: bool,
    ) -> Result<Value> {
        let suffix = if watch { "/watch" } else { "" };
        self.get(&format!(
            "{}{suffix}?platform={platform}",
            run_path(org, key, run, platform)?
        ))
        .await
    }

    pub async fn challenge_complete(
        &self,
        org: &str,
        key: &str,
        run: &str,
        platform: &str,
        steps: &[String],
    ) -> Result<Value> {
        self.post(
            &format!(
                "{}/complete?platform={platform}",
                run_path(org, key, run, platform)?
            ),
            &json!({"manualSteps": steps}),
        )
        .await
    }

    pub async fn challenge_target(
        &self,
        org: &str,
        key: &str,
        run: &str,
        platform: &str,
        target: &Value,
    ) -> Result<Value> {
        self.put_no_content(
            &format!(
                "{}/target?platform={platform}",
                run_path(org, key, run, platform)?
            ),
            target,
        )
        .await?;
        Ok(json!({"ok": true}))
    }

    pub async fn challenge_skip(
        &self,
        org: &str,
        key: &str,
        platform: &str,
        undo: bool,
    ) -> Result<Value> {
        let path = format!("{}/skip?platform={platform}", key_path(org, key, platform)?);
        if !undo {
            self.post_empty_no_content(&path).await?;
            return Ok(json!({"ok": true}));
        }
        let (url, req) = self.request(Method::DELETE, &path, true)?;
        let response = self.send(req, &url).await?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(json!({"ok": true}));
        }
        let bytes = response
            .bytes()
            .await
            .context("Failed to read challenge response")?;
        Self::parse_json_bytes(&bytes, &url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::mcp::tests::server;

    const ORG: &str = "79e5df38-ffbb-4a95-9d41-f5968427c653";
    const RUN: &str = "11111111-2222-4333-8444-555555555555";

    #[tokio::test]
    async fn challenge_requests_keep_server_data_and_send_only_explicit_manual_steps() {
        let (client, task) = server(vec![
            (
                "200 OK",
                json!({"challenges":[{"key":"crm_list","state":"open"}]}).to_string(),
            ),
            ("201 Created", json!({"runId":RUN}).to_string()),
            (
                "200 OK",
                json!({"steps":[{"key":"see_lists","satisfied":null}]}).to_string(),
            ),
            ("200 OK", json!({"points":10}).to_string()),
            ("204 No Content", String::new()),
            ("200 OK", json!({"runId":RUN}).to_string()),
        ]);
        let client = client.with_org_id(Some(ORG.to_string()));
        assert_eq!(
            client.challenge_list(ORG, "pc", 3, false).await.unwrap()["challenges"][0]["key"],
            "crm_list"
        );
        assert_eq!(
            client.challenge_start(ORG, "crm_list", "pc").await.unwrap()["runId"],
            RUN
        );
        client
            .challenge_verify(ORG, "crm_list", RUN, "mobile", false)
            .await
            .unwrap();
        client
            .challenge_complete(ORG, "crm_list", RUN, "pc", &["see_lists".into()])
            .await
            .unwrap();
        assert_eq!(
            client
                .challenge_skip(ORG, "crm_list", "pc", true)
                .await
                .unwrap()["ok"],
            true
        );
        assert_eq!(
            client
                .challenge_skip(ORG, "first_challenge", "pc", true)
                .await
                .unwrap()["runId"],
            RUN
        );
        let requests = task.join().unwrap();
        assert!(requests[0].starts_with(&format!(
            "GET /api/v2/organizations/{ORG}/challenges?platform=pc&defs=3 "
        )));
        assert!(requests[1].starts_with(&format!(
            "POST /api/v2/organizations/{ORG}/challenges/crm_list/runs?platform=pc "
        )));
        assert!(requests[2].starts_with(&format!(
            "GET /api/v2/organizations/{ORG}/challenges/crm_list/runs/{RUN}?platform=mobile "
        )));
        assert!(requests[3].starts_with(&format!(
            "POST /api/v2/organizations/{ORG}/challenges/crm_list/runs/{RUN}/complete?platform=pc "
        )));
        let body: Value =
            serde_json::from_str(requests[3].split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body, json!({"manualSteps":["see_lists"]}));
        assert!(
            requests
                .iter()
                .all(|r| r.contains(&format!("x-organization-id: {ORG}")))
        );
    }

    #[tokio::test]
    async fn challenge_rejects_bad_path_segments_before_sending() {
        let client = ApiClient::new("token", "http://127.0.0.1:1").unwrap();
        assert!(
            client
                .challenge_start(ORG, "../other", "pc")
                .await
                .unwrap_err()
                .to_string()
                .contains("Invalid challenge key")
        );
        assert!(
            client
                .challenge_verify(ORG, "crm_list", "not-a-run", "pc", false)
                .await
                .unwrap_err()
                .to_string()
                .contains("Invalid run ID")
        );
        assert!(
            client
                .challenge_list("invalid", "pc", 3, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("Invalid organization ID")
        );
    }
}
