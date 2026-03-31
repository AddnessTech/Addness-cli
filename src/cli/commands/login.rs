use std::convert::Infallible;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use oauth2::PkceCodeChallenge;
use rand::rngs::OsRng;
use spki::EncodePublicKey;

use crate::config::{Credentials, Settings};

#[derive(serde::Deserialize)]
struct StartSessionResponse {
    data: StartSessionData,
}

#[derive(serde::Deserialize)]
struct StartSessionData {
    start_token: String,
}

#[derive(serde::Deserialize)]
struct ExchangeResponse {
    data: ExchangeData,
}

#[derive(serde::Deserialize)]
struct ExchangeData {
    api_key: Option<String>,
    organizations: Option<Vec<OrgInfo>>,
}

#[derive(serde::Deserialize)]
struct OrgInfo {
    id: String,
    name: String,
}

#[derive(serde::Deserialize)]
struct RegisterResponse {
    #[allow(dead_code)]
    data: RegisterData,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct RegisterData {
    installation_id: String,
}

fn generate_keypair() -> (SigningKey, String) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let pem = verifying_key
        .to_public_key_pem(spki::der::pem::LineEnding::LF)
        .expect("failed to encode public key as PEM");
    (signing_key, pem)
}

fn sign_message(signing_key: &SigningKey, message: &str) -> String {
    let signature = signing_key.sign(message.as_bytes());
    URL_SAFE_NO_PAD.encode(signature.to_bytes())
}

fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs() as i64
}

pub async fn handle_login(api_url: &str, frontend_url: Option<&str>) -> Result<()> {
    println!("Logging in to Addness...");
    println!();

    // 1. Ed25519キーペア生成
    let (signing_key, public_key_pem) = generate_keypair();
    let installation_id = uuid::Uuid::new_v4().simple().to_string();

    // 2. localhostサーバー起動（空きポートを自動取得）
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("Failed to bind localhost port")?;
    let port = listener.local_addr()?.port();

    // 3. PKCE用のcode_verifierとcode_challenge生成（oauth2 crateを使用）
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let code_challenge = pkce_challenge.as_str().to_string();
    let code_verifier = pkce_verifier.secret().to_string();

    // 4. State生成
    let state = uuid::Uuid::new_v4().simple().to_string();

    // 5. Installation登録
    let client = reqwest::Client::new();
    let register_resp = client
        .post(format!(
            "{api_url}/api/v1/public/desktop/auth/installations/register"
        ))
        .json(&serde_json::json!({
            "installationId": installation_id,
            "publicKey": public_key_pem,
        }))
        .send()
        .await?;

    if !register_resp.status().is_success() {
        let body = register_resp.text().await?;
        bail!("Failed to register installation: {body}");
    }
    let _: RegisterResponse = register_resp.json().await?;

    // 6. StartSession作成
    let ts = timestamp();
    let start_message = format!(
        r#"visiontodo-desktop-auth-start
installation_id={installation_id}
state={state}
port={port}
next_path={}
code_challenge={code_challenge}
auth_path={}
referral_code={}
timestamp={ts}"#,
        "/organization/set", "/sign-in", ""
    );
    let start_signature = sign_message(&signing_key, &start_message);

    let start_resp = client
        .post(format!(
            "{api_url}/api/v1/public/desktop/auth/start-sessions"
        ))
        .json(&serde_json::json!({
            "installationId": installation_id,
            "state": state,
            "port": port.to_string(),
            "nextPath": "/organization/set",
            "codeChallenge": code_challenge,
            "authPath": "/sign-in",
            "timestamp": ts,
            "signature": start_signature,
        }))
        .send()
        .await?;

    if !start_resp.status().is_success() {
        let body = start_resp.text().await?;
        bail!("Failed to create start session: {body}");
    }

    let start_data: StartSessionResponse = start_resp.json().await?;
    let start_token = start_data.data.start_token;

    // 7. ブラウザを開く
    let fe_url = frontend_url
        .map(|s| s.to_string())
        .unwrap_or_else(|| api_url.replace(":8080", ":3000").replace("api.", ""));
    let fe_base = fe_url.trim_end_matches('/');
    let browser_url = format!("{fe_base}/desktop/browser-auth?start_token={start_token}");

    println!("Opening browser for login...");
    println!("If the browser doesn't open, visit:");
    println!("  {browser_url}");
    println!();

    if open::that(&browser_url).is_err() {
        println!("Could not open browser automatically.");
    }

    println!("Waiting for login...");

    // 8. localhostでコールバックを待機（hyperでHTTPリクエストを処理）
    let (handoff_id, callback_state) =
        tokio::time::timeout(Duration::from_secs(300), wait_for_callback(listener))
            .await
            .map_err(|_| anyhow::anyhow!("Login timed out (5 minutes). Please try again."))??;

    if callback_state != state {
        bail!("State mismatch: expected {}, got {}", state, callback_state);
    }

    // 9. Token Exchange（source=cli でAPI Key自動発行）
    let ts = timestamp();
    let exchange_message = format!(
        r#"visiontodo-desktop-auth-exchange
installation_id={installation_id}
handoff_id={handoff_id}
code_verifier={code_verifier}
timestamp={ts}"#
    );
    let exchange_signature = sign_message(&signing_key, &exchange_message);

    let exchange_resp = client
        .post(format!(
            "{api_url}/api/v1/public/desktop/auth/token-exchange"
        ))
        .json(&serde_json::json!({
            "handoffId": handoff_id,
            "codeVerifier": code_verifier,
            "installationId": installation_id,
            "timestamp": ts,
            "signature": exchange_signature,
            "source": "cli",
        }))
        .send()
        .await?;

    if !exchange_resp.status().is_success() {
        let body = exchange_resp.text().await?;
        bail!("Token exchange failed: {body}");
    }

    let exchange_data: ExchangeResponse = exchange_resp.json().await?;

    let api_key = exchange_data
        .data
        .api_key
        .context("Server did not return an API key. Is the API Key feature enabled?")?;

    // 10. 保存
    Credentials::new(api_key.clone(), api_url.to_string()).save()?;

    // 組織が返ってきた場合、最初の組織をデフォルトに設定
    if let Some(orgs) = &exchange_data.data.organizations
        && !orgs.is_empty()
    {
        let mut settings = Settings::load()?;
        settings.set_current_organization_id(orgs[0].id.clone())?;
    }

    println!();
    println!("Login successful!");
    let masked = if api_key.len() >= 10 {
        let prefix = &api_key[..6];
        let suffix = &api_key[api_key.len() - 4..];
        format!("{prefix}...{suffix}")
    } else {
        "[saved]".to_string()
    };
    println!("  API Key: {masked}");
    println!("  API URL: {api_url}");

    if let Some(orgs) = &exchange_data.data.organizations
        && !orgs.is_empty()
    {
        println!("  Organization: {} ({})", orgs[0].name, orgs[0].id);
        if orgs.len() > 1 {
            println!();
            println!(
                "  You belong to {} organizations. Switch with: addness org switch <id>",
                orgs.len()
            );
        }
    }

    Ok(())
}

