use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const SERVICE: &str = "SNineLauncher/MicrosoftAuth";
const USER: &str = "microsoft-refresh-token";
const TARGET_PREFIX: &str = "SNineLauncher/MicrosoftAuth/";
const MAX_CREDENTIAL_BLOB_BYTES: usize = 2560;
const MAX_DPAPI_FILE_BYTES: u64 = 64 * 1024;

pub trait CredentialStore: Send + Sync {
    fn save(&self, account_id: &str, refresh_token: &str) -> Result<(), String>;
    fn load(&self, account_id: &str) -> Result<Option<String>, String>;
    fn delete(&self, account_id: &str) -> Result<(), String>;
}

pub struct WindowsCredentialStore {
    auth_dir: PathBuf,
    target_prefix: String,
}

#[derive(Serialize, Deserialize)]
struct StoredRefreshToken {
    version: u8,
    account_id: String,
    microsoft_refresh_token: String,
}

#[derive(Serialize, Deserialize)]
struct StoredRefreshVault {
    version: u8,
    accounts: Vec<StoredRefreshToken>,
}

#[derive(Deserialize)]
struct LegacySession {
    microsoft_refresh_token: String,
}

impl Drop for StoredRefreshToken {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.microsoft_refresh_token.zeroize();
    }
}
impl Drop for LegacySession {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.microsoft_refresh_token.zeroize();
    }
}

impl WindowsCredentialStore {
    pub fn new(auth_dir: impl Into<PathBuf>) -> Self {
        Self {
            auth_dir: auth_dir.into(),
            target_prefix: TARGET_PREFIX.into(),
        }
    }

    fn entry(&self, account_id: &str) -> Result<keyring::Entry, String> {
        let target = format!("{}{account_id}", self.target_prefix);
        keyring::Entry::new_with_target(&target, SERVICE, USER).map_err(|error| {
            format!(
                "CredentialStoreUnavailable ({})",
                keyring_error_kind(&error)
            )
        })
    }

    fn fallback_path(&self) -> PathBuf {
        self.auth_dir.join("credentials.dat")
    }

    fn fallback_save(&self, account_id: &str, record: &[u8]) -> Result<(), String> {
        let mut vault = self.read_fallback_vault()?;
        let account: StoredRefreshToken = serde_json::from_slice(record)
            .map_err(|_| "CredentialWriteFailed: invalid account credential".to_string())?;
        vault.accounts.retain(|item| item.account_id != account_id);
        vault.accounts.push(account);
        let payload = serde_json::to_vec(&vault)
            .map_err(|_| "CredentialWriteFailed: credential vault serialization".to_string())?;
        let encrypted = dpapi_protect(&payload)?;
        use zeroize::Zeroize;
        let mut payload = payload;
        payload.zeroize();
        self.write_fallback(&encrypted)?;
        eprintln!("[auth-storage] operation=write backend=dpapi-current-user target=SNineLauncher/auth/credentials.dat account={account_id} accounts={} blob_len={}", vault.accounts.len(), encrypted.len());
        Ok(())
    }

    fn write_fallback(&self, encrypted: &[u8]) -> Result<(), String> {
        fs::create_dir_all(&self.auth_dir).map_err(|error| {
            format!(
                "CredentialWriteFailed: auth directory (os {})",
                error.raw_os_error().unwrap_or_default()
            )
        })?;
        let path = self.fallback_path();
        let temporary = self.auth_dir.join("credentials.dat.tmp");
        fs::write(&temporary, encrypted).map_err(|error| {
            format!(
                "CredentialWriteFailed: DPAPI file (os {})",
                error.raw_os_error().unwrap_or_default()
            )
        })?;
        atomic_replace(&temporary, &path)?;
        Ok(())
    }

