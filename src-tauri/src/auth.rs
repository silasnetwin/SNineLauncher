mod credentials;

use crate::database::{Database, SafeAccount};
use credentials::{CredentialStore, WindowsCredentialStore};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::time::{sleep, Instant};
use tokio_util::sync::CancellationToken;

pub(crate) const CLIENT_ID: &str = "e686aebd-d575-4472-b163-b0c54f388f43";
const SCOPE: &str = "XboxLive.signin offline_access";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const OFFLINE_AUTH_REQUIRED: &str =
    "OfflineAuthenticationRequired: reconnect to refresh your Minecraft account before launching.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    pub status: String,
    pub account: Option<SafeAccount>,
    pub accounts: Vec<SafeAccount>,
    pub persistence_warning: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLoginPrompt {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_at_unix: i64,
    pub interval_seconds: u64,
}

#[derive(Deserialize)]
struct DeviceCodeSecret {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_poll_interval")]
    interval: u64,
}
fn default_poll_interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct MicrosoftToken {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}
#[derive(Deserialize)]
struct MicrosoftError {
    error: String,
}
#[derive(Deserialize)]
struct XboxResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    claims: XboxClaims,
}
#[derive(Deserialize)]
struct XboxClaims {
    xui: Vec<XboxClaim>,
}
#[derive(Deserialize)]
struct XboxClaim {
    uhs: String,
    #[serde(default)]
    xid: Option<String>,
}
#[derive(Deserialize)]
struct MinecraftToken {
    access_token: String,
    expires_in: i64,
}
#[derive(Deserialize)]
struct Entitlements {
    #[serde(default)]
    items: Vec<Entitlement>,
}
#[derive(Deserialize)]
struct Entitlement {
    name: String,
}
#[derive(Deserialize)]
struct MinecraftProfile {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<MinecraftSkin>,
    #[serde(default)]
    capes: Vec<MinecraftCape>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftSkin {
    pub id: String,
    pub state: String,
    pub url: String,
    pub variant: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftCape {
    pub id: String,
    pub state: String,
    pub url: String,
    pub alias: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountCosmetics {
    pub username: String,
    pub skins: Vec<MinecraftSkin>,
    pub capes: Vec<MinecraftCape>,
}

#[derive(Clone)]
struct LiveSession {
    microsoft_refresh_token: String,
    minecraft_access_token: String,
    minecraft_expires_at: i64,
    xuid: Option<String>,
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.microsoft_refresh_token.zeroize();
        self.minecraft_access_token.zeroize();
        if let Some(xuid) = &mut self.xuid {
            xuid.zeroize();
        }
    }
}

#[derive(Clone)]
pub struct AuthService {
    client: Client,
    database: Arc<Database>,
    credentials: Arc<dyn CredentialStore>,
    memory_session: Arc<Mutex<Option<(SafeAccount, LiveSession)>>>,
    cancel: Arc<Mutex<Option<CancellationToken>>>,
}

impl AuthService {
    pub fn new(database: Arc<Database>, auth_dir: PathBuf) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(50))
                .user_agent(concat!("SNineLauncher/", env!("CARGO_PKG_VERSION")))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            database,
            credentials: Arc::new(WindowsCredentialStore::new(&auth_dir)),
            memory_session: Arc::new(Mutex::new(None)),
            cancel: Arc::new(Mutex::new(None)),
        })
    }

    pub fn state(&self) -> Result<AuthState, String> {
        let account = self
            .database
            .safe_account()
            .map_err(|_| "Could not read account information".to_string())?;
        Ok(AuthState {
            status: if account.is_some() {
                "loggedIn"
            } else {
                "loggedOut"
            }
            .into(),
            account,
            accounts: self
                .database
                .safe_accounts()
                .map_err(|_| "Could not read saved accounts".to_string())?,
            persistence_warning: None,
        })
    }

    pub async fn account_cosmetics(&self) -> Result<AccountCosmetics, String> {
        let (_, token, _) = self.require_valid_minecraft_session().await?;
        let response = self
            .client
            .get(MC_PROFILE_URL)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| "Could not retrieve your Minecraft cosmetics.".to_string())?;
        let mut profile: MinecraftProfile = parse_ok(response)
            .await
            .map_err(|_| "Could not retrieve your Minecraft cosmetics.".to_string())?;
        for skin in &mut profile.skins {
            skin.url = secure_texture_url(&skin.url).unwrap_or_default();
        }
        for cape in &mut profile.capes {
            cape.url = secure_texture_url(&cape.url).unwrap_or_default();
        }
        profile.skins.retain(|skin| !skin.url.is_empty());
        profile.capes.retain(|cape| !cape.url.is_empty());
        Ok(AccountCosmetics {
            username: profile.name,
            skins: profile.skins,
            capes: profile.capes,
        })
    }

    pub async fn skin_texture_for_uuid(&self, uuid: &str) -> Option<String> {
        let uuid = normalize_uuid(uuid).ok()?;
        let response = self
            .client
            .get(format!(
                "https://sessionserver.mojang.com/session/minecraft/profile/{uuid}?unsigned=false"
            ))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let profile = response.json::<Value>().await.ok()?;
        let encoded = profile
            .get("properties")?
            .as_array()?
            .iter()
            .find(|property| property.get("name").and_then(Value::as_str) == Some("textures"))?
            .get("value")?
            .as_str()?;
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()?;
        let textures: Value = serde_json::from_slice(&decoded).ok()?;
        let texture_url = textures.pointer("/textures/SKIN/url")?.as_str()?;
        let texture_url = secure_texture_url(texture_url)?;
        let parsed = reqwest::Url::parse(&texture_url).ok()?;
        let texture_hash = parsed.path().strip_prefix("/texture/")?;
        if texture_hash.len() < 32
            || !texture_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return None;
        }
        Some(texture_url)
    }

    pub async fn activate_skin(&self, skin_id: &str) -> Result<(), String> {
        let (_, token, _) = self.require_valid_minecraft_session().await?;
        let response = self
            .client
            .get(MC_PROFILE_URL)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| "Could not retrieve your Minecraft skins.".to_string())?;
        let profile: MinecraftProfile = parse_ok(response)
            .await
            .map_err(|_| "Could not retrieve your Minecraft skins.".to_string())?;
        let skin = profile
            .skins
            .into_iter()
            .find(|skin| skin.id == skin_id && secure_texture_url(&skin.url).is_some())
            .ok_or_else(|| "That skin is not available on this Minecraft account.".to_string())?;
        let response = self
            .client
            .post(format!("{MC_PROFILE_URL}/skins"))
            .bearer_auth(token)
            .json(&json!({"variant": skin.variant.to_ascii_lowercase(), "url": secure_texture_url(&skin.url).ok_or("Skin texture URL is invalid")?}))
            .send()
            .await
            .map_err(|_| "Could not change your Minecraft skin.".to_string())?;
        parse_ok::<Value>(response)
            .await
            .map(|_| ())
            .map_err(|_| "Minecraft could not change this skin.".to_string())
    }

    pub async fn upload_skin(&self, path: PathBuf, variant: &str) -> Result<(), String> {
        let variant = match variant.to_ascii_lowercase().as_str() {
            "slim" => "slim",
            "classic" => "classic",
            _ => return Err("Choose the Slim or Classic player model.".into()),
        };
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|_| "Could not read the selected skin file.".to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("Choose a regular PNG skin file.".into());
        }
        if metadata.len() == 0 || metadata.len() > 1024 * 1024 {
            return Err("Skin PNG files must be smaller than 1 MiB.".into());
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| "Could not read the selected skin file.".to_string())?;
        let dimensions = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|_| "The selected skin is not a valid PNG image.".to_string())?
            .into_dimensions()
            .map_err(|_| "The selected skin is not a valid PNG image.".to_string())?;
        if dimensions != (64, 64) && dimensions != (64, 32) {
            return Err("Minecraft skins must be 64×64 or legacy 64×32 pixels.".into());
        }
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err("Choose a PNG skin file.".into());
        }
        let (_, token, _) = self.require_valid_minecraft_session().await?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name("skin.png")
            .mime_str("image/png")
            .map_err(|_| "Could not prepare the skin upload.".to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("variant", variant.to_string())
            .part("file", part);
        let response = self
            .client
            .post(MC_PROFILE_URL.to_owned() + "/skins")
            .bearer_auth(token)
            .multipart(form)
            .send()
            .await
            .map_err(|_| {
                "Could not connect to Minecraft services to upload your skin.".to_string()
            })?;
        parse_ok::<Value>(response).await.map(|_| ()).map_err(|_| {
            "Minecraft could not upload this skin. Check the PNG and try again.".to_string()
        })
    }

    pub async fn activate_cape(&self, cape_id: &str) -> Result<(), String> {
        let (_, token, _) = self.require_valid_minecraft_session().await?;
        let response = self
            .client
            .get(MC_PROFILE_URL)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| "Could not retrieve your Minecraft capes.".to_string())?;
        let profile: MinecraftProfile = parse_ok(response)
            .await
            .map_err(|_| "Could not retrieve your Minecraft capes.".to_string())?;
        if !profile
            .capes
            .iter()
            .any(|cape| cape.id == cape_id && secure_texture_url(&cape.url).is_some())
        {
            return Err("That cape is not available on this Minecraft account.".into());
        }
        let response = self
            .client
            .put(format!("{MC_PROFILE_URL}/capes/active"))
            .bearer_auth(token)
            .json(&json!({"capeId": cape_id}))
            .send()
            .await
            .map_err(|_| "Could not change your Minecraft cape.".to_string())?;
        parse_ok::<Value>(response)
            .await
            .map(|_| ())
            .map_err(|_| "Minecraft could not change this cape.".to_string())
    }

    pub async fn hide_cape(&self) -> Result<(), String> {
        let (_, token, _) = self.require_valid_minecraft_session().await?;
        let response = self
            .client
            .delete(format!("{MC_PROFILE_URL}/capes/active"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| "Could not hide your Minecraft cape.".to_string())?;
        parse_ok::<Value>(response)
            .await
            .map(|_| ())
            .map_err(|_| "Minecraft could not hide this cape.".to_string())
    }

    pub async fn begin_sign_in(
        self: &Arc<Self>,
        app: AppHandle,
    ) -> Result<DeviceLoginPrompt, String> {
        if self
            .cancel
            .lock()
            .map_err(|_| "Authentication is unavailable".to_string())?
            .is_some()
        {
            return Err("A sign-in is already in progress".into());
        }
        let response = self
            .client
            .post(DEVICE_CODE_URL)
            .form(&[("client_id", CLIENT_ID), ("scope", SCOPE)])
            .send()
            .await
            .map_err(|_| "Could not connect to Microsoft sign-in".to_string())?;
        let secret: DeviceCodeSecret = parse_ok(response)
            .await
            .map_err(|_| "Microsoft sign-in could not be started".to_string())?;
        if secret.device_code.is_empty()
            || secret.user_code.is_empty()
            || !valid_verification_url(&secret.verification_uri)
        {
            return Err("Microsoft returned an invalid sign-in request".into());
        }
        let expires_at_unix =
            now_unix().saturating_add(secret.expires_in.min(i64::MAX as u64) as i64);
        let prompt = DeviceLoginPrompt {
            user_code: secret.user_code.clone(),
            verification_uri: secret.verification_uri.clone(),
            expires_at_unix,
            interval_seconds: secret.interval.max(5),
        };
        let cancellation = CancellationToken::new();
        *self
            .cancel
            .lock()
            .map_err(|_| "Authentication is unavailable".to_string())? = Some(cancellation.clone());
        emit_event(&app, "auth-login-started", json!({}));
        emit_state(
            &app,
            "authorizing",
            self.database.safe_account().ok().flatten(),
        );
        let _ = opener::open(&secret.verification_uri);
        let service = Arc::clone(self);
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err("Microsoft sign-in was cancelled.".into()),
                result = service.complete_login(&secret, cancellation.clone()) => result,
            };
            if cancellation.is_cancelled() {
                emit_event(
                    &app,
                    "auth-login-failed",
                    json!({"message":"Microsoft sign-in was cancelled."}),
                );
                let existing = service.database.safe_account().ok().flatten();
                emit_state(
                    &app,
                    if existing.is_some() {
                        "loggedIn"
                    } else {
                        "loggedOut"
                    },
                    existing,
                );
            } else {
                match result {
                    Ok(account) => {
                        let safe = account.safe();
                        if let Ok(mut live) = service.memory_session.lock() {
                            *live = Some((safe.clone(), account.session.clone()));
                        }
                        let credential_result = service
                            .credentials
                            .save(&safe.uuid, &account.session.microsoft_refresh_token);
                        let metadata_result =
                            service
                                .database
                                .save_account(&safe.uuid, &safe.username, now_unix());
                        let warning = credential_result.err().or_else(|| metadata_result.err().map(|_| "Signed in, but the account could not be saved locally. You may need to sign in again after restart.".to_string()));
                        emit_event(
                            &app,
                            "auth-login-completed",
                            json!({"account":safe,"persistenceWarning":warning}),
                        );
                        emit_state_warning(&app, "loggedIn", Some(safe), warning);
                    }
                    Err(error) => {
                        emit_event(&app, "auth-login-failed", json!({"message":error}));
                        let existing = service.database.safe_account().ok().flatten();
                        emit_state(
                            &app,
                            if existing.is_some() {
                                "loggedIn"
                            } else {
                                "loggedOut"
                            },
                            existing,
                        );
                    }
                }
            }
            if let Ok(mut pending) = service.cancel.lock() {
                *pending = None;
            }
        });
        Ok(prompt)
    }

    pub fn cancel_sign_in(&self) -> Result<(), String> {
        let pending = self
            .cancel
            .lock()
            .map_err(|_| "Authentication is unavailable".to_string())?
            .take();
        if let Some(token) = pending {
            token.cancel();
        }
        Ok(())
    }

    pub async fn restore(&self, app: &AppHandle) -> Result<AuthState, String> {
        let Some(account) = self
            .database
            .safe_account()
            .map_err(|_| "Could not read saved account".to_string())?
        else {
            return Ok(AuthState {
                status: "loggedOut".into(),
                account: None,
                accounts: self.database.safe_accounts().unwrap_or_default(),
                persistence_warning: None,
            });
        };
        let Some(mut refresh_token) = self.credentials.load(&account.uuid)? else {
            return Ok(AuthState {
                status: "reloginRequired".into(),
                account: Some(account),
                accounts: self.database.safe_accounts().unwrap_or_default(),
                persistence_warning: Some(
                    "The saved sign-in could not be found. Please sign in again.".into(),
                ),
            });
        };
        emit_state(app, "refreshing", Some(account.clone()));
        let result = self.exchange_refresh(&refresh_token).await;
        zeroize::Zeroize::zeroize(&mut refresh_token);
        let refreshed = result?;
        let safe = refreshed.safe();
        if let Ok(mut live) = self.memory_session.lock() {
            *live = Some((safe.clone(), refreshed.session.clone()));
        }
        let warning = self.credentials.save(&safe.uuid, &refreshed.session.microsoft_refresh_token).err()
            .or_else(|| self.database.save_account(&safe.uuid, &safe.username, refreshed.last_authenticated_at).err().map(|_| "Signed in, but the account could not be saved locally. You may need to sign in again after restart.".to_string()));
        let state = AuthState {
            status: "loggedIn".into(),
            account: Some(safe),
            accounts: self.database.safe_accounts().unwrap_or_default(),
            persistence_warning: warning.clone(),
        };
        emit_state_warning(app, "loggedIn", state.account.clone(), warning);
        Ok(state)
    }

    pub async fn sign_out(&self, app: &AppHandle) -> Result<AuthState, String> {
        let state = self.sign_out_local()?;
        if let Some(next_uuid) = state.accounts.first().map(|next| next.uuid.clone()) {
            return self.select_account(app, &next_uuid).await;
        }
        emit_event(app, "auth-logged-out", json!({}));
        emit_state(app, "loggedOut", None);
        Ok(state)
    }

    fn sign_out_local(&self) -> Result<AuthState, String> {
        self.cancel_sign_in()?;
        let stored_account = self
            .database
            .safe_account()
            .map_err(|_| "Could not read saved account".to_string())?;
        let memory_account = self
            .memory_session
            .lock()
            .map_err(|_| "Authentication is unavailable".to_string())?
            .as_ref()
            .map(|(account, _)| account.clone());
        if let Some(account) = stored_account.as_ref() {
            self.credentials.delete(&account.uuid)?;
        }
        if let Some(account) = memory_account.as_ref().filter(|account| {
            stored_account
                .as_ref()
                .is_none_or(|stored| stored.uuid != account.uuid)
        }) {
            self.credentials.delete(&account.uuid)?;
        }
        if let Ok(mut live) = self.memory_session.lock() {
            *live = None;
        }
        if let Some(account) = stored_account.as_ref() {
            self.database
                .remove_saved_account(&account.uuid)
                .map_err(|_| "Could not clear saved account".to_string())?;
        } else {
            self.database
                .clear_account()
                .map_err(|_| "Could not clear saved account".to_string())?;
        }
        let state = AuthState {
            status: "loggedOut".into(),
            account: None,
            accounts: self
                .database
                .safe_accounts()
                .map_err(|_| "Could not read saved accounts".to_string())?,
            persistence_warning: None,
        };
        Ok(state)
    }

    pub async fn select_account(&self, app: &AppHandle, uuid: &str) -> Result<AuthState, String> {
        let account = self
            .database
            .safe_accounts()
            .map_err(|_| "Could not read saved accounts".to_string())?
            .into_iter()
            .find(|account| account.uuid == uuid)
            .ok_or_else(|| "That Minecraft account is not saved in this launcher.".to_string())?;
        let mut refresh_token = self
            .credentials
            .load(&account.uuid)?
            .ok_or_else(|| "This account needs Microsoft sign-in again.".to_string())?;
        emit_state(app, "refreshing", Some(account.clone()));
        let result = self.exchange_refresh(&refresh_token).await;
        zeroize::Zeroize::zeroize(&mut refresh_token);
        let verified = match result {
            Ok(verified) => verified,
            Err(error) => {
                let current = self.database.safe_account().ok().flatten();
                emit_state(
                    app,
                    if current.is_some() {
                        "loggedIn"
                    } else {
                        "loggedOut"
                    },
                    current,
                );
                return Err(error);
            }
        };
        let safe = verified.safe();
        self.database
            .select_account(uuid)
            .map_err(|_| "Could not select this Minecraft account.".to_string())?
            .ok_or_else(|| "That Minecraft account is no longer saved.".to_string())?;
        self.database
            .save_account(&safe.uuid, &safe.username, safe.last_authenticated_at)
            .map_err(|_| "Could not save the selected Minecraft account.".to_string())?;
        self.credentials
            .save(&safe.uuid, &verified.session.microsoft_refresh_token)?;
        if let Ok(mut live) = self.memory_session.lock() {
            *live = Some((safe.clone(), verified.session));
        }
        let state = self.state()?;
        emit_state_warning(
            app,
            "loggedIn",
            state.account.clone(),
            state.persistence_warning.clone(),
        );
        Ok(state)
    }

    /// Future launch code obtains an already verified and refreshed identity here;
    /// frontend IPC never receives the Minecraft access token.
    pub async fn require_valid_minecraft_session(
        &self,
    ) -> Result<(SafeAccount, String, Option<String>), String> {
        let account = self
            .database
            .safe_account()
            .map_err(|_| "Could not read Minecraft account".to_string())?
            .ok_or_else(|| "Sign in to a Minecraft account first.".to_string())?;
        let live = self
            .memory_session
            .lock()
            .map_err(|_| "Authentication is unavailable".to_string())?
            .clone();
        if let Some((live_account, session)) = live
            .filter(|(_, session)| !session_needs_refresh(session.minecraft_expires_at, now_unix()))
        {
            return Ok((
                live_account,
                session.minecraft_access_token.clone(),
                session.xuid.clone(),
            ));
        }
        let mut refresh = self
            .credentials
            .load(&account.uuid)?
            .ok_or_else(|| "Your login expired. Please sign in again.".to_string())?;
        let result = self.exchange_refresh(&refresh).await;
        zeroize::Zeroize::zeroize(&mut refresh);
        let verified = result?;
        let safe = verified.safe();
        let _ = self
            .credentials
            .save(&safe.uuid, &verified.session.microsoft_refresh_token);
        let _ =
            self.database
                .save_account(&safe.uuid, &safe.username, verified.last_authenticated_at);
        if let Ok(mut live) = self.memory_session.lock() {
            *live = Some((safe.clone(), verified.session.clone()));
        }
        Ok((
            safe,
            verified.session.minecraft_access_token.clone(),
            verified.session.xuid.clone(),
        ))
    }

    pub fn retry_persist_session(&self) -> Result<AuthState, String> {
        let live = self
            .memory_session
            .lock()
            .map_err(|_| "Authentication is unavailable".to_string())?
            .clone()
            .ok_or_else(|| "Sign in to a Minecraft account first.".to_string())?;
        self.credentials
            .save(&live.0.uuid, &live.1.microsoft_refresh_token)?;
        self.database
            .save_account(&live.0.uuid, &live.0.username, live.0.last_authenticated_at)
            .map_err(|_| "Could not save account information".to_string())?;
        Ok(AuthState {
            status: "loggedIn".into(),
            account: Some(live.0),
            accounts: self.database.safe_accounts().unwrap_or_default(),
            persistence_warning: None,
        })
    }

    async fn complete_login(
        &self,
        secret: &DeviceCodeSecret,
        cancellation: CancellationToken,
    ) -> Result<VerifiedAccount, String> {
        let microsoft = self.poll_token(secret, cancellation).await?;
        let refresh = microsoft
            .refresh_token
            .filter(|value| !value.is_empty())
            .ok_or("Microsoft did not return a refresh credential. Please sign in again.")?;
        self.exchange_tokens(&microsoft.access_token, refresh).await
    }

    async fn poll_token(
        &self,
        secret: &DeviceCodeSecret,
        cancellation: CancellationToken,
    ) -> Result<MicrosoftToken, String> {
        let deadline = Instant::now() + Duration::from_secs(secret.expires_in.max(60));
        let mut interval = secret.interval.max(5);
        while Instant::now() < deadline {
            tokio::select! { _ = cancellation.cancelled() => return Err("Microsoft sign-in was cancelled.".into()), _ = sleep(Duration::from_secs(interval)) => {} }
            let response = tokio::select! {
                _ = cancellation.cancelled() => return Err("Microsoft sign-in was cancelled.".into()),
                response = self.client
                    .post(TOKEN_URL)
                    .form(&[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                        ("client_id", CLIENT_ID),
                        ("device_code", secret.device_code.as_str()),
                    ])
                    .send() => response.map_err(|_| "Could not connect to Microsoft sign-in".to_string())?,
            };
            if response.status().is_success() {
                return response
                    .json()
                    .await
                    .map_err(|_| "Microsoft returned an invalid token response".into());
            }
            let error = response
                .json::<MicrosoftError>()
                .await
                .map(|item| item.error)
                .unwrap_or_default();
            match error.as_str() {
                "authorization_pending" => {}
                "slow_down" => interval = interval.saturating_add(5).min(30),
                "authorization_declined" => return Err("Microsoft sign-in was cancelled.".into()),
                "expired_token" => {
                    return Err("Microsoft sign-in expired. Please try again.".into())
                }
                "bad_verification_code" => {
                    return Err("Microsoft rejected the sign-in code.".into())
                }
                _ => return Err("Microsoft sign-in failed. Please try again.".into()),
            }
        }
        Err("Microsoft sign-in expired. Please try again.".into())
    }

    async fn exchange_refresh(&self, refresh: &str) -> Result<VerifiedAccount, String> {
        let response = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("client_id", CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("scope", SCOPE),
            ])
            .send()
            .await
            .map_err(|error| {
                if error.is_connect() || error.is_timeout() {
                    OFFLINE_AUTH_REQUIRED.to_string()
                } else {
                    "Could not refresh Microsoft sign-in".to_string()
                }
            })?;
        let token: MicrosoftToken = parse_ok(response)
            .await
            .map_err(|_| "Your login expired. Please sign in again.".to_string())?;
        self.exchange_tokens(
            &token.access_token,
            token.refresh_token.unwrap_or_else(|| refresh.into()),
        )
        .await
    }

    async fn exchange_tokens(
        &self,
        microsoft_token: &str,
        refresh_token: String,
    ) -> Result<VerifiedAccount, String> {
        let xbox = self.xbox_request(XBOX_URL, json!({"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={microsoft_token}")},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"})).await?;
        let first_claim = xbox
            .claims
            .xui
            .first()
            .ok_or("Could not retrieve Xbox account identity.")?;
        let xsts = self.xbox_request(XSTS_URL, json!({"Properties":{"SandboxId":"RETAIL","UserTokens":[xbox.token]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"})).await?;
        let xsts_claim = xsts
            .claims
            .xui
            .first()
            .ok_or("Could not retrieve Xbox authorization.")?;
        let minecraft_response = self
            .client
            .post(MC_LOGIN_URL)
            .json(&json!({"identityToken":format!("XBL3.0 x={};{}", first_claim.uhs, xsts.token)}))
            .send()
            .await
            .map_err(|_| "Could not connect to Minecraft services".to_string())?;
        let minecraft: MinecraftToken = parse_ok(minecraft_response)
            .await
            .map_err(|_| "Could not authenticate with Minecraft services".to_string())?;
        let minecraft_expires_at = now_unix().saturating_add(minecraft.expires_in.max(0));
        let entitlement_response = self
            .client
            .get(MC_ENTITLEMENTS_URL)
            .bearer_auth(&minecraft.access_token)
            .send()
            .await
            .map_err(|_| "Could not check Minecraft ownership".to_string())?;
        let entitlements: Entitlements = parse_ok(entitlement_response)
            .await
            .map_err(|_| "Could not check Minecraft ownership".to_string())?;
        if !owns_java(&entitlements) {
            return Err(
                "This Microsoft account does not have access to Minecraft Java Edition.".into(),
            );
        }
        let profile_response = self
            .client
            .get(MC_PROFILE_URL)
            .bearer_auth(&minecraft.access_token)
            .send()
            .await
            .map_err(|_| "Could not retrieve your Minecraft profile".to_string())?;
        if profile_response.status() == StatusCode::NOT_FOUND {
            return Err("No Minecraft profile exists for this account.".into());
        }
        let profile: MinecraftProfile = parse_ok(profile_response)
            .await
            .map_err(|_| "Could not retrieve your Minecraft profile".to_string())?;
        let account = SafeAccount {
            uuid: normalize_uuid(&profile.id)?,
            username: profile.name,
            last_authenticated_at: now_unix(),
        };
        let session = LiveSession {
            microsoft_refresh_token: refresh_token,
            minecraft_access_token: minecraft.access_token,
            minecraft_expires_at,
            xuid: xsts_claim.xid.clone().or_else(|| first_claim.xid.clone()),
        };
        Ok(VerifiedAccount {
            uuid: account.uuid,
            username: account.username,
            last_authenticated_at: account.last_authenticated_at,
            session,
        })
    }

    async fn xbox_request(&self, url: &str, body: Value) -> Result<XboxResponse, String> {
        let response = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|_| "Could not connect to Xbox services".to_string())?;
        let status = response.status();
        if status.is_success() {
            return response
                .json()
                .await
                .map_err(|_| "Xbox services returned an invalid response".into());
        }
        let code = response
            .json::<Value>()
            .await
            .ok()
            .and_then(|value| value.get("XErr").and_then(Value::as_i64));
        match code {
            Some(2148916233) => Err("This Microsoft account does not have an Xbox profile.".into()),
            Some(2148916235) => {
                Err("Xbox services are not available in this account's region.".into())
            }
            Some(2148916236 | 2148916237) => {
                Err("Xbox account age verification is required.".into())
            }
            Some(2148916238) => Err("Xbox family approval is required for this account.".into()),
            _ => Err("Xbox authorization failed. Please try again.".into()),
        }
    }
}

#[derive(Clone)]
struct VerifiedAccount {
    uuid: String,
    username: String,
    last_authenticated_at: i64,
    session: LiveSession,
}
impl VerifiedAccount {
    fn safe(&self) -> SafeAccount {
        SafeAccount {
            uuid: self.uuid.clone(),
            username: self.username.clone(),
            last_authenticated_at: self.last_authenticated_at,
        }
    }
}
impl From<VerifiedAccount> for SafeAccount {
    fn from(value: VerifiedAccount) -> Self {
        value.safe()
    }
}

fn owns_java(value: &Entitlements) -> bool {
    value.items.iter().any(|item| {
        matches!(
            item.name.to_ascii_lowercase().as_str(),
            "game_minecraft" | "product_minecraft"
        )
    })
}

fn secure_texture_url(value: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("textures.minecraft.net"))
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with("/texture/")
    {
        return None;
    }
    if url.scheme() == "http" && url.set_scheme("https").is_err() {
        return None;
    }
    Some(url.to_string())
}

fn normalize_uuid(value: &str) -> Result<String, String> {
    let compact: String = value
        .chars()
        .filter(|character| *character != '-')
        .collect();
    if compact.len() != 32 || !compact.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Minecraft returned an invalid profile identifier.".into());
    }
    Ok(compact.to_ascii_lowercase())
}
fn valid_verification_url(value: &str) -> bool {
    reqwest::Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("microsoft.com")
                    || host.ends_with(".microsoft.com")
                    || host.eq_ignore_ascii_case("microsoftonline.com")
                    || host.ends_with(".microsoftonline.com")
            })
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
    })
}
async fn parse_ok<T: for<'de> Deserialize<'de>>(response: reqwest::Response) -> Result<T, String> {
    if !response.status().is_success() {
        return Err("Remote authentication service rejected the request".into());
    }
    response
        .json()
        .await
        .map_err(|_| "Remote authentication service returned an invalid response".into())
}
fn emit_state(app: &AppHandle, status: &str, account: Option<SafeAccount>) {
    emit_state_warning(app, status, account, None);
}
fn emit_state_warning(
    app: &AppHandle,
    status: &str,
    account: Option<SafeAccount>,
    persistence_warning: Option<String>,
) {
    emit_event(
        app,
        "auth-state-changed",
        json!({"status":status,"account":account,"persistenceWarning":persistence_warning}),
    );
}
fn emit_event(app: &AppHandle, event: &str, payload: Value) {
    let _ = app.emit(event, payload);
}
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn session_needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at <= now.saturating_add(300)
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_uuid, owns_java, session_needs_refresh, valid_verification_url, AuthService,
        AuthState, Entitlement, Entitlements, LiveSession, OFFLINE_AUTH_REQUIRED,
    };
    use crate::{
        auth::credentials::CredentialStore,
        database::{Database, SafeAccount},
    };
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Default)]
    struct TestCredentials(Mutex<HashMap<String, String>>);
    impl CredentialStore for TestCredentials {
        fn save(&self, id: &str, value: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(id.into(), value.into());
            Ok(())
        }
        fn load(&self, id: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(id).cloned())
        }
        fn delete(&self, id: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    fn test_service() -> (AuthService, Arc<Database>, Arc<TestCredentials>) {
        let database = Arc::new(Database::open(":memory:").unwrap());
        let credentials = Arc::new(TestCredentials::default());
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .build()
            .unwrap();
        let service = AuthService {
            client,
            database: Arc::clone(&database),
            credentials: credentials.clone(),
            memory_session: Arc::new(Mutex::new(None)),
            cancel: Arc::new(Mutex::new(None)),
        };
        (service, database, credentials)
    }

    #[test]
    fn frontend_auth_payload_contains_identity_but_no_credentials() {
        let json = serde_json::to_string(&AuthState {
            status: "loggedIn".into(),
            account: Some(SafeAccount {
                uuid: "0123456789abcdef0123456789abcdef".into(),
                username: "Player".into(),
                last_authenticated_at: 100,
            }),
            accounts: vec![],
            persistence_warning: Some("Signed in, but the session could not be saved.".into()),
        })
        .unwrap();
        assert!(json.contains("Player"));
        assert!(!json.contains("token"));
        assert!(!json.contains("credential"));
        assert!(json.contains("persistenceWarning"));
        assert!(json.contains("loggedIn"));
    }
    #[test]
    fn refresh_uses_expiry_safety_margin() {
        assert!(session_needs_refresh(1_299, 1_000));
        assert!(!session_needs_refresh(1_301, 1_000));
    }
    #[test]
    fn expired_session_offline_boundary_is_precise_and_typed() {
        assert!(OFFLINE_AUTH_REQUIRED.starts_with("OfflineAuthenticationRequired:"));
        assert!(session_needs_refresh(1, 1_000));
    }
    #[test]
    fn validates_only_microsoft_https_verification_urls() {
        assert!(valid_verification_url("https://microsoft.com/devicelogin"));
        assert!(valid_verification_url(
            "https://login.microsoftonline.com/common"
        ));
        assert!(!valid_verification_url("http://microsoft.com/devicelogin"));
        assert!(!valid_verification_url(
            "https://evil.example/microsoft.com"
        ));
    }
    #[test]
    fn normalizes_profile_uuid_and_rejects_invalid_values() {
        assert_eq!(
            normalize_uuid("01234567-89ab-cdef-0123-456789abcdef").unwrap(),
            "0123456789abcdef0123456789abcdef"
        );
        assert!(normalize_uuid("not-a-uuid").is_err());
    }
    #[test]
    fn only_recognized_minecraft_entitlements_count_as_ownership() {
        assert!(owns_java(&Entitlements {
            items: vec![Entitlement {
                name: "game_minecraft".into()
            }]
        }));
        assert!(!owns_java(&Entitlements {
            items: vec![Entitlement {
                name: "other_game".into()
            }]
        }));
    }

    #[test]
    fn logout_removes_credential_clears_memory_and_fresh_service_is_logged_out() {
        let (service, database, credentials) = test_service();
        let account = SafeAccount {
            uuid: "fixture-account".into(),
            username: "FixturePlayer".into(),
            last_authenticated_at: 1,
        };
        database
            .save_account(
                &account.uuid,
                &account.username,
                account.last_authenticated_at,
            )
            .unwrap();
        credentials
            .save(&account.uuid, "fixture-refresh-only")
            .unwrap();
        *service.memory_session.lock().unwrap() = Some((
            account.clone(),
            LiveSession {
                microsoft_refresh_token: "fixture-refresh-only".into(),
                minecraft_access_token: "fixture-short-lived".into(),
                minecraft_expires_at: i64::MAX,
                xuid: None,
            },
        ));

        let state = service.sign_out_local().unwrap();
        assert_eq!(state.status, "loggedOut");
        assert!(credentials.load(&account.uuid).unwrap().is_none());
        assert!(service.memory_session.lock().unwrap().is_none());
        assert!(database.safe_account().unwrap().is_none());

        let (restarted, _, _) = test_service();
        assert_eq!(restarted.state().unwrap().status, "loggedOut");
    }

    #[test]
    fn saved_account_and_refresh_credential_are_available_for_async_restore() {
        let (service, database, credentials) = test_service();
        database
            .save_account("fixture-id", "FixturePlayer", 1)
            .unwrap();
        credentials
            .save("fixture-id", "fixture-refresh-only")
            .unwrap();
        let safe_state = service.state().unwrap();
        assert_eq!(safe_state.status, "loggedIn");
        assert_eq!(safe_state.account.unwrap().username, "FixturePlayer");
        assert_eq!(
            credentials.load("fixture-id").unwrap().as_deref(),
            Some("fixture-refresh-only")
        );
        assert!(session_needs_refresh(1, 1_000));
    }

    #[tokio::test]
    async fn valid_cached_minecraft_session_works_without_network() {
        let (service, database, _) = test_service();
        let account = SafeAccount {
            uuid: "fixture-id".into(),
            username: "FixturePlayer".into(),
            last_authenticated_at: 1,
        };
        database
            .save_account(&account.uuid, &account.username, 1)
            .unwrap();
        *service.memory_session.lock().unwrap() = Some((
            account,
            LiveSession {
                microsoft_refresh_token: "fixture-refresh-only".into(),
                minecraft_access_token: "fixture-short-lived".into(),
                minecraft_expires_at: i64::MAX,
                xuid: Some("fixture-xuid".into()),
            },
        ));
        let result = service.require_valid_minecraft_session().await.unwrap();
        assert_eq!(result.0.username, "FixturePlayer");
        assert_eq!(result.1, "fixture-short-lived");
    }

    #[tokio::test]
    async fn expired_session_with_unavailable_transport_returns_offline_auth_error() {
        let (service, database, credentials) = test_service();
        database
            .save_account("fixture-id", "FixturePlayer", 1)
            .unwrap();
        credentials
            .save("fixture-id", "fixture-refresh-only")
            .unwrap();
        let error = match service.require_valid_minecraft_session().await {
            Ok(_) => panic!("offline refresh unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error, OFFLINE_AUTH_REQUIRED);
    }
}
