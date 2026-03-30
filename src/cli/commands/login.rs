use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

use crate::api::ApiClient;
use crate::config::{save_credentials, save_settings, Credentials};

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
        .unwrap()
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
    let browser_url = format!(
        "{}/api/v1/public/desktop/auth/start-sessions/redeem?start_token={}",
        api_url, start_token
    );

    // ただし、redeemはPOSTなので、ブラウザに直接開かせる方法を変える必要がある
    // Desktop Authの設計を見ると、ブラウザはフロントエンド経由でredeemする
    // CLIはstart_tokenをフロントエンドのURLパラメータとして渡す
    // フロントエンドのsign-inページがstart_tokenを受け取ってredeemする

    // 実際のDesktop Authフロー:
    // CLIがlocalhostでHTTPサーバーを起動
    // ブラウザがClerkでログイン → intent complete → localhostにリダイレクト

    // フロントエンドのDesktop Auth用URLにstart_tokenを渡す
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

    if let Err(_) = open::that(&browser_url) {
        println!("Could not open browser automatically.");
    }

    println!("Waiting for login...");

    // 8. localhostでコールバックを待機
    let (handoff_id, callback_state) = tokio::task::spawn_blocking(move || {
        wait_for_callback(listener)
    })
    .await??;

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
    save_credentials(&Credentials {
        token: api_key.clone(),
        api_url: api_url.to_string(),
    })?;

    // 組織が返ってきた場合、最初の組織をデフォルトに設定
    if let Some(orgs) = &exchange_data.data.organizations {
        if !orgs.is_empty() {
            let mut settings = crate::config::load_settings()?;
            settings.current_organization_id = Some(orgs[0].id.clone());
            save_settings(&settings)?;
        }
    }

    println!();
    println!("Login successful!");
    println!("  API Key: {}...{}", &api_key[..6], &api_key[api_key.len() - 4..]);
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
    let (mut stream, _) = listener.accept().context("Failed to accept connection")?;

    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .context("Failed to read request")?;

    // Parse: GET /callback?handoff_id=xxx&state=yyy HTTP/1.1
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("");

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
    let body = "<html><body><h1>Login successful!</h1><p>You can close this tab.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;

    if handoff_id.is_empty() {
        bail!("No handoff_id in callback");
    }

    Ok((handoff_id, state))
}
