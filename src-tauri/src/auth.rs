use crate::credentials::AuthCredentials;
use crate::{DeviceCodeResponse, BASE_URL};
use oauth2::basic::BasicClient;
use oauth2::{
    ClientId, DeviceAuthorizationUrl, RefreshToken, Scope, StandardDeviceAuthorizationResponse,
    TokenResponse, TokenUrl,
};
use serde::Deserialize;
use std::sync::OnceLock;
use tokio::sync::Mutex;

static REFRESH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAuthConfig {
    pub issuer: String,
    pub client_id: String,
    pub resource: String,
    pub scope: String,
}

#[derive(Debug, Deserialize)]
struct Discovery {
    device_authorization_endpoint: String,
    token_endpoint: String,
}

pub async fn config(client: &reqwest::Client) -> Result<DesktopAuthConfig, String> {
    config_at(client, BASE_URL).await
}

async fn config_at(client: &reqwest::Client, base_url: &str) -> Result<DesktopAuthConfig, String> {
    client
        .get(format!("{base_url}/api/auth/desktop-config"))
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())
}

async fn discovery(
    client: &reqwest::Client,
    config: &DesktopAuthConfig,
) -> Result<Discovery, String> {
    client
        .get(format!(
            "{}/.well-known/openid-configuration",
            config.issuer.trim_end_matches('/')
        ))
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())
}

fn oauth_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())
}

fn device_client(
    config: &DesktopAuthConfig,
    metadata: &Discovery,
) -> Result<
    BasicClient<
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    >,
    String,
> {
    Ok(BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_token_uri(TokenUrl::new(metadata.token_endpoint.clone()).map_err(|e| e.to_string())?)
        .set_device_authorization_url(
            DeviceAuthorizationUrl::new(metadata.device_authorization_endpoint.clone())
                .map_err(|e| e.to_string())?,
        ))
}

fn token_client(
    config: &DesktopAuthConfig,
    metadata: &Discovery,
) -> Result<
    BasicClient<
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    >,
    String,
> {
    Ok(BasicClient::new(ClientId::new(config.client_id.clone()))
        .set_token_uri(TokenUrl::new(metadata.token_endpoint.clone()).map_err(|e| e.to_string())?))
}

fn scopes(value: &str) -> Vec<Scope> {
    value
        .split_whitespace()
        .map(|scope| Scope::new(scope.to_string()))
        .collect()
}

pub async fn start_device_authorization(
    client: &reqwest::Client,
) -> Result<DeviceCodeResponse, String> {
    start_device_authorization_at(client, BASE_URL).await
}

