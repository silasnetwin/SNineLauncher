use crate::auth::AuthService;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

const API_BASE: &str = "https://api.s9lab.site/api/v1";
const SESSION_JOIN_URL: &str = "https://sessionserver.mojang.com/session/minecraft/join";
const SESSION_HEADER: &str = "X-SNine-Session";
const CLIENT_VERSION: &str = "1.1.5";

#[derive(Clone)]
struct BackendSession {
    uuid: String,
    token: String,
}

impl Drop for BackendSession {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

#[derive(Clone)]
pub struct SocialService {
    client: Client,
    session: Arc<Mutex<Option<BackendSession>>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Friend {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub custom_nickname: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub last_seen: i64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub unread_messages: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendRequest {
    pub uuid: String,
    pub name: String,
    #[serde(default)]
    pub created_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendsSnapshot {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub friends: Vec<Friend>,
    #[serde(default)]
    pub incoming_requests: Vec<FriendRequest>,
    #[serde(default)]
    pub outgoing_requests: Vec<FriendRequest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendMessage {
    pub message_id: i64,
    pub sender_uuid: String,
    pub receiver_uuid: String,
    pub sender_name: String,
    pub message: String,
    pub sent_at: i64,
    #[serde(default)]
    pub read: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Challenge {
    challenge_id: String,
    server_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HandshakeResult {
    session_token: String,
}

#[derive(Deserialize)]
struct ApiError {
    error: Option<String>,
    code: Option<String>,
}

impl SocialService {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(8))
                .timeout(std::time::Duration::from_secs(15))
                .user_agent(concat!("SNineLauncher/", env!("CARGO_PKG_VERSION")))
                .build()?,
            session: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn clear_session(&self) {
        *self.session.lock().await = None;
    }

    async fn clear_if_current(&self, used: &BackendSession) {
        let mut current = self.session.lock().await;
        if current
            .as_ref()
            .is_some_and(|session| session.uuid == used.uuid && session.token == used.token)
        {
            *current = None;
        }
    }

    async fn parse_authenticated<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::Response,
        session: &BackendSession,
    ) -> Result<T, String> {
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            self.clear_if_current(session).await;
        }
        parse_response(response).await
    }

    async fn backend_session(&self, auth: &AuthService) -> Result<BackendSession, String> {
        let (account, minecraft_token, _) = auth.require_valid_minecraft_session().await?;
        let minecraft_token = Zeroizing::new(minecraft_token);
        let mut session = self.session.lock().await;
        if let Some(current) = session
            .as_ref()
            .filter(|current| current.uuid == account.uuid)
        {
            return Ok(current.clone());
        }
        *session = None;

        let challenge = self
            .client
            .post(format!("{API_BASE}/handshake/challenge"))
            .json(&serde_json::json!({
                "uuid": account.uuid,
                "name": account.username,
                "clientVersion": CLIENT_VERSION,
            }))
            .send()
            .await
            .map_err(|_| "SNine service is unavailable. Try again shortly.".to_string())?;
        let challenge: Challenge = parse_response(challenge).await?;

        let join_response = self
            .client
            .post(SESSION_JOIN_URL)
            .json(&serde_json::json!({
                "accessToken": minecraft_token.as_str(),
                "selectedProfile": account.uuid.replace('-', ""),
                "serverId": challenge.server_id,
            }))
            .send()
            .await
            .map_err(|_| {
                "Minecraft authentication could not verify the SNine session.".to_string()
            })?;
        if join_response.status() != StatusCode::NO_CONTENT {
            return Err("Minecraft authentication could not verify the SNine session.".into());
        }
        // Consume any unexpected body so the pooled connection remains reusable.
        let _ = join_response.bytes().await;

        let complete = self
            .client
            .post(format!("{API_BASE}/handshake/complete"))
            .json(&serde_json::json!({
                "challengeId": challenge.challenge_id,
                "uuid": account.uuid,
                "name": account.username,
                "clientVersion": CLIENT_VERSION,
            }))
            .send()
            .await
            .map_err(|_| "SNine service is unavailable. Try again shortly.".to_string())?;
        let result: HandshakeResult = parse_response(complete).await?;
        if result.session_token.trim().is_empty() {
            return Err("SNine did not return a valid session.".into());
        }
        let value = BackendSession {
            uuid: account.uuid,
            token: result.session_token,
        };
        *session = Some(value.clone());
        Ok(value)
    }

    pub async fn friends(&self, auth: &AuthService) -> Result<FriendsSnapshot, String> {
        let session = self.backend_session(auth).await?;
        // Microsoft profile IDs are stored compactly for Minecraft session calls,
        // while the SNine API's heartbeat endpoint expects UUID.fromString format.
        let account_uuid = canonical_uuid(&session.uuid)?;
        let status = self
            .client
            .post(format!("{API_BASE}/heartbeat"))
            .header(SESSION_HEADER, &session.token)
            .json(&serde_json::json!({
                "uuid": account_uuid,
                "name": auth.state()?.account.map(|account| account.username).unwrap_or_default(),
                "playtimeSeconds": 0,
                "status": "In launcher",
            }))
            .send()
            .await
            .map_err(|_| "SNine service is unavailable. Try again shortly.".to_string())?;
        if matches!(
            status.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            let error = response_error(status).await;
            self.clear_if_current(&session).await;
            return Err(error);
        }
        if !status.status().is_success() {
            return Err(response_error(status).await);
        }
        let response = self
            .client
            .get(format!("{API_BASE}/friends"))
            .header(SESSION_HEADER, &session.token)
            .send()
            .await
            .map_err(|_| "SNine service is unavailable. Try again shortly.".to_string())?;
        self.parse_authenticated(response, &session).await
    }

    pub async fn messages(
        &self,
        auth: &AuthService,
        friend_uuid: &str,
    ) -> Result<Vec<FriendMessage>, String> {
        let session = self.backend_session(auth).await?;
        let url = format!("{API_BASE}/friends/messages/{}", valid_uuid(friend_uuid)?);
        let response = self
            .client
            .get(url)
            .header(SESSION_HEADER, &session.token)
            .send()
            .await
            .map_err(|_| "Could not load this conversation. Check your connection.".to_string())?;
        let body: MessageResponse = self.parse_authenticated(response, &session).await?;
        Ok(body.messages)
    }

    pub async fn send_message(
        &self,
        auth: &AuthService,
        friend_uuid: &str,
        message: String,
    ) -> Result<FriendMessage, String> {
        let friend_uuid = valid_uuid(friend_uuid)?;
        let message = message.trim();
        if message.is_empty() || message.chars().count() > 240 {
            return Err("Messages must contain 1–240 characters.".into());
        }
        let session = self.backend_session(auth).await?;
        let response = self
            .client
            .post(format!("{API_BASE}/friends/message"))
            .header(SESSION_HEADER, &session.token)
            .json(&serde_json::json!({ "friendUuid": friend_uuid, "message": message }))
            .send()
            .await
            .map_err(|_| "Could not send this message. Check your connection.".to_string())?;
        self.parse_authenticated(response, &session).await
    }

    pub async fn add_friend(
        &self,
        auth: &AuthService,
        target_name: String,
    ) -> Result<FriendsSnapshot, String> {
        let target_name = target_name.trim();
        if target_name.is_empty() || target_name.len() > 32 {
            return Err("Enter a valid SNine or Minecraft username.".into());
        }
        let session = self.backend_session(auth).await?;
        let response = self
            .client
            .post(format!("{API_BASE}/friends/add"))
            .header(SESSION_HEADER, &session.token)
            .json(&serde_json::json!({ "targetName": target_name }))
            .send()
            .await
            .map_err(|_| "Could not send the friend request. Check your connection.".to_string())?;
        self.parse_authenticated(response, &session).await
    }

    pub async fn respond_to_request(
        &self,
        auth: &AuthService,
        requester_uuid: &str,
        accept: bool,
    ) -> Result<FriendsSnapshot, String> {
        let session = self.backend_session(auth).await?;
        let response = self
            .client
            .post(format!("{API_BASE}/friends/respond"))
            .header(SESSION_HEADER, &session.token)
            .json(&serde_json::json!({ "requesterUuid": valid_uuid(requester_uuid)?, "accept": accept }))
            .send()
            .await
            .map_err(|_| "Could not update this friend request. Check your connection.".to_string())?;
        self.parse_authenticated(response, &session).await
    }

    pub async fn remove_friend(
        &self,
        auth: &AuthService,
        friend_uuid: &str,
    ) -> Result<FriendsSnapshot, String> {
        let session = self.backend_session(auth).await?;
        let response = self
            .client
            .post(format!("{API_BASE}/friends/remove"))
            .header(SESSION_HEADER, &session.token)
            .json(&serde_json::json!({ "friendUuid": valid_uuid(friend_uuid)? }))
            .send()
            .await
            .map_err(|_| "Could not remove this friend. Check your connection.".to_string())?;
        self.parse_authenticated(response, &session).await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageResponse {
    messages: Vec<FriendMessage>,
}

async fn parse_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, String> {
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "SNine returned an unreadable response.".to_string())
}

async fn response_error(response: reqwest::Response) -> String {
    let status = response.status();
    let body = response.json::<ApiError>().await.ok();
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "Your SNine session expired. Sign in to Minecraft again.".into()
        }
        StatusCode::NOT_FOUND => "This SNine player or conversation was not found.".into(),
        StatusCode::TOO_MANY_REQUESTS => {
            "SNine is receiving too many requests. Try again shortly.".into()
        }
        _ => match body.and_then(|body| body.code.or(body.error)) {
            Some(code) if code == "already_friends" => "You are already friends.".into(),
            Some(code) if code == "friend_request_exists" || code == "friend_request_pending" => {
                "A friend request is already pending.".into()
            }
            Some(code) if code == "not_friends" || code == "friends_only" => {
                "You can only message confirmed friends.".into()
            }
            Some(code) if code == "minecraft_session_proof_failed" || code == "minecraft_identity_mismatch" => {
                "SNine could not verify this Minecraft session. Sign in to Minecraft again and retry.".into()
            }
            Some(code) if code == "internal_error" || code == "database_error" || code == "database_temporarily_busy" => {
                format!("SNine backend error ({code}, HTTP {}). Try again shortly.", status.as_u16())
            }
            Some(code) if code.chars().all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-') => {
                format!("SNine request failed ({code}, HTTP {}).", status.as_u16())
            }
            _ => format!("SNine request failed (HTTP {}). Try again shortly.", status.as_u16()),
        },
    }
}

fn valid_uuid(value: &str) -> Result<&str, String> {
    uuid::Uuid::parse_str(value)
        .map(|_| value)
        .map_err(|_| "Invalid SNine player ID.".into())
}

fn canonical_uuid(value: &str) -> Result<String, String> {
    uuid::Uuid::parse_str(value)
        .map(|parsed| parsed.to_string())
        .map_err(|_| "Minecraft account has an invalid profile identifier.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friend_payloads_parse_with_safe_defaults() {
        let response: FriendsSnapshot = serde_json::from_str(
            r#"{"ok":true,"friends":[{"uuid":"00000000-0000-0000-0000-000000000001","name":"Example","online":true}],"incomingRequests":[],"outgoingRequests":[]}"#,
        )
        .unwrap();
        assert_eq!(response.friends.len(), 1);
        assert!(response.friends[0].online);
        assert_eq!(response.friends[0].unread_messages, 0);
    }

    #[test]
    fn player_ids_are_validated_before_routes_are_built() {
        assert!(valid_uuid("not-a-player-id").is_err());
        assert!(valid_uuid("00000000-0000-0000-0000-000000000001").is_ok());
    }

    #[test]
    fn compact_minecraft_uuid_is_canonicalized_for_snine_api() {
        assert_eq!(
            canonical_uuid("00000000000000000000000000000001").unwrap(),
            "00000000-0000-0000-0000-000000000001"
        );
        assert!(canonical_uuid("not-a-player-id").is_err());
    }
}
