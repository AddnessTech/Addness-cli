use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

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

    // PEM形式のEd25519公開鍵
    let public_key_bytes = verifying_key.as_bytes();
    // PKIX wrapping for Ed25519: 12-byte prefix + 32-byte key
    let mut pkix = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    pkix.extend_from_slice(public_key_bytes);

    let b64 = base64::engine::general_purpose::STANDARD.encode(&pkix);
    let pem = format!("-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----", b64);

    (signing_key, pem)
}

fn sign_message(signing_key: &SigningKey, message: &str) -> String {
    let signature = signing_key.sign(message.as_bytes());
    URL_SAFE_NO_PAD.encode(signature.to_bytes())
}

fn sha256_base64url(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let result = hasher.finalize();
    URL_SAFE_NO_PAD.encode(result)
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
    let installation_id = uuid::Uuid::new_v4().to_string().replace("-", "");
    let installation_id = &installation_id[..32]; // max 128 chars, alphanumeric + hyphens

    // 2. localhostサーバー起動（空きポートを自動取得）
    let listener = TcpListener::bind("127.0.0.1:0").context("Failed to bind localhost port")?;
    let port = listener.local_addr()?.port();

    // 3. PKCE用のcode_verifierとcode_challenge生成
    let code_verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let code_challenge = sha256_base64url(&code_verifier);

    // 4. State生成
    let state = uuid::Uuid::new_v4().to_string().replace("-", "");
    let state = &state[..32];

    // 5. Installation登録
    let client = reqwest::Client::new();
    let register_resp = client
        .post(format!(
            "{}/api/v1/public/desktop/auth/installations/register",
            api_url
        ))
        .json(&serde_json::json!({
            "installationId": installation_id,
            "publicKey": public_key_pem,
        }))
        .send()
        .await?;

    if !register_resp.status().is_success() {
        let body = register_resp.text().await?;
        bail!("Failed to register installation: {}", body);
    }
    let _: RegisterResponse = register_resp.json().await?;

    // 6. StartSession作成
    let ts = timestamp();
    let start_message = format!(
        "visiontodo-desktop-auth-start\ninstallation_id={}\nstate={}\nport={}\nnext_path={}\ncode_challenge={}\nauth_path={}\nreferral_code={}\ntimestamp={}",
        installation_id, state, port, "/organization/set", code_challenge, "/sign-in", "", ts
    );
    let start_signature = sign_message(&signing_key, &start_message);

    let start_resp = client
        .post(format!(
            "{}/api/v1/public/desktop/auth/start-sessions",
            api_url
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
        bail!("Failed to create start session: {}", body);
    }

    let start_data: StartSessionResponse = start_resp.json().await?;
    let start_token = start_data.data.start_token;

    // 7. ブラウザを開く
    let fe_url = frontend_url
        .map(|s| s.to_string())
        .unwrap_or_else(|| api_url.replace(":8080", ":3000").replace("api.", ""));
    let browser_url = format!(
        "{}/desktop/browser-auth?start_token={}",
        fe_url.trim_end_matches('/'), start_token
    );

    println!("Opening browser for login...");
    println!("If the browser doesn't open, visit:");
    println!("  {}", browser_url);
    println!();

    if open::that(&browser_url).is_err() {
        println!("Could not open browser automatically.");
    }

    println!("Waiting for login...");

    // 8. localhostでコールバックを待機
    let callback_result = tokio::time::timeout(
        Duration::from_secs(300),
        tokio::task::spawn_blocking(move || wait_for_callback(listener)),
    )
    .await;
    let (handoff_id, callback_state) = match callback_result {
        Ok(Ok(result)) => result?,
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => bail!("Login timed out (5 minutes). Please try again."),
    };

    if callback_state != state {
        bail!("State mismatch: expected {}, got {}", state, callback_state);
    }

    // 9. Token Exchange（source=cli でAPI Key自動発行）
    let ts = timestamp();
    let exchange_message = format!(
        "visiontodo-desktop-auth-exchange\ninstallation_id={}\nhandoff_id={}\ncode_verifier={}\ntimestamp={}",
        installation_id, handoff_id, code_verifier, ts
    );
    let exchange_signature = sign_message(&signing_key, &exchange_message);

    let exchange_resp = client
        .post(format!(
            "{}/api/v1/public/desktop/auth/token-exchange",
            api_url
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
        bail!("Token exchange failed: {}", body);
    }

    let exchange_data: ExchangeResponse = exchange_resp.json().await?;

    let api_key = exchange_data
        .data
        .api_key
        .context("Server did not return an API key. Is the API Key feature enabled?")?;

    // 10. 保存
    Credentials::new(api_key.clone(), api_url.to_string()).save()?;

    // 組織が返ってきた場合、最初の組織をデフォルトに設定
    if let Some(orgs) = &exchange_data.data.organizations {
        if !orgs.is_empty() {
            let mut settings = Settings::load()?;
            settings.set_current_organization_id(orgs[0].id.clone())?;
        }
    }

    println!();
    println!("Login successful!");
    let masked = if api_key.len() >= 10 {
        format!("{}...{}", &api_key[..6], &api_key[api_key.len() - 4..])
    } else {
        "[saved]".to_string()
    };
    println!("  API Key: {}", masked);
    println!("  API URL: {}", api_url);

    if let Some(orgs) = &exchange_data.data.organizations {
        if !orgs.is_empty() {
            println!("  Organization: {} ({})", orgs[0].name, orgs[0].id);
        }
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

fn wait_for_callback(listener: TcpListener) -> Result<(String, String)> {
    loop {
        let (mut stream, _) = listener.accept().context("Failed to accept connection")?;

        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        reader
            .read_line(&mut request_line)
            .context("Failed to read request")?;

        // Parse: GET /callback?handoff_id=xxx&state=yyy HTTP/1.1
        let path = request_line.split_whitespace().nth(1).unwrap_or("");

        // Ignore requests that are not the callback (e.g. favicon, preflight)
        if !path.starts_with("/callback") {
            let body = "not found";
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            continue;
        }

        let query = path.split('?').nth(1).unwrap_or("");
        let mut handoff_id = String::new();
        let mut state = String::new();

        for param in query.split('&') {
            if let Some((key, value)) = param.split_once('=') {
                match key {
                    "handoff_id" => handoff_id = value.to_string(),
                    "state" => state = value.to_string(),
                    _ => {}
                }
            }
        }

        // レスポンスを返す
        let body =
            "<html><body><h1>Login successful!</h1><p>You can close this tab.</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes())?;

        if handoff_id.is_empty() {
            bail!("No handoff_id in callback");
        }

        return Ok((handoff_id, state));
    }
}