async fn start_device_authorization_at(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<DeviceCodeResponse, String> {
    let config = config_at(client, base_url).await?;
    let metadata = discovery(client, &config).await?;
    let oauth_client = device_client(&config, &metadata)?;
    let http_client = oauth_http_client()?;
    let response: StandardDeviceAuthorizationResponse = oauth_client
        .exchange_device_code()
        .add_scopes(scopes(&config.scope))
        .add_extra_param("resource", config.resource)
        .request_async(&http_client)
        .await
        .map_err(|error| format!("Realmroot device authorization failed: {error}"))?;

    Ok(DeviceCodeResponse {
        device_code: response.device_code().secret().to_string(),
        user_code: response.user_code().secret().to_string(),
        verification_uri: response.verification_uri().to_string(),
        verification_uri_complete: response
            .verification_uri_complete()
            .map(|value| value.secret().to_string()),
        expires_in: response.expires_in().as_secs(),
        interval: response.interval().as_secs(),
    })
}

fn standard_device_response(
    response: &DeviceCodeResponse,
) -> Result<StandardDeviceAuthorizationResponse, String> {
    serde_json::from_value(serde_json::json!({
        "device_code": response.device_code,
        "user_code": response.user_code,
        "verification_uri": response.verification_uri,
        "verification_uri_complete": response.verification_uri_complete,
        "expires_in": response.expires_in,
        "interval": response.interval,
    }))
    .map_err(|error| error.to_string())
}

pub async fn complete_device_authorization(
    client: &reqwest::Client,
    response: &DeviceCodeResponse,
) -> Result<(), String> {
    exchange_device_credentials_at(client, response, BASE_URL)
        .await?
        .save()
}

async fn exchange_device_credentials_at(
    client: &reqwest::Client,
    response: &DeviceCodeResponse,
    base_url: &str,
) -> Result<AuthCredentials, String> {
    let config = config_at(client, base_url).await?;
    let metadata = discovery(client, &config).await?;
    let oauth_client = device_client(&config, &metadata)?;
    let http_client = oauth_http_client()?;
    let details = standard_device_response(response)?;
    let token = oauth_client
        .exchange_device_access_token(&details)
        .add_extra_param("resource", config.resource)
        .request_async(&http_client, tokio::time::sleep, Some(details.expires_in()))
        .await
        .map_err(|error| format!("Realmroot device authorization failed: {error}"))?;
    let expires_in = token
        .expires_in()
        .ok_or_else(|| "Realmroot did not return token expiry".to_string())?;
    Ok(AuthCredentials {
        access_token: token.access_token().secret().to_string(),
        refresh_token: token
            .refresh_token()
            .map(|value| value.secret().to_string())
            .unwrap_or_default(),
        expires_at: chrono::Utc::now().timestamp() + expires_in.as_secs() as i64,
    })
}

async fn refresh_credentials_at(
    client: &reqwest::Client,
    credentials: AuthCredentials,
    base_url: &str,
) -> Result<AuthCredentials, String> {
    let config = config_at(client, base_url).await?;
    let metadata = discovery(client, &config).await?;
    let oauth_client = token_client(&config, &metadata)?;
    let http_client = oauth_http_client()?;
    let token = oauth_client
        .exchange_refresh_token(&RefreshToken::new(credentials.refresh_token.clone()))
        .add_extra_param("resource", config.resource)
        .request_async(&http_client)
        .await
        .map_err(|error| format!("Realmroot refresh failed: {error}"))?;
    let expires_in = token
        .expires_in()
        .ok_or_else(|| "Realmroot did not return token expiry".to_string())?;
    Ok(AuthCredentials {
        access_token: token.access_token().secret().to_string(),
        refresh_token: token
            .refresh_token()
            .map(|value| value.secret().to_string())
            .unwrap_or(credentials.refresh_token),
        expires_at: chrono::Utc::now().timestamp() + expires_in.as_secs() as i64,
    })
}

pub async fn access_token(client: &reqwest::Client) -> Result<String, String> {
    let credentials = AuthCredentials::load();
    if !credentials.signed_in() {
        return Err("Authentication not configured".into());
    }
    if credentials.expires_at > chrono::Utc::now().timestamp() + 60 {
        return Ok(credentials.access_token);
    }

    let _guard = REFRESH_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let credentials = AuthCredentials::load();
    if credentials.expires_at > chrono::Utc::now().timestamp() + 60 {
        return Ok(credentials.access_token);
    }
    if credentials.refresh_token.is_empty() {
        return Err("Realmroot session expired; sign in again".into());
    }

    let updated = refresh_credentials_at(client, credentials, BASE_URL).await?;
    updated.save()?;
    Ok(updated.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    async fn mock_config_and_discovery(server: &mut Server, expected_calls: usize) {
        let issuer = format!("{}/issuer", server.url());
        server
            .mock("GET", "/api/auth/desktop-config")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "issuer": issuer,
                    "clientId": "desktop-client",
                    "resource": "https://wakatoken.com/api/v1",
                    "scope": "openid offline_access heartbeats:write"
                })
                .to_string(),
            )
            .expect(expected_calls)
            .create_async()
            .await;
        server
            .mock("GET", "/issuer/.well-known/openid-configuration")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "device_authorization_endpoint": format!("{}/issuer/device", server.url()),
                    "token_endpoint": format!("{}/issuer/token", server.url())
                })
                .to_string(),
            )
            .expect(expected_calls)
            .create_async()
            .await;
    }

    #[test]
    fn reconstructs_device_authorization_response() {
        let response = DeviceCodeResponse {
            device_code: "device".into(),
            user_code: "USER".into(),
            verification_uri: "https://id.example/device".into(),
            verification_uri_complete: Some("https://id.example/device?code=USER".into()),
            expires_in: 600,
            interval: 5,
        };
        let standard = standard_device_response(&response).unwrap();
        assert_eq!(standard.device_code().secret(), "device");
        assert_eq!(standard.interval(), std::time::Duration::from_secs(5));
    }

    #[tokio::test]
    async fn uses_discovery_and_resource_for_device_authorization() {
        let mut server = Server::new_async().await;
        mock_config_and_discovery(&mut server, 1).await;
        let device = server
            .mock("POST", "/issuer/device")
            .match_body(Matcher::AllOf(vec![
                Matcher::UrlEncoded("client_id".into(), "desktop-client".into()),
                Matcher::UrlEncoded("resource".into(), "https://wakatoken.com/api/v1".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "device_code": "device-code",
                    "user_code": "WAKA-TOKEN",
                    "verification_uri": "https://id.example/device",
                    "verification_uri_complete": "https://id.example/device?user_code=WAKA-TOKEN",
                    "expires_in": 600,
                    "interval": 1
                })
                .to_string(),
            )
            .create_async()
            .await;

        let response = start_device_authorization_at(&reqwest::Client::new(), &server.url())
            .await
            .unwrap();

        assert_eq!(response.device_code, "device-code");
        assert_eq!(response.user_code, "WAKA-TOKEN");
        device.assert_async().await;
    }

    #[tokio::test]
    async fn exchanges_device_code_and_rotates_refresh_token() {
        let mut server = Server::new_async().await;
        mock_config_and_discovery(&mut server, 2).await;
        let device_exchange = server
            .mock("POST", "/issuer/token")
            .match_body(Matcher::UrlEncoded(
                "grant_type".into(),
                "urn:ietf:params:oauth:grant-type:device_code".into(),
            ))
            .match_body(Matcher::UrlEncoded(
                "device_code".into(),
                "device-code".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "access_token": "device-access",
                    "refresh_token": "device-refresh",
                    "token_type": "Bearer",
                    "expires_in": 3600
                })
                .to_string(),
            )
            .create_async()
            .await;
        let refresh_exchange = server
            .mock("POST", "/issuer/token")
            .match_body(Matcher::UrlEncoded(
                "grant_type".into(),
                "refresh_token".into(),
            ))
            .match_body(Matcher::UrlEncoded(
                "refresh_token".into(),
                "device-refresh".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "access_token": "rotated-access",
                    "refresh_token": "rotated-refresh",
                    "token_type": "Bearer",
                    "expires_in": 3600
                })
                .to_string(),
            )
            .create_async()
            .await;
        let device_response = DeviceCodeResponse {
            device_code: "device-code".into(),
            user_code: "WAKA-TOKEN".into(),
            verification_uri: "https://id.example/device".into(),
            verification_uri_complete: None,
            expires_in: 600,
            interval: 1,
        };

        let credentials = exchange_device_credentials_at(
            &reqwest::Client::new(),
            &device_response,
            &server.url(),
        )
        .await
        .unwrap();
        assert_eq!(credentials.access_token, "device-access");
        assert_eq!(credentials.refresh_token, "device-refresh");

        let rotated = refresh_credentials_at(&reqwest::Client::new(), credentials, &server.url())
            .await
            .unwrap();
        assert_eq!(rotated.access_token, "rotated-access");
        assert_eq!(rotated.refresh_token, "rotated-refresh");
        device_exchange.assert_async().await;
        refresh_exchange.assert_async().await;
    }
}
