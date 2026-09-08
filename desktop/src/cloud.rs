use keyring::Entry;
use reqwest::{Client, Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;
use tokio::sync::Mutex;
use url::Url;

type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct CloudConfig {
    pub cloud_api_url: String,
    pub auth_url: String,
    pub auth_public_key: String,
}
#[derive(Default, Deserialize, Serialize)]
struct Saved {
    config: CloudConfig,
    refresh_token: Option<String>,
}
#[derive(Default)]
pub struct CloudState(Mutex<Option<String>>);
fn entry() -> Result<Entry> {
    Entry::new("ai.skillloom.desktop", "cloud-session")
        .map_err(|e| format!("系统凭证库不可用：{e}"))
}
fn load() -> Result<Saved> {
    match entry()?.get_password() {
        Ok(raw) => serde_json::from_str(&raw).map_err(|_| "系统凭证库中的 Loom 配置损坏".into()),
        Err(keyring::Error::NoEntry) => Ok(Saved::default()),
        Err(e) => Err(format!("无法读取系统凭证库：{e}")),
    }
}
fn save(saved: &Saved) -> Result<()> {
    entry()?
        .set_password(&serde_json::to_string(saved).map_err(|e| e.to_string())?)
        .map_err(|e| format!("无法保存系统凭证库：{e}"))
}
fn base(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| "服务地址无效".to_string())?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(
            "服务地址必须为 HTTPS origin（本机开发允许 HTTP），不含路径、凭证或查询参数".into(),
        );
    }
    Ok(url)
}
fn endpoint(origin: &str, path: &str) -> Result<Url> {
    if !path.starts_with("/v1/") || path.contains('\\') || path.contains('#') {
        return Err("仅允许 /v1/ 云端 API".into());
    }
    let origin = base(origin)?;
    let url = origin.join(path).map_err(|_| "API 路径无效".to_string())?;
    if url.origin() != origin.origin() || !url.path().starts_with("/v1/") {
        return Err("API 路径越界".into());
    }
    Ok(url)
}
fn client() -> Result<Client> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}
async fn json_response(response: Response) -> Result<Value> {
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|_| format!("服务返回非 JSON 响应（HTTP {status}）"))?;
    if !status.is_success() {
        let message = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| value.get("msg").and_then(Value::as_str))
            .unwrap_or("请求失败");
        return Err(format!("云端请求失败（HTTP {status}）：{message}"));
    }
    Ok(value)
}
async fn token_request(saved: &mut Saved, path: &str, body: Value) -> Result<String> {
    let url = base(&saved.config.auth_url)?
        .join(path)
        .map_err(|e| e.to_string())?;
    let response = client()?
        .post(url)
        .header("apikey", &saved.config.auth_public_key)
        .json(&body)
        .send()
        .await
        .map_err(|_| "认证服务连接失败".to_string())?;
    let value = json_response(response).await?;
    let access = value["access_token"]
        .as_str()
        .ok_or("认证响应缺少 access_token")?
        .to_string();
    saved.refresh_token = Some(
        value["refresh_token"]
            .as_str()
            .ok_or("认证响应缺少 refresh_token")?
            .to_string(),
    );
    save(saved)?;
    Ok(access)
}
#[tauri::command]
pub fn get_cloud_config() -> Result<CloudConfig> {
    Ok(load()?.config)
}
#[tauri::command]
pub async fn save_cloud_config(state: State<'_, CloudState>, config: CloudConfig) -> Result<()> {
    base(&config.cloud_api_url)?;
    base(&config.auth_url)?;
    if config.auth_public_key.trim().is_empty() {
        return Err("请输入认证服务 public key".into());
    }
    let mut token = state.0.lock().await;
    save(&Saved {
        config,
        refresh_token: None,
    })?;
    *token = None;
    Ok(())
}
#[tauri::command]
pub async fn request_otp(email: String) -> Result<()> {
    let saved = load()?;
    let url = base(&saved.config.auth_url)?
        .join("auth/v1/otp")
        .map_err(|e| e.to_string())?;
    let response = client()?
        .post(url)
        .header("apikey", saved.config.auth_public_key)
        .json(&json!({"email":email,"create_user":true}))
        .send()
        .await
        .map_err(|_| "认证服务连接失败".to_string())?;
    json_response(response).await?;
    Ok(())
}
async fn identity(config: &CloudConfig, access: &str) -> Result<Value> {
    let url = base(&config.auth_url)?
        .join("auth/v1/user")
        .map_err(|e| e.to_string())?;
    let response = client()?
        .get(url)
        .header("apikey", &config.auth_public_key)
        .bearer_auth(access)
        .send()
        .await
        .map_err(|_| "认证服务连接失败".to_string())?;
    let user = json_response(response).await?;
    let id = user["id"].as_str().ok_or("认证服务缺少用户身份")?;
    Ok(json!({"id":id,"email":user["email"].as_str()}))
}
#[tauri::command]
pub async fn verify_otp(
    state: State<'_, CloudState>,
    email: String,
    token: String,
) -> Result<Value> {
    let mut session = state.0.lock().await;
    let mut saved = load()?;
    let access = token_request(
        &mut saved,
        "auth/v1/verify",
        json!({"email":email,"token":token,"type":"email"}),
    )
    .await?;
    let user = identity(&saved.config, &access).await?;
    *session = Some(access);
    Ok(user)
}
#[tauri::command]
pub async fn current_user(state: State<'_, CloudState>) -> Result<Option<Value>> {
    let mut session = state.0.lock().await;
    let mut saved = load()?;
    let Some(refresh) = saved.refresh_token.clone() else {
        return Ok(None);
    };
    // Verify current identity with the auth server; refresh also restores an app restart.
    let access = token_request(
        &mut saved,
        "auth/v1/token?grant_type=refresh_token",
        json!({"refresh_token":refresh}),
    )
    .await?;
    let user = identity(&saved.config, &access).await?;
    *session = Some(access);
    Ok(Some(user))
}
#[tauri::command]
pub async fn cloud_request(
    state: State<'_, CloudState>,
    method: String,
    path: String,
    body: Option<Value>,
    idempotency_key: Option<String>,
) -> Result<Value> {
    let method = match method.as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        _ => return Err("不支持的 HTTP 方法".into()),
    };
    let mut session = state.0.lock().await;
    let mut saved = load()?;
    let url = endpoint(&saved.config.cloud_api_url, &path)?;
    if session.is_none() {
        let refresh = saved.refresh_token.clone().ok_or("请先登录")?;
        *session = Some(
            token_request(
                &mut saved,
                "auth/v1/token?grant_type=refresh_token",
                json!({"refresh_token":refresh}),
            )
            .await?,
        );
    }
    let request_key = idempotency_key.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let send = |token: &str| {
        let mut request = client()?
            .request(method.clone(), url.clone())
            .bearer_auth(token);
        if method == Method::POST {
            request = request.header("Idempotency-Key", &request_key);
        }
        if let Some(body) = &body {
            request = request.json(body);
        }
        Ok::<_, String>(request)
    };
    let mut response = send(session.as_deref().ok_or("请先登录")?)?
        .send()
        .await
        .map_err(|_| "云端连接失败；写入请求可能已提交，请查询状态后重试".to_string())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        *session = None;
        let refresh = saved.refresh_token.clone().ok_or("会话失效，请重新登录")?;
        *session = Some(
            token_request(
                &mut saved,
                "auth/v1/token?grant_type=refresh_token",
                json!({"refresh_token":refresh}),
            )
            .await?,
        );
        response = send(session.as_deref().ok_or("请先登录")?)?
            .send()
            .await
            .map_err(|_| "云端连接失败".to_string())?;
    }
    json_response(response).await
}
#[tauri::command]
pub async fn logout(state: State<'_, CloudState>) -> Result<()> {
    let mut session = state.0.lock().await;
    let mut saved = load()?;
    saved.refresh_token = None;
    save(&saved)?;
    *session = None;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_credential_and_remote_http_origins() {
        for s in [
            "http://example.com",
            "https://user:pass@example.com",
            "https://example.com/path",
        ] {
            assert!(base(s).is_err());
        }
    }
    #[test]
    fn confines_api_paths() {
        for p in [
            "//evil.com/v1/x",
            "/v1/../../secret",
            "/v1/\\evil",
            "/auth/v1/token",
        ] {
            assert!(endpoint("https://example.com", p).is_err());
        }
        assert!(endpoint("https://example.com", "/v1/me/teams").is_ok());
    }
}