    fn read_fallback_vault(&self) -> Result<StoredRefreshVault, String> {
        let path = self.fallback_path();
        let encrypted = match fs::read(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(StoredRefreshVault {
                    version: 1,
                    accounts: Vec::new(),
                })
            }
            Err(error) => {
                return Err(format!(
                    "CredentialReadFailed: DPAPI file (os {})",
                    error.raw_os_error().unwrap_or_default()
                ))
            }
        };
        if encrypted.len() as u64 > MAX_DPAPI_FILE_BYTES {
            return Err("CredentialCorrupt: encrypted credential file exceeds safe size".into());
        }
        let mut clear =
            dpapi_unprotect(&encrypted).map_err(|error| format!("CredentialCorrupt: {error}"))?;
        let parsed = serde_json::from_slice::<StoredRefreshVault>(&clear).or_else(|_| {
            serde_json::from_slice::<StoredRefreshToken>(&clear).map(|legacy| StoredRefreshVault {
                version: 1,
                accounts: vec![legacy],
            })
        });
        use zeroize::Zeroize;
        clear.zeroize();
        let vault = parsed.map_err(|_| "CredentialCorrupt: invalid DPAPI payload".to_string())?;
        if vault.version != 1 {
            return Err("CredentialCorrupt: unsupported credential vault".into());
        }
        Ok(vault)
    }

    fn fallback_delete(&self, account_id: &str) -> Result<(), String> {
        let mut vault = self.read_fallback_vault()?;
        vault.accounts.retain(|item| item.account_id != account_id);
        if vault.accounts.is_empty() {
            return self.remove_fallback_file();
        }
        let mut payload = serde_json::to_vec(&vault)
            .map_err(|_| "CredentialWriteFailed: credential vault serialization".to_string())?;
        let encrypted = dpapi_protect(&payload)?;
        use zeroize::Zeroize;
        payload.zeroize();
        self.write_fallback(&encrypted)
    }

    fn fallback_load(&self, account_id: &str) -> Result<Option<String>, String> {
        let vault = self.read_fallback_vault()?;
        Ok(vault
            .accounts
            .into_iter()
            .find(|record| {
                record.version == 1
                    && record.account_id == account_id
                    && !record.microsoft_refresh_token.is_empty()
            })
            .map(|record| record.microsoft_refresh_token.clone()))
    }
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            windows::core::PCWSTR(source_wide.as_ptr()),
            windows::core::PCWSTR(target_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| {
        format!(
            "CredentialWriteFailed: atomic replace HRESULT 0x{:08X}",
            error.code().0 as u32
        )
    })
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|error| {
        format!(
            "CredentialWriteFailed: atomic rename (os {})",
            error.raw_os_error().unwrap_or_default()
        )
    })
}

impl CredentialStore for WindowsCredentialStore {
    fn save(&self, account_id: &str, refresh_token: &str) -> Result<(), String> {
        let record = StoredRefreshToken {
            version: 1,
            account_id: account_id.into(),
            microsoft_refresh_token: refresh_token.into(),
        };
        let mut payload = serde_json::to_vec(&record)
            .map_err(|_| "CredentialWriteFailed: serialization".to_string())?;
        let result = (|| {
            let entry = self.entry(account_id)?;
            match entry.set_secret(&payload) {
                Ok(()) => {
                    eprintln!("[auth-storage] operation=write backend=windows-credential-manager credential_type=generic target={TARGET_PREFIX}{account_id} account={account_id} blob_len={} payload_len={}", refresh_token.len(), payload.len());
                    Ok(())
                }
                Err(error) => {
                    let kind = keyring_error_kind(&error);
                    eprintln!("[auth-storage] operation=write backend=windows-credential-manager credential_type=generic target={TARGET_PREFIX}{account_id} account={account_id} blob_len={} payload_len={} error_type={kind}", refresh_token.len(), payload.len());
                    if matches!(error, keyring::Error::TooLong(_, _))
                        || refresh_token.len() > MAX_CREDENTIAL_BLOB_BYTES
                    {
                        eprintln!("[auth-storage] diagnostic=CredentialPayloadTooLarge limit_bytes={MAX_CREDENTIAL_BLOB_BYTES}");
                    }
                    self.fallback_save(account_id, &payload).map_err(|fallback| {
                    eprintln!("[auth-storage] operation=write backend=dpapi-current-user target=SNineLauncher/auth/credentials.dat account={account_id} error_type={fallback}");
                    "Could not securely save your Microsoft session. Please keep the launcher open and retry.".to_string()
                })
                }
            }
        })();
        use zeroize::Zeroize;
        payload.zeroize();
        result
    }