async fn wait_for_callback(listener: tokio::net::TcpListener) -> Result<(String, String)> {
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel::<(String, String)>();
    let tx = std::sync::Mutex::new(Some(tx));

    let (stream, _) = listener
        .accept()
        .await
        .context("Failed to accept connection")?;
    let io = hyper_util::rt::TokioIo::new(stream);

    let service = service_fn(move |req: Request<Incoming>| {
        let tx = tx.lock().unwrap().take();
        async move {
            if req.uri().path() != "/callback" {
                return Ok::<_, Infallible>(
                    Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Full::new(Bytes::from("not found")))
                        .unwrap(),
                );
            }

            let query = req.uri().query().unwrap_or("");
            let params: std::collections::HashMap<String, String> =
                form_urlencoded::parse(query.as_bytes())
                    .into_owned()
                    .collect();

            let handoff_id = params.get("handoff_id").cloned().unwrap_or_default();
            let state = params.get("state").cloned().unwrap_or_default();

            if let Some(tx) = tx {
                let _ = tx.send((handoff_id, state));
            }

            let body = include_str!("login_success.html");
            Ok::<_, Infallible>(
                Response::builder()
                    .header("Content-Type", "text/html")
                    .body(Full::new(Bytes::from(body)))
                    .unwrap(),
            )
        }
    });

    http1::Builder::new()
        .serve_connection(io, service)
        .await
        .context("Failed to serve callback connection")?;

    let (handoff_id, state) = rx
        .await
        .context("Callback handler did not produce a result")?;

    if handoff_id.is_empty() {
        bail!("No handoff_id in callback");
    }

    Ok((handoff_id, state))
}