    fn load(&self, account_id: &str) -> Result<Option<String>, String> {
        let entry = self.entry(account_id)?;
        match entry.get_secret() {
            Ok(mut bytes) => {
                let current_format = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|value| serde_json::from_str::<StoredRefreshToken>(value).ok())
                    .is_some();
                let parsed = std::str::from_utf8(&bytes)
                    .map_err(|_| "CredentialCorruptEncoding".to_string())
                    .and_then(|value| parse_keyring_payload(account_id, value));
                use zeroize::Zeroize;
                let size = bytes.len();
                bytes.zeroize();
                match parsed {
                    Ok(Some(token)) => {
                        if !current_format {
                            self.save(account_id, &token)?;
                        }
                        eprintln!("[auth-storage] operation=read backend=windows-credential-manager target={TARGET_PREFIX}{account_id} account={account_id} payload_len={size}");
                        Ok(Some(token))
                    }
                    Ok(None) => self.fallback_load(account_id),
                    Err(error) => {
                        eprintln!("[auth-storage] operation=read backend=windows-credential-manager target={TARGET_PREFIX}{account_id} account={account_id} payload_len={size} error_type={error}");
                        let _ = entry.delete_credential();
                        self.fallback_load(account_id)
                    }
                }
            }
            Err(keyring::Error::NoEntry) => {
                eprintln!("[auth-storage] operation=read backend=windows-credential-manager target={TARGET_PREFIX}{account_id} account={account_id} result=missing");
                // Migrate the prior per-user target and old full-session JSON. Only
                // its refresh token is copied to the new minimal credential.
                match legacy_entry(account_id).and_then(|entry| {
                    entry
                        .get_password()
                        .map_err(|error| keyring_error_kind(&error))
                }) {
                    Ok(mut old) => {
                        let refresh = legacy_refresh_token(&old);
                        use zeroize::Zeroize;
                        let size = old.len();
                        old.zeroize();
                        if let Some(refresh) = refresh {
                            self.save(account_id, &refresh)?;
                            let _ = legacy_entry(account_id).and_then(|entry| {
                                entry
                                    .delete_credential()
                                    .map_err(|error| keyring_error_kind(&error))
                            });
                            return Ok(Some(refresh));
                        }
                        eprintln!("[auth-storage] operation=read backend=legacy-keyring target=SNine Launcher.minecraft:{account_id} account={account_id} payload_len={size} result=corrupt");
                        let _ = legacy_entry(account_id).and_then(|entry| {
                            entry
                                .delete_credential()
                                .map_err(|error| keyring_error_kind(&error))
                        });
                        self.fallback_load(account_id)
                    }
                    Err(_) => self.fallback_load(account_id),
                }
            }
            Err(error) => {
                let kind = keyring_error_kind(&error);
                eprintln!("[auth-storage] operation=read backend=windows-credential-manager target={TARGET_PREFIX}{account_id} account={account_id} error_type={kind}");
                self.fallback_load(account_id)
            }
        }
    }

    fn delete(&self, account_id: &str) -> Result<(), String> {
        let entry = self.entry(account_id)?;
        let mut cm_error = None;
        if let Err(error) = entry.delete_credential() {
            if !matches!(error, keyring::Error::NoEntry) {
                let kind = keyring_error_kind(&error);
                eprintln!("[auth-storage] operation=delete backend=windows-credential-manager target={TARGET_PREFIX}{account_id} account={account_id} error_type={kind}");
                cm_error = Some(format!("CredentialDeleteFailed ({kind})"));
            }
        }
        let legacy_error = legacy_entry(account_id)
            .and_then(|entry| match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(keyring_error_kind(&error)),
            })
            .err();
        if let Some(error) = &legacy_error {
            eprintln!("[auth-storage] operation=delete backend=legacy-keyring target=SNine Launcher.minecraft:{account_id} account={account_id} error_type={error}");
        }
        self.fallback_delete(account_id)?;
        eprintln!("[auth-storage] operation=delete backend=windows-credential-manager+dpapi-current-user target={TARGET_PREFIX}{account_id} account={account_id}");
        cm_error.or(legacy_error).map_or(Ok(()), Err)
    }
}

fn remove_fallback_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "CredentialDeleteFailed: DPAPI file (os {})",
            error.raw_os_error().unwrap_or_default()
        )),
    }
}

impl WindowsCredentialStore {
    fn remove_fallback_file(&self) -> Result<(), String> {
        remove_fallback_file(&self.fallback_path())
    }
}

fn legacy_entry(account_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("SNine Launcher", &format!("minecraft:{account_id}"))
        .map_err(|error| keyring_error_kind(&error))
}

fn parse_keyring_payload(account_id: &str, value: &str) -> Result<Option<String>, String> {
    match serde_json::from_str::<StoredRefreshToken>(value) {
        Ok(record)
            if record.version == 1
                && record.account_id == account_id
                && !record.microsoft_refresh_token.is_empty() =>
        {
            Ok(Some(record.microsoft_refresh_token.clone()))
        }
        Ok(_) => Err("CredentialCorrupt".into()),
        Err(_) => legacy_refresh_token(value)
            .map(Some)
            .ok_or_else(|| "CredentialCorrupt".into()),
    }
}

fn legacy_refresh_token(value: &str) -> Option<String> {
    serde_json::from_str::<LegacySession>(value)
        .ok()
        .map(|session| session.microsoft_refresh_token.clone())
        .filter(|token| !token.is_empty())
}

fn keyring_error_kind(error: &keyring::Error) -> String {
    match error {
        keyring::Error::NoEntry => "NoEntry".into(),
        keyring::Error::TooLong(field, limit) => {
            format!("CredentialPayloadTooLarge(field={field},limit={limit})")
        }
        keyring::Error::Invalid(field, _) => format!("CredentialInvalid(field={field})"),
        keyring::Error::PlatformFailure(source) | keyring::Error::NoStorageAccess(source) => {
            #[cfg(windows)]
            if let Some(error) = source.downcast_ref::<keyring::windows::Error>() {
                return format!("WindowsCredentialError(0x{:08X})", error.0);
            }
            if matches!(error, keyring::Error::PlatformFailure(_)) {
                "PlatformFailure".into()
            } else {
                "NoStorageAccess".into()
            }
        }
        keyring::Error::BadEncoding(_) => "CredentialCorruptEncoding".into(),
        keyring::Error::Ambiguous(entries) => {
            format!("CredentialAmbiguous(count={})", entries.len())
        }
        _ => "CredentialStoreError".into(),
    }
}

#[cfg(windows)]
fn dpapi_protect(input: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input_blob,
            windows::core::PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|error| format!("DPAPI protect HRESULT 0x{:08X}", error.code().0 as u32))?;
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData.cast(),
        )))
    };
    Ok(bytes)
}

#[cfg(not(windows))]
fn dpapi_protect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI is only available on Windows".into())
}

#[cfg(windows)]
fn dpapi_unprotect(input: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input_blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|error| format!("DPAPI unprotect HRESULT 0x{:08X}", error.code().0 as u32))?;
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData.cast(),
        )))
    };
    Ok(bytes)
}

#[cfg(not(windows))]
fn dpapi_unprotect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI is only available on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::{parse_keyring_payload, CredentialStore, StoredRefreshToken};
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct MemoryCredentialStore(Mutex<HashMap<String, String>>);
    impl CredentialStore for MemoryCredentialStore {
        fn save(&self, account_id: &str, refresh_token: &str) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(account_id.into(), refresh_token.into());
            Ok(())
        }
        fn load(&self, account_id: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(account_id).cloned())
        }
        fn delete(&self, account_id: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(account_id);
            Ok(())
        }
    }

    #[test]
    fn credential_store_roundtrip_delete_and_missing() {
        let store = MemoryCredentialStore::default();
        assert_eq!(store.load("user").unwrap(), None);
        store.save("user", "fixture-refresh-token").unwrap();
        assert_eq!(
            store.load("user").unwrap().as_deref(),
            Some("fixture-refresh-token")
        );
        store.delete("user").unwrap();
        assert_eq!(store.load("user").unwrap(), None);
    }

    #[test]
    fn minimal_secret_serialization_roundtrips_without_access_tokens() {
        let value = serde_json::to_string(&StoredRefreshToken {
            version: 1,
            account_id: "uuid".into(),
            microsoft_refresh_token: "test-refresh-only".into(),
        })
        .unwrap();
        assert_eq!(
            parse_keyring_payload("uuid", &value).unwrap().as_deref(),
            Some("test-refresh-only")
        );
        assert!(!value.contains("minecraftAccessToken"));
    }

    #[test]
    fn legacy_session_migrates_only_refresh_credential() {
        let old = r#"{"microsoft_refresh_token":"fixture-refresh","minecraft_access_token":"not-real","minecraft_expires_at":1,"xuid":null}"#;
        assert_eq!(
            parse_keyring_payload("uuid", old).unwrap().as_deref(),
            Some("fixture-refresh")
        );
    }

    #[test]
    fn corrupt_and_wrong_account_payloads_are_rejected() {
        assert!(parse_keyring_payload("uuid", "{broken").is_err());
        let other = r#"{"version":1,"account_id":"another","microsoft_refresh_token":"fixture"}"#;
        assert!(parse_keyring_payload("uuid", other).is_err());
    }

    #[test]
    fn deletes_only_the_dpapi_fallback_file_at_the_supplied_auth_path() {
        let dir = std::env::temp_dir().join(format!("snine-auth-delete-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("credentials.dat");
        std::fs::write(&file, b"test-encrypted-placeholder").unwrap();
        super::remove_fallback_file(&file).unwrap();
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn windows_credential_manager_isolated_test_target_can_be_deleted() {
        let id = uuid::Uuid::new_v4().to_string();
        let dir = std::env::temp_dir().join(format!("snine-auth-store-test-{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        let store = super::WindowsCredentialStore {
            auth_dir: dir.clone(),
            target_prefix: "SNineLauncher/Test/Auth/".into(),
        };
        store.save(&id, "fixture-only-test-value").unwrap();
        let read = store.load(&id);
        let deleted = store.delete(&id);
        let after_delete = store.load(&id);
        assert_eq!(read.unwrap().as_deref(), Some("fixture-only-test-value"));
        assert!(deleted.is_ok());
        assert!(after_delete.unwrap().is_none());
        assert!(!dir.join("credentials.dat").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip_is_bound_to_current_user() {
        let encrypted = super::dpapi_protect(b"fixture-only").unwrap();
        assert_ne!(encrypted, b"fixture-only");
        assert_eq!(super::dpapi_unprotect(&encrypted).unwrap(), b"fixture-only");
    }
}
