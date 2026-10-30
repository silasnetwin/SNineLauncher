use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Database(Mutex<Connection>);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub group_name: String,
    pub client_version: String,
    pub minecraft_version: String,
    pub installed: bool,
    pub client_ready: bool,
    pub selected: bool,
    pub favorite: bool,
    pub hidden: bool,
    pub loader_type: LoaderType,
    pub loader_version: Option<String>,
    pub snineclient_enabled: bool,
    pub icon_id: Option<String>,
    pub memory_mb: Option<u32>,
    pub snineclient_version: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProfileConfig {
    pub name: String,
    pub minecraft_version: String,
    pub loader_type: LoaderType,
    pub loader_version: Option<String>,
    pub snineclient_enabled: bool,
    pub memory_mb: Option<u32>,
    pub icon_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LoaderType {
    Vanilla,
    Fabric,
    NeoForge,
    Forge,
    Quilt,
}

impl LoaderType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Fabric => "fabric",
            Self::NeoForge => "neoforge",
            Self::Forge => "forge",
            Self::Quilt => "quilt",
        }
    }
    fn parse(value: &str) -> Self {
        match value {
            "vanilla" => Self::Vanilla,
            "neoforge" => Self::NeoForge,
            "forge" => Self::Forge,
            "quilt" => Self::Quilt,
            _ => Self::Fabric,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub profiles: Vec<Profile>,
    pub selected_profile_id: Option<String>,
    pub schema_version: u32,
    pub recent_profile_ids: Vec<String>,
    pub activity: Vec<ActivityEntry>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub profile_id: Option<String>,
    pub occurred_at: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeAccount {
    pub uuid: String,
    pub username: String,
    pub last_authenticated_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftWindowSettings {
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedMod {
    pub profile_id: String,
    pub mod_id: String,
    pub project_id: String,
    pub slug: String,
    pub version_id: String,
    pub filename: String,
    pub hash: String,
    pub enabled: bool,
    pub managed: bool,
    pub icon_data_url: Option<String>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS profiles (id TEXT PRIMARY KEY, name TEXT NOT NULL, client_version TEXT NOT NULL, minecraft_version TEXT NOT NULL, installed INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS recent_profiles (profile_id TEXT PRIMARY KEY REFERENCES profiles(id) ON DELETE CASCADE, used_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS activity (id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL, detail TEXT NOT NULL, profile_id TEXT REFERENCES profiles(id) ON DELETE SET NULL, occurred_at INTEGER NOT NULL); INSERT OR IGNORE INTO metadata(key,value) VALUES('schema_version','1'); INSERT OR IGNORE INTO settings(key,value) VALUES('selected_profile_id',''); INSERT OR IGNORE INTO settings(key,value) VALUES('minecraft_memory_mb','4096'); INSERT OR IGNORE INTO settings(key,value) VALUES('download_concurrency','4'); INSERT OR IGNORE INTO settings(key,value) VALUES('minimize_on_game_start','0'); INSERT OR IGNORE INTO settings(key,value) VALUES('minecraft_window_width','1280'); INSERT OR IGNORE INTO settings(key,value) VALUES('minecraft_window_height','720'); INSERT OR IGNORE INTO settings(key,value) VALUES('minecraft_window_fullscreen','0');")?;
        migrate_schema(&mut connection)?;
        // Recent selections are useful immediately on a fresh install.
        connection.execute(
            "INSERT OR IGNORE INTO profiles(id,name,client_version,minecraft_version,installed,created_at) VALUES('snineclient-default','SNineClient','Latest','Latest supported',0,0)",
            [],
        )?;
        connection.execute(
            "UPDATE settings SET value='snineclient-default' WHERE key='selected_profile_id' AND value=''",
            [],
        )?;
        Ok(Self(Mutex::new(connection)))
    }

    pub fn bootstrap(&self) -> rusqlite::Result<Bootstrap> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let selected: String = db.query_row(
            "SELECT value FROM settings WHERE key='selected_profile_id'",
            [],
            |r| r.get(0),
        )?;
        let mut stmt = db.prepare("SELECT id,name,client_version,minecraft_version,installed,favorite,hidden,client_ready,loader_type,loader_version,snineclient_enabled,icon_id,memory_mb,snineclient_version,group_name FROM profiles ORDER BY created_at")?;
        let profiles = stmt
            .query_map([], |r| {
                let id: String = r.get(0)?;
                Ok(Profile {
                    selected: id == selected,
                    id,
                    name: r.get(1)?,
                    group_name: r.get(14)?,
                    client_version: r.get(2)?,
                    minecraft_version: r.get(3)?,
                    installed: r.get::<_, i64>(4)? != 0,
                    favorite: r.get::<_, i64>(5)? != 0,
                    hidden: r.get::<_, i64>(6)? != 0,
                    client_ready: r.get::<_, i64>(7)? != 0,
                    loader_type: LoaderType::parse(&r.get::<_, String>(8)?),
                    loader_version: r.get(9)?,
                    snineclient_enabled: r.get::<_, i64>(10)? != 0,
                    icon_id: r.get(11)?,
                    memory_mb: r.get(12)?,
                    snineclient_version: r.get(13)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let version: String = db.query_row(
            "SELECT value FROM metadata WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )?;
        let mut recent_stmt =
            db.prepare("SELECT profile_id FROM recent_profiles ORDER BY used_at DESC LIMIT 8")?;
        let recent_profile_ids = recent_stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        let mut activity_stmt = db.prepare("SELECT id,kind,title,detail,profile_id,occurred_at FROM activity ORDER BY occurred_at DESC,rowid DESC LIMIT 100")?;
        let activity = activity_stmt
            .query_map([], |row| {
                Ok(ActivityEntry {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    title: row.get(2)?,
                    detail: row.get(3)?,
                    profile_id: row.get(4)?,
                    occurred_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Bootstrap {
            selected_profile_id: if selected.is_empty() {
                None
            } else {
                Some(selected)
            },
            profiles,
            schema_version: version.parse().unwrap_or(1),
            recent_profile_ids,
            activity,
        })
    }

    #[cfg(test)]
    pub fn create_profile(&self, name: String) -> rusqlite::Result<Profile> {
        self.create_profile_config(NewProfileConfig {
            name,
            minecraft_version: "1.21.11".into(),
            loader_type: LoaderType::Fabric,
            loader_version: None,
            snineclient_enabled: false,
            memory_mb: None,
            icon_id: None,
        })
    }

    #[cfg(test)]
    pub fn create_profile_with_memory(
        &self,
        name: String,
        memory_mb: Option<u32>,
    ) -> rusqlite::Result<Profile> {
        self.create_profile_config(NewProfileConfig {
            name,
            minecraft_version: "1.21.11".into(),
            loader_type: LoaderType::Fabric,
            loader_version: None,
            snineclient_enabled: false,
            memory_mb,
            icon_id: None,
        })
    }

    pub fn create_profile_config(&self, config: NewProfileConfig) -> rusqlite::Result<Profile> {
        let name = validate_profile_name(config.name)?;
        let minecraft_version = validate_minecraft_version(config.minecraft_version)?;
        let loader_type = config.loader_type;
        let loader_version = config
            .loader_version
            .filter(|value| !value.trim().is_empty());
        let snineclient_enabled = config.snineclient_enabled;
        let memory_mb = config.memory_mb;
        let icon_id = config.icon_id;
        if snineclient_enabled && !supports_snineclient(&minecraft_version, loader_type) {
            return Err(rusqlite::Error::InvalidParameterName(
                "SNineClient requires Fabric on Minecraft 1.21.11, 26.1.2, or 26.3".into(),
            ));
        }
        if let Some(icon) = icon_id.as_deref() {
            if !valid_icon_id(icon) {
                return Err(rusqlite::Error::InvalidParameterName(
                    "Invalid Minecraft block icon identity".into(),
                ));
            }
        }
        if memory_mb
            .is_some_and(|value| !(2048..=32768).contains(&value) || !value.is_multiple_of(512))
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "Profile memory must be 2048–32768 MB in 512 MB increments".into(),
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let mut db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        tx.execute("INSERT INTO profiles(id,name,client_version,minecraft_version,installed,created_at,loader_type,loader_version,snineclient_enabled,memory_mb,icon_id,snineclient_version,updated_at) VALUES(?1,?2,?3,?4,0,?5,?6,?7,?8,?9,?10,?11,?5)", params![id,name,if snineclient_enabled { "1.1.5" } else { "Vanilla" },minecraft_version,timestamp,loader_type.as_str(),loader_version,snineclient_enabled as i64,memory_mb,icon_id,if snineclient_enabled { Some("1.1.5") } else { None }])?;
        tx.execute(
            "UPDATE settings SET value=?1 WHERE key='selected_profile_id'",
            [&id],
        )?;
        let timestamp = unix_now();
        tx.execute("INSERT INTO recent_profiles(profile_id,used_at) VALUES(?1,?2) ON CONFLICT(profile_id) DO UPDATE SET used_at=excluded.used_at", params![id, timestamp])?;
        tx.execute("DELETE FROM recent_profiles WHERE profile_id NOT IN (SELECT profile_id FROM recent_profiles ORDER BY used_at DESC LIMIT 8)", [])?;
        insert_activity(&tx, "profile-created", "Profile created", &name, Some(&id))?;
        tx.commit()?;
        Ok(Profile {
            selected: true,
            id,
            name,
            group_name: String::new(),
            client_version: if snineclient_enabled {
                "1.1.5".into()
            } else {
                "Vanilla".into()
            },
            minecraft_version,
            installed: false,
            client_ready: false,
            favorite: false,
            hidden: false,
            loader_type,
            loader_version,
            snineclient_enabled,
            icon_id,
            memory_mb,
            snineclient_version: if snineclient_enabled {
                Some("1.1.5".into())
            } else {
                None
            },
        })
    }

    pub fn rename_profile(&self, id: &str, name: String) -> rusqlite::Result<Profile> {
        let name = validate_profile_name(name)?;
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let changed = db.execute(
            "UPDATE profiles SET name=?1,updated_at=?2 WHERE id=?3",
            params![name, unix_now(), id],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        insert_activity(&db, "profile-renamed", "Profile renamed", &name, Some(id))?;
        load_profile(&db, id)
    }

    pub fn set_profile_icon(&self, id: &str, icon_id: Option<String>) -> rusqlite::Result<()> {
        if icon_id
            .as_deref()
            .is_some_and(|value| !valid_icon_id(value))
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "Invalid Minecraft icon identifier".into(),
            ));
        }
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let changed = db.execute(
            "UPDATE profiles SET icon_id=?1,updated_at=?2 WHERE id=?3",
            params![icon_id, unix_now(), id],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        Ok(())
    }

    pub fn duplicate_profile(&self, id: &str, name: String) -> rusqlite::Result<Profile> {
        let name = validate_profile_name(name)?;
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let source = load_profile(&db, id)?;
        let new_id = uuid::Uuid::new_v4().to_string();
        let timestamp = unix_now();
        db.execute("INSERT INTO profiles(id,name,client_version,minecraft_version,installed,created_at,favorite,hidden,client_ready,loader_type,loader_version,snineclient_enabled,icon_id,memory_mb,snineclient_version,updated_at,group_name) VALUES(?1,?2,?3,?4,0,?5,0,0,0,?6,?7,?8,?9,?10,?11,?5,?12)", params![new_id, name, source.client_version, source.minecraft_version, timestamp, source.loader_type.as_str(), source.loader_version, source.snineclient_enabled as i64, source.icon_id, source.memory_mb, source.snineclient_version, source.group_name])?;
        insert_activity(
            &db,
            "profile-duplicated",
            "Profile duplicated",
            &name,
            Some(&new_id),
        )?;
        load_profile(&db, &new_id)
    }

    pub fn set_profile_group(&self, id: &str, group_name: String) -> rusqlite::Result<()> {
        let group_name = group_name.trim().to_owned();
        if group_name.chars().count() > 48 || group_name.chars().any(char::is_control) {
            return Err(rusqlite::Error::InvalidParameterName(
                "Group name must be at most 48 printable characters".into(),
            ));
        }
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        if db.execute(
            "UPDATE profiles SET group_name=?1,updated_at=?2 WHERE id=?3",
            params![group_name, unix_now(), id],
        )? == 0
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        Ok(())
    }

    pub fn delete_empty_profile(&self, id: &str) -> rusqlite::Result<()> {
        if id == "snineclient-default" {
            return Err(rusqlite::Error::InvalidParameterName(
                "The primary SNineClient profile cannot be deleted".into(),
            ));
        }
        let mut db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        let row: Option<(String, i64, i64)> = tx
            .query_row(
                "SELECT name,installed,client_ready FROM profiles WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((name, installed, client_ready)) = row else {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        };
        if installed != 0 || client_ready != 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "Uninstall this profile before deleting it".into(),
            ));
        }
        tx.execute("UPDATE settings SET value='snineclient-default' WHERE key='selected_profile_id' AND value=?1", [id])?;
        tx.execute("DELETE FROM profiles WHERE id=?1", [id])?;
        insert_activity(&tx, "profile-deleted", "Profile deleted", &name, None)?;
        tx.commit()
    }

    pub fn managed_mods(&self, profile_id: &str) -> rusqlite::Result<Vec<ManagedMod>> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let mut stmt = db.prepare("SELECT profile_id,project_id,slug,version_id,filename,hash,enabled FROM profile_mods WHERE profile_id=?1 ORDER BY slug")?;
        let rows = stmt.query_map([profile_id], |row| {
            Ok(ManagedMod {
                profile_id: row.get(0)?,
                mod_id: row.get(2)?,
                project_id: row.get(1)?,
                slug: row.get(2)?,
                version_id: row.get(3)?,
                filename: row.get(4)?,
                hash: row.get(5)?,
                enabled: row.get::<_, i64>(6)? != 0,
                managed: true,
                icon_data_url: None,
            })
        })?;
        rows.collect()
    }

    pub fn record_managed_mods(&self, mods: &[ManagedMod]) -> rusqlite::Result<()> {
        let mut db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        for item in mods {
            tx.execute(
                "DELETE FROM profile_mods WHERE profile_id=?1 AND project_id=?2",
                params![item.profile_id, item.project_id],
            )?;
            tx.execute("INSERT INTO profile_mods(profile_id,project_id,slug,version_id,filename,hash,enabled) VALUES(?1,?2,?3,?4,?5,?6,1) ON CONFLICT(profile_id,filename) DO UPDATE SET project_id=excluded.project_id,slug=excluded.slug,version_id=excluded.version_id,hash=excluded.hash,enabled=1", params![item.profile_id,item.project_id,item.slug,item.version_id,item.filename,item.hash])?;
        }
        if let Some(first) = mods.first() {
            insert_activity(
                &tx,
                "mods-installed",
                "Mods installed",
                &format!("{} mod file(s)", mods.len()),
                Some(&first.profile_id),
            )?;
        }
        tx.commit()
    }

    pub fn set_mod_enabled(
        &self,
        profile_id: &str,
        filename: &str,
        enabled: bool,
    ) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let changed = db.execute(
            "UPDATE profile_mods SET enabled=?1 WHERE profile_id=?2 AND filename=?3",
            params![enabled as i64, profile_id, filename],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "Managed mod not found".into(),
            ));
        }
        Ok(())
    }

    pub fn forget_managed_mod(&self, profile_id: &str, filename: &str) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        db.execute(
            "DELETE FROM profile_mods WHERE profile_id=?1 AND filename=?2",
            params![profile_id, filename],
        )?;
        Ok(())
    }

    pub fn profile_exists(&self, id: &str) -> rusqlite::Result<bool> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM profiles WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
    }

    /// Add the two launcher-owned SNine profiles on first startup without changing
    /// profiles that already exist or have been customized by the user.
    pub fn ensure_default_snine_profiles(&self) -> rusqlite::Result<()> {
        let mut db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        let now = unix_now();
        tx.execute(
            "INSERT OR IGNORE INTO profiles(id,name,client_version,minecraft_version,installed,created_at,favorite,hidden,client_ready,loader_type,loader_version,snineclient_enabled,icon_id,memory_mb,updated_at,snineclient_version,group_name) VALUES('snineclient-default','SNineClient','1.1.5','1.21.11',0,?1,0,0,0,'fabric','0.19.5',1,NULL,NULL,?1,'1.1.5','')",
            [now],
        )?;
        // Upgrade only the untouched placeholder created by older launcher builds.
        tx.execute(
            "UPDATE profiles SET client_version='1.1.5',minecraft_version='1.21.11',loader_type='fabric',loader_version='0.19.5',snineclient_enabled=1,snineclient_version='1.1.5',updated_at=?1 WHERE id='snineclient-default' AND minecraft_version='Latest supported'",
            [now],
        )?;
        tx.execute(
            "UPDATE profiles SET loader_version='0.19.5' WHERE id='snineclient-default' AND minecraft_version='1.21.11' AND loader_type='fabric' AND snineclient_enabled=1 AND (loader_version IS NULL OR loader_version='')",
            [],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO profiles(id,name,client_version,minecraft_version,installed,created_at,favorite,hidden,client_ready,loader_type,loader_version,snineclient_enabled,icon_id,memory_mb,updated_at,snineclient_version,group_name) VALUES('snineclient-26-1-2','SNineClient 26.1.2','1.1.5','26.1.2',0,?1,0,0,0,'fabric','0.19.5',1,NULL,NULL,?1,'1.1.5','')",
            [now.saturating_add(1)],
        )?;
        tx.execute(
            "UPDATE settings SET value='snineclient-default' WHERE key='selected_profile_id' AND value=''",
            [],
        )?;
        tx.commit()
    }

    /// Add the next two SNC ports as ready-to-install library profiles.
    /// Existing profile rows are left untouched so saved profile preferences survive upgrades.
    pub fn ensure_default_port_profiles(&self) -> rusqlite::Result<()> {
        let mut db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        let now = unix_now();
        tx.execute(
            "INSERT OR IGNORE INTO profiles(id,name,client_version,minecraft_version,installed,created_at,favorite,hidden,client_ready,loader_type,loader_version,snineclient_enabled,icon_id,memory_mb,updated_at,snineclient_version,group_name) VALUES('snineclient-26-3','SNineClient 26.3','1.1.5','26.3',0,?1,0,0,0,'fabric','0.19.5',1,NULL,NULL,?1,'1.1.5','')",
            [now.saturating_add(3)],
        )?;
        tx.commit()
    }

    pub fn minecraft_memory_mb(&self) -> rusqlite::Result<u32> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let value: String = db.query_row(
            "SELECT value FROM settings WHERE key='minecraft_memory_mb'",
            [],
            |row| row.get(0),
        )?;
        Ok(value.parse::<u32>().unwrap_or(4096).clamp(2048, 32768))
    }

    pub fn set_minecraft_memory_mb(&self, megabytes: u32) -> rusqlite::Result<()> {
        if !(2048..=32768).contains(&megabytes) || !megabytes.is_multiple_of(512) {
            return Err(rusqlite::Error::InvalidParameterName(
                "Minecraft memory must be 2048–32768 MB in 512 MB increments".into(),
            ));
        }
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        db.execute("INSERT INTO settings(key,value) VALUES('minecraft_memory_mb',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [megabytes.to_string()])?;
        Ok(())
    }

    pub fn profile_memory_mb(&self, id: &str) -> rusqlite::Result<Option<u32>> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        db.query_row("SELECT memory_mb FROM profiles WHERE id=?1", [id], |row| {
            row.get(0)
        })
    }

    pub fn effective_profile_memory_mb(&self, id: &str) -> rusqlite::Result<u32> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let profile_memory: Option<u32> =
            db.query_row("SELECT memory_mb FROM profiles WHERE id=?1", [id], |row| {
                row.get(0)
            })?;
        let global_memory: String = db.query_row(
            "SELECT value FROM settings WHERE key='minecraft_memory_mb'",
            [],
            |row| row.get(0),
        )?;
        Ok(profile_memory
            .unwrap_or_else(|| global_memory.parse::<u32>().unwrap_or(4096))
            .clamp(2048, 32768))
    }

    pub fn set_profile_memory_mb(&self, id: &str, megabytes: Option<u32>) -> rusqlite::Result<()> {
        if megabytes
            .is_some_and(|value| !(2048..=32768).contains(&value) || !value.is_multiple_of(512))
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "Profile memory must be 2048–32768 MB in 512 MB increments".into(),
            ));
        }
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        if db.execute(
            "UPDATE profiles SET memory_mb=?1,updated_at=?2 WHERE id=?3",
            params![megabytes, unix_now(), id],
        )? == 0
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        Ok(())
    }

    pub fn download_concurrency(&self) -> rusqlite::Result<u32> {
        self.setting_u32("download_concurrency", 4, 1, 16)
    }

    pub fn set_download_concurrency(&self, value: u32) -> rusqlite::Result<()> {
        if !(1..=16).contains(&value) {
            return Err(rusqlite::Error::InvalidParameterName(
                "Download concurrency must be between 1 and 16".into(),
            ));
        }
        self.set_setting("download_concurrency", &value.to_string())
    }

    pub fn minimize_on_game_start(&self) -> rusqlite::Result<bool> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let value: String = db.query_row(
            "SELECT value FROM settings WHERE key='minimize_on_game_start'",
            [],
            |row| row.get(0),
        )?;
        Ok(value == "1")
    }

    pub fn set_minimize_on_game_start(&self, minimize: bool) -> rusqlite::Result<()> {
        self.set_setting("minimize_on_game_start", if minimize { "1" } else { "0" })
    }

    pub fn minecraft_window_settings(&self) -> rusqlite::Result<MinecraftWindowSettings> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let read = |key: &str, default: u32| -> rusqlite::Result<u32> {
            let value: String =
                db.query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                    row.get(0)
                })?;
            Ok(value.parse::<u32>().unwrap_or(default))
        };
        let fullscreen: String = db.query_row(
            "SELECT value FROM settings WHERE key='minecraft_window_fullscreen'",
            [],
            |row| row.get(0),
        )?;
        Ok(MinecraftWindowSettings {
            width: read("minecraft_window_width", 1280)?.clamp(640, 7680),
            height: read("minecraft_window_height", 720)?.clamp(480, 4320),
            fullscreen: fullscreen == "1",
        })
    }

    pub fn set_minecraft_window_settings(
        &self,
        value: MinecraftWindowSettings,
    ) -> rusqlite::Result<()> {
        if !(640..=7680).contains(&value.width) || !(480..=4320).contains(&value.height) {
            return Err(rusqlite::Error::InvalidParameterName(
                "Minecraft resolution must be between 640×480 and 7680×4320".into(),
            ));
        }
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        db.execute("INSERT INTO settings(key,value) VALUES('minecraft_window_width',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value.width.to_string()])?;
        db.execute("INSERT INTO settings(key,value) VALUES('minecraft_window_height',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [value.height.to_string()])?;
        db.execute("INSERT INTO settings(key,value) VALUES('minecraft_window_fullscreen',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [if value.fullscreen { "1" } else { "0" }])?;
        Ok(())
    }

    fn setting_u32(&self, key: &str, default: u32, min: u32, max: u32) -> rusqlite::Result<u32> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let value: String =
            db.query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })?;
        Ok(value.parse::<u32>().unwrap_or(default).clamp(min, max))
    }

    fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        db.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
        Ok(())
    }

    pub fn mark_client_installed(
        &self,
        id: &str,
        version: &str,
        minecraft_version: &str,
    ) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let changed = db.execute(
            "UPDATE profiles SET client_ready=1,client_version=?1,minecraft_version=?2 WHERE id=?3",
            params![version, minecraft_version, id],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        let name: String =
            db.query_row("SELECT name FROM profiles WHERE id=?1", [id], |r| r.get(0))?;
        insert_activity(
            &db,
            "client-installed",
            "SNine client installed",
            &name,
            Some(id),
        )?;
        Ok(())
    }

    pub fn set_profile_installed(&self, id: &str, installed: bool) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        if db.execute(
            "UPDATE profiles SET installed=?1 WHERE id=?2",
            params![installed as i64, id],
        )? == 0
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        let name: String = db.query_row("SELECT name FROM profiles WHERE id=?1", [id], |row| {
            row.get(0)
        })?;
        insert_activity(
            &db,
            if installed {
                "profile-ready"
            } else {
                "profile-uninstalled"
            },
            if installed {
                "Minecraft profile ready"
            } else {
                "Minecraft profile removed"
            },
            &name,
            Some(id),
        )?;
        Ok(())
    }

    pub fn set_profile_loader_version(&self, id: &str, version: &str) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        if db.execute(
            "UPDATE profiles SET loader_version=?1 WHERE id=?2",
            params![version, id],
        )? == 0
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        Ok(())
    }

    pub fn mark_profile_uninstalled(&self, id: &str) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let changed = db.execute(
            "UPDATE profiles SET installed=0,client_ready=0 WHERE id=?1",
            [id],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        let name: String = db.query_row("SELECT name FROM profiles WHERE id=?1", [id], |row| {
            row.get(0)
        })?;
        insert_activity(
            &db,
            "profile-uninstalled",
            "Profile uninstalled",
            &name,
            Some(id),
        )
    }

    pub fn record_profile_verified(&self, id: &str, detail: &str) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let name: String = db.query_row("SELECT name FROM profiles WHERE id=?1", [id], |row| {
            row.get(0)
        })?;
        insert_activity(
            &db,
            "profile-verified",
            "Profile verified",
            &format!("{name} · {detail}"),
            Some(id),
        )
    }

    pub fn record_launch_started(&self, id: &str, username: &str) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let name: String = db.query_row("SELECT name FROM profiles WHERE id=?1", [id], |row| {
            row.get(0)
        })?;
        let timestamp = unix_now();
        db.execute(
            "INSERT INTO recent_profiles(profile_id,used_at) VALUES(?1,?2) ON CONFLICT(profile_id) DO UPDATE SET used_at=excluded.used_at",
            params![id, timestamp],
        )?;
        insert_activity(
            &db,
            "game-started",
            "Started Minecraft",
            &format!("{name} · {username}"),
            Some(id),
        )
    }

    pub fn record_launch_ended(
        &self,
        id: &str,
        detail: &str,
        crashed: bool,
    ) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let name: String = db.query_row("SELECT name FROM profiles WHERE id=?1", [id], |row| {
            row.get(0)
        })?;
        insert_activity(
            &db,
            if crashed {
                "game-crashed"
            } else {
                "game-exited"
            },
            if crashed {
                "Minecraft closed unexpectedly"
            } else {
                "Minecraft exited"
            },
            &format!("{name} · {detail}"),
            Some(id),
        )
    }

    pub fn save_account(
        &self,
        uuid: &str,
        username: &str,
        authenticated_at: i64,
    ) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        db.execute("INSERT INTO saved_accounts(uuid,username,last_authenticated_at) VALUES(?1,?2,?3) ON CONFLICT(uuid) DO UPDATE SET username=excluded.username,last_authenticated_at=excluded.last_authenticated_at", params![uuid, username, authenticated_at])?;
        db.execute("INSERT INTO auth_account(singleton,uuid,username,last_authenticated_at) VALUES(1,?1,?2,?3) ON CONFLICT(singleton) DO UPDATE SET uuid=excluded.uuid,username=excluded.username,last_authenticated_at=excluded.last_authenticated_at", params![uuid, username, authenticated_at])?;
        insert_activity(&db, "auth-signed-in", "Signed in", username, None)?;
        Ok(())
    }

    pub fn safe_account(&self) -> rusqlite::Result<Option<SafeAccount>> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        db.query_row(
            "SELECT uuid,username,last_authenticated_at FROM auth_account WHERE singleton=1",
            [],
            |row| {
                Ok(SafeAccount {
                    uuid: row.get(0)?,
                    username: row.get(1)?,
                    last_authenticated_at: row.get(2)?,
                })
            },
        )
        .optional()
    }

    pub fn safe_accounts(&self) -> rusqlite::Result<Vec<SafeAccount>> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let mut statement = db.prepare("SELECT uuid,username,last_authenticated_at FROM saved_accounts ORDER BY last_authenticated_at DESC, username COLLATE NOCASE")?;
        let rows = statement.query_map([], |row| {
            Ok(SafeAccount {
                uuid: row.get(0)?,
                username: row.get(1)?,
                last_authenticated_at: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn select_account(&self, uuid: &str) -> rusqlite::Result<Option<SafeAccount>> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let account = db
            .query_row(
                "SELECT uuid,username,last_authenticated_at FROM saved_accounts WHERE uuid=?1",
                [uuid],
                |row| {
                    Ok(SafeAccount {
                        uuid: row.get(0)?,
                        username: row.get(1)?,
                        last_authenticated_at: row.get(2)?,
                    })
                },
            )
            .optional()?;
        if let Some(account) = account {
            db.execute("INSERT INTO auth_account(singleton,uuid,username,last_authenticated_at) VALUES(1,?1,?2,?3) ON CONFLICT(singleton) DO UPDATE SET uuid=excluded.uuid,username=excluded.username,last_authenticated_at=excluded.last_authenticated_at", params![account.uuid, account.username, account.last_authenticated_at])?;
            Ok(Some(account))
        } else {
            Ok(None)
        }
    }

    pub fn remove_saved_account(&self, uuid: &str) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let username: Option<String> = db
            .query_row(
                "SELECT username FROM saved_accounts WHERE uuid=?1",
                [uuid],
                |row| row.get(0),
            )
            .optional()?;
        db.execute("DELETE FROM saved_accounts WHERE uuid=?1", [uuid])?;
        db.execute(
            "DELETE FROM auth_account WHERE singleton=1 AND uuid=?1",
            [uuid],
        )?;
        if let Some(username) = username {
            insert_activity(&db, "auth-signed-out", "Signed out", &username, None)?;
        }
        Ok(())
    }

    pub fn clear_account(&self) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        if let Some(uuid) = db
            .query_row(
                "SELECT uuid FROM auth_account WHERE singleton=1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            db.execute("DELETE FROM saved_accounts WHERE uuid=?1", [uuid])?;
        }
        db.execute("DELETE FROM auth_account WHERE singleton=1", [])?;
        insert_activity(
            &db,
            "auth-signed-out",
            "Signed out",
            "Minecraft account",
            None,
        )?;
        Ok(())
    }

    pub fn select_profile(&self, id: String) -> rusqlite::Result<()> {
        let mut db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        let selected: String = tx.query_row(
            "SELECT value FROM settings WHERE key='selected_profile_id'",
            [],
            |r| r.get(0),
        )?;
        let found: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM profiles WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )?;
        if !found {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        tx.execute(
            "UPDATE settings SET value=?1 WHERE key='selected_profile_id'",
            [&id],
        )?;
        let timestamp = unix_now();
        tx.execute("INSERT INTO recent_profiles(profile_id,used_at) VALUES(?1,?2) ON CONFLICT(profile_id) DO UPDATE SET used_at=excluded.used_at", params![id, timestamp])?;
        tx.execute("DELETE FROM recent_profiles WHERE profile_id NOT IN (SELECT profile_id FROM recent_profiles ORDER BY used_at DESC LIMIT 8)", [])?;
        if selected != id {
            let name: String =
                tx.query_row("SELECT name FROM profiles WHERE id=?1", [&id], |row| {
                    row.get(0)
                })?;
            insert_activity(&tx, "profile-selected", "Profile opened", &name, Some(&id))?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_profile_flag(
        &self,
        id: String,
        flag: ProfileFlag,
        value: bool,
    ) -> rusqlite::Result<()> {
        let db = self.0.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let column = match flag {
            ProfileFlag::Favorite => "favorite",
            ProfileFlag::Hidden => "hidden",
        };
        let existing: Option<i64> = db
            .query_row(
                &format!("SELECT {column} FROM profiles WHERE id=?1"),
                [&id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(existing) = existing else {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        };
        if (existing != 0) == value {
            return Ok(());
        }
        let sql = format!("UPDATE profiles SET {column}=?1 WHERE id=?2");
        if db.execute(&sql, params![value as i64, id])? == 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "Unknown profile".into(),
            ));
        }
        let name: String = db.query_row("SELECT name FROM profiles WHERE id=?1", [&id], |row| {
            row.get(0)
        })?;
        let (kind, title) = match flag {
            ProfileFlag::Favorite if value => ("favorite-added", "Added to favorites"),
            ProfileFlag::Favorite => ("favorite-removed", "Removed from favorites"),
            ProfileFlag::Hidden if value => ("profile-hidden", "Profile hidden"),
            ProfileFlag::Hidden => ("profile-shown", "Profile restored"),
        };
        insert_activity(&db, kind, title, &name, Some(&id))?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub enum ProfileFlag {
    Favorite,
    Hidden,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn insert_activity(
    db: &Connection,
    kind: &str,
    title: &str,
    detail: &str,
    profile_id: Option<&str>,
) -> rusqlite::Result<()> {
    db.execute("INSERT INTO activity(id,kind,title,detail,profile_id,occurred_at) VALUES(?1,?2,?3,?4,?5,?6)", params![uuid::Uuid::new_v4().to_string(), kind, title, detail, profile_id, unix_now()])?;
    db.execute("DELETE FROM activity WHERE id NOT IN (SELECT id FROM activity ORDER BY occurred_at DESC,rowid DESC LIMIT 100)", [])?;
    Ok(())
}

fn migrate_schema(connection: &mut Connection) -> rusqlite::Result<()> {
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 2 {
        let tx = connection.transaction()?;
        tx.execute_batch("ALTER TABLE profiles ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0; ALTER TABLE profiles ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0; UPDATE metadata SET value='2' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 3 {
        let tx = connection.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS activity (id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL, detail TEXT NOT NULL, profile_id TEXT REFERENCES profiles(id) ON DELETE SET NULL, occurred_at INTEGER NOT NULL); UPDATE metadata SET value='3' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 4 {
        let tx = connection.transaction()?;
        tx.execute_batch("ALTER TABLE profiles ADD COLUMN client_ready INTEGER NOT NULL DEFAULT 0; UPDATE metadata SET value='4' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 5 {
        let tx = connection.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS auth_account (singleton INTEGER PRIMARY KEY CHECK(singleton=1), uuid TEXT NOT NULL, username TEXT NOT NULL, last_authenticated_at INTEGER NOT NULL); UPDATE metadata SET value='5' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 6 {
        let tx = connection.transaction()?;
        tx.execute_batch("ALTER TABLE profiles ADD COLUMN loader_type TEXT NOT NULL DEFAULT 'fabric'; ALTER TABLE profiles ADD COLUMN loader_version TEXT; ALTER TABLE profiles ADD COLUMN snineclient_enabled INTEGER NOT NULL DEFAULT 1; ALTER TABLE profiles ADD COLUMN icon_id TEXT; ALTER TABLE profiles ADD COLUMN memory_mb INTEGER; ALTER TABLE profiles ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0; UPDATE profiles SET updated_at=created_at; UPDATE metadata SET value='6' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 7 {
        let tx = connection.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS profile_mods (profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, project_id TEXT NOT NULL, slug TEXT NOT NULL, version_id TEXT NOT NULL, filename TEXT NOT NULL, hash TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, PRIMARY KEY(profile_id,filename)); UPDATE metadata SET value='7' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 8 {
        let tx = connection.transaction()?;
        tx.execute_batch("ALTER TABLE profiles ADD COLUMN snineclient_version TEXT; UPDATE profiles SET snineclient_version=client_version WHERE snineclient_enabled=1; UPDATE metadata SET value='8' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 9 {
        let tx = connection.transaction()?;
        tx.execute_batch("ALTER TABLE profiles ADD COLUMN group_name TEXT NOT NULL DEFAULT ''; UPDATE metadata SET value='9' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    let version: String = connection.query_row(
        "SELECT value FROM metadata WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    if version.parse::<u32>().unwrap_or(1) < 10 {
        let tx = connection.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS saved_accounts (uuid TEXT PRIMARY KEY, username TEXT NOT NULL, last_authenticated_at INTEGER NOT NULL); INSERT OR IGNORE INTO saved_accounts(uuid,username,last_authenticated_at) SELECT uuid,username,last_authenticated_at FROM auth_account WHERE singleton=1; UPDATE metadata SET value='10' WHERE key='schema_version';")?;
        tx.commit()?;
    }
    Ok(())
}

fn validate_profile_name(name: String) -> rusqlite::Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 48 || name.chars().any(char::is_control) {
        return Err(rusqlite::Error::InvalidParameterName(
            "Profile name must be 1–48 printable characters".into(),
        ));
    }
    Ok(name)
}

fn validate_minecraft_version(value: String) -> rusqlite::Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "Invalid Minecraft version id".into(),
        ));
    }
    Ok(value.to_owned())
}

fn supports_snineclient(minecraft_version: &str, loader_type: LoaderType) -> bool {
    loader_type == LoaderType::Fabric && matches!(minecraft_version, "1.21.11" | "26.1.2" | "26.3")
}

fn valid_icon_id(value: &str) -> bool {
    let Some(path) = value.strip_prefix("minecraft:") else {
        return false;
    };
    !path.is_empty()
        && path.len() <= 96
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'_'
                        || byte == b'-'
                })
        })
}

fn load_profile(db: &Connection, id: &str) -> rusqlite::Result<Profile> {
    let selected: String = db.query_row(
        "SELECT value FROM settings WHERE key='selected_profile_id'",
        [],
        |row| row.get(0),
    )?;
    db.query_row("SELECT id,name,client_version,minecraft_version,installed,favorite,hidden,client_ready,loader_type,loader_version,snineclient_enabled,icon_id,memory_mb,snineclient_version,group_name FROM profiles WHERE id=?1", [id], |row| {
        Ok(Profile {
            id: row.get(0)?, name: row.get(1)?, group_name: row.get(14)?, client_version: row.get(2)?, minecraft_version: row.get(3)?,
            installed: row.get::<_, i64>(4)? != 0, favorite: row.get::<_, i64>(5)? != 0, hidden: row.get::<_, i64>(6)? != 0,
            client_ready: row.get::<_, i64>(7)? != 0, loader_type: LoaderType::parse(&row.get::<_, String>(8)?), loader_version: row.get(9)?,
            snineclient_enabled: row.get::<_, i64>(10)? != 0, icon_id: row.get(11)?, memory_mb: row.get(12)?, selected: row.get::<_, String>(0)? == selected, snineclient_version: row.get(13)?,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{migrate_schema, Database, MinecraftWindowSettings};
    use rusqlite::Connection;

    #[test]
    fn schema_bootstraps_default_profile_and_persists_new_profiles() {
        let db = Database::open(":memory:").expect("database");
        let before = db.bootstrap().expect("bootstrap");
        assert_eq!(before.schema_version, 10);
        assert_eq!(before.profiles.len(), 1);
        assert_eq!(
            before.selected_profile_id.as_deref(),
            Some("snineclient-default")
        );
        assert!(before.profiles[0].selected);
        assert!(!before.profiles[0].favorite);
        assert!(!before.profiles[0].hidden);

        let profile = db
            .create_profile("  SNineClient  ".into())
            .expect("profile");
        assert_eq!(profile.name, "SNineClient");
        assert!(profile.selected);
        let after = db.bootstrap().expect("bootstrap after create");
        assert_eq!(
            after.selected_profile_id.as_deref(),
            Some(profile.id.as_str())
        );
        assert_eq!(after.profiles.len(), 2);
        assert_eq!(after.recent_profile_ids.first(), Some(&profile.id));
        db.set_profile_flag(profile.id.clone(), super::ProfileFlag::Favorite, true)
            .unwrap();
        db.set_profile_flag(profile.id.clone(), super::ProfileFlag::Hidden, true)
            .unwrap();
        let updated = db
            .bootstrap()
            .unwrap()
            .profiles
            .into_iter()
            .find(|item| item.id == profile.id)
            .unwrap();
        assert!(updated.favorite && updated.hidden);
    }

    #[test]
    fn default_snine_profiles_include_both_supported_minecraft_builds_and_are_idempotent() {
        let db = Database::open(":memory:").unwrap();
        db.ensure_default_snine_profiles().unwrap();
        db.ensure_default_snine_profiles().unwrap();
        let profiles = db.bootstrap().unwrap().profiles;
        assert_eq!(profiles.len(), 2);
        let classic = profiles
            .iter()
            .find(|profile| profile.id == "snineclient-default")
            .unwrap();
        assert_eq!(classic.minecraft_version, "1.21.11");
        assert_eq!(classic.loader_type, super::LoaderType::Fabric);
        assert!(classic.snineclient_enabled);
        let current = profiles
            .iter()
            .find(|profile| profile.id == "snineclient-26-1-2")
            .unwrap();
        assert_eq!(current.name, "SNineClient 26.1.2");
        assert_eq!(current.minecraft_version, "26.1.2");
        assert_eq!(current.loader_version.as_deref(), Some("0.19.5"));
        assert!(current.snineclient_enabled);
        assert!(!current.installed);
    }

    #[test]
    fn snineclient_configuration_accepts_only_available_fabric_builds() {
        let db = Database::open(":memory:").unwrap();
        for version in ["1.21.11", "26.1.2", "26.3"] {
            assert!(db
                .create_profile_config(super::NewProfileConfig {
                    name: format!("SNine {version}"),
                    minecraft_version: version.into(),
                    loader_type: super::LoaderType::Fabric,
                    loader_version: Some("0.19.5".into()),
                    snineclient_enabled: true,
                    memory_mb: None,
                    icon_id: None,
                })
                .is_ok());
        }
        assert!(db
            .create_profile_config(super::NewProfileConfig {
                name: "SNine 26.2".into(),
                minecraft_version: "26.2".into(),
                loader_type: super::LoaderType::Fabric,
                loader_version: Some("0.19.5".into()),
                snineclient_enabled: true,
                memory_mb: None,
                icon_id: None,
            })
            .is_err());
        assert!(db
            .create_profile_config(super::NewProfileConfig {
                name: "Unsupported SNine".into(),
                minecraft_version: "26.1.2".into(),
                loader_type: super::LoaderType::Vanilla,
                loader_version: None,
                snineclient_enabled: true,
                memory_mb: None,
                icon_id: None,
            })
            .is_err());
    }

    #[test]
    fn profile_configuration_accepts_vanilla_and_fabric_without_snine_and_rejects_invalid_pairing()
    {
        let db = Database::open(":memory:").unwrap();
        let vanilla = db
            .create_profile_config(super::NewProfileConfig {
                name: "Vanilla test".into(),
                minecraft_version: "1.21.11".into(),
                loader_type: super::LoaderType::Vanilla,
                loader_version: None,
                snineclient_enabled: false,
                memory_mb: Some(4096),
                icon_id: Some("minecraft:stone".into()),
            })
            .unwrap();
        assert_eq!(vanilla.loader_type, super::LoaderType::Vanilla);
        assert!(!vanilla.snineclient_enabled);
        assert_eq!(vanilla.minecraft_version, "1.21.11");
        assert_eq!(vanilla.icon_id.as_deref(), Some("minecraft:stone"));
        assert_eq!(vanilla.memory_mb, Some(4096));
        assert_eq!(vanilla.client_version, "Vanilla");
        let fabric = db
            .create_profile_config(super::NewProfileConfig {
                name: "Fabric no client".into(),
                minecraft_version: "1.21.11".into(),
                loader_type: super::LoaderType::Fabric,
                loader_version: Some("0.19.5".into()),
                snineclient_enabled: false,
                memory_mb: None,
                icon_id: None,
            })
            .unwrap();
        assert_eq!(fabric.loader_version.as_deref(), Some("0.19.5"));
        assert!(!fabric.snineclient_enabled);
        assert_eq!(fabric.client_version, "Vanilla");
        let persisted = db.bootstrap().unwrap().profiles;
        assert!(persisted.iter().any(|profile| profile.id == vanilla.id
            && profile.client_version == "Vanilla"
            && profile.loader_type == super::LoaderType::Vanilla));
        assert!(persisted.iter().any(|profile| profile.id == fabric.id
            && profile.client_version == "Vanilla"
            && profile.loader_type == super::LoaderType::Fabric
            && !profile.snineclient_enabled));
        assert!(db
            .create_profile_config(super::NewProfileConfig {
                name: "Invalid SNine".into(),
                minecraft_version: "1.20.1".into(),
                loader_type: super::LoaderType::Fabric,
                loader_version: None,
                snineclient_enabled: true,
                memory_mb: None,
                icon_id: None
            })
            .is_err());
        assert!(db
            .create_profile_config(super::NewProfileConfig {
                name: "Invalid Vanilla".into(),
                minecraft_version: "1.21.11".into(),
                loader_type: super::LoaderType::Vanilla,
                loader_version: None,
                snineclient_enabled: true,
                memory_mb: None,
                icon_id: None
            })
            .is_err());
    }

    #[test]
    fn rejects_invalid_profile_name_and_unknown_selection() {
        let db = Database::open(":memory:").expect("database");
        assert!(db.create_profile("  ".into()).is_err());
        assert!(db.create_profile("x".repeat(49)).is_err());
        assert!(db.select_profile("missing".into()).is_err());
    }

    #[test]
    fn profile_management_renames_duplicates_and_persists_safe_icon_identity() {
        let db = Database::open(":memory:").unwrap();
        let original = db.create_profile("  Test Profile  ".into()).unwrap();
        let renamed = db
            .rename_profile(&original.id, "  Renamed  ".into())
            .unwrap();
        assert_eq!(renamed.name, "Renamed");
        db.set_profile_group(&original.id, "  QA pack  ".into())
            .unwrap();
        db.set_profile_icon(&original.id, Some("minecraft:diamond_block".into()))
            .unwrap();
        assert!(db
            .set_profile_icon(&original.id, Some("minecraft:../escape".into()))
            .is_err());
        let duplicate = db
            .duplicate_profile(&original.id, "Copy Profile".into())
            .unwrap();
        assert_ne!(duplicate.id, original.id);
        assert_eq!(duplicate.name, "Copy Profile");
        assert_eq!(duplicate.group_name, "QA pack");
        assert!(!duplicate.installed);
        let loaded = db.bootstrap().unwrap();
        let stored = loaded
            .profiles
            .iter()
            .find(|profile| profile.id == original.id)
            .unwrap();
        assert_eq!(stored.name, "Renamed");
        assert_eq!(stored.group_name, "QA pack");
        assert_eq!(stored.icon_id.as_deref(), Some("minecraft:diamond_block"));
        assert_eq!(stored.loader_type, super::LoaderType::Fabric);
    }

    #[test]
    fn profile_delete_only_removes_uninstalled_non_primary_profiles() {
        let db = Database::open(":memory:").unwrap();
        let profile = db.create_profile("Temporary".into()).unwrap();
        assert!(db.delete_empty_profile("snineclient-default").is_err());
        assert!(db.delete_empty_profile(&profile.id).is_ok());
        assert!(!db
            .bootstrap()
            .unwrap()
            .profiles
            .iter()
            .any(|item| item.id == profile.id));
        let protected = db.create_profile("Installed profile".into()).unwrap();
        db.set_profile_installed(&protected.id, true).unwrap();
        assert!(db.delete_empty_profile(&protected.id).is_err());
    }

    #[test]
    fn managed_mod_records_roundtrip_and_disable_state() {
        let db = Database::open(":memory:").unwrap();
        let profile = db.create_profile("Mod profile".into()).unwrap();
        let record = super::ManagedMod {
            profile_id: profile.id.clone(),
            mod_id: "sodium".into(),
            project_id: "project-id".into(),
            slug: "sodium".into(),
            version_id: "version-id".into(),
            filename: "sodium.jar".into(),
            hash: "a".repeat(128),
            enabled: true,
            managed: true,
            icon_data_url: None,
        };
        db.record_managed_mods(std::slice::from_ref(&record))
            .unwrap();
        assert_eq!(db.managed_mods(&profile.id).unwrap().len(), 1);
        db.set_mod_enabled(&profile.id, "sodium.jar", false)
            .unwrap();
        assert!(!db.managed_mods(&profile.id).unwrap()[0].enabled);
        db.forget_managed_mod(&profile.id, "sodium.jar").unwrap();
        assert!(db.managed_mods(&profile.id).unwrap().is_empty());
    }

    #[test]
    fn minecraft_memory_setting_is_bounded_and_persistent_in_database() {
        let db = Database::open(":memory:").unwrap();
        assert_eq!(db.minecraft_memory_mb().unwrap(), 4096);
        assert_eq!(
            db.effective_profile_memory_mb("snineclient-default")
                .unwrap(),
            4096
        );
        db.set_minecraft_memory_mb(6144).unwrap();
        assert_eq!(db.minecraft_memory_mb().unwrap(), 6144);
        let profile = db.create_profile("Custom memory".into()).unwrap();
        db.set_profile_memory_mb(&profile.id, Some(8192)).unwrap();
        assert_eq!(db.effective_profile_memory_mb(&profile.id).unwrap(), 8192);
        db.set_profile_memory_mb(&profile.id, None).unwrap();
        assert_eq!(db.effective_profile_memory_mb(&profile.id).unwrap(), 6144);
        assert!(db.set_profile_memory_mb(&profile.id, Some(4097)).is_err());
        assert!(db.set_minecraft_memory_mb(1024).is_err());
        assert!(db.set_minecraft_memory_mb(4097).is_err());
        assert!(db.set_minecraft_memory_mb(65536).is_err());
    }

    #[test]
    fn profile_creation_persists_optional_memory_override() {
        let db = Database::open(":memory:").unwrap();
        let profile = db
            .create_profile_with_memory("Custom launch profile".into(), Some(8192))
            .unwrap();
        assert_eq!(profile.memory_mb, Some(8192));
        assert_eq!(db.profile_memory_mb(&profile.id).unwrap(), Some(8192));
        assert!(db
            .create_profile_with_memory("Invalid memory".into(), Some(4097))
            .is_err());
    }

    #[test]
    fn launch_behavior_and_download_concurrency_are_persistent_and_bounded() {
        let db = Database::open(":memory:").unwrap();
        assert!(!db.minimize_on_game_start().unwrap());
        db.set_minimize_on_game_start(true).unwrap();
        assert!(db.minimize_on_game_start().unwrap());
        assert_eq!(db.download_concurrency().unwrap(), 4);
        db.set_download_concurrency(12).unwrap();
        assert_eq!(db.download_concurrency().unwrap(), 12);
        assert!(db.set_download_concurrency(0).is_err());
        assert!(db.set_download_concurrency(17).is_err());
        let window = db.minecraft_window_settings().unwrap();
        assert_eq!((window.width, window.height), (1280, 720));
        db.set_minecraft_window_settings(MinecraftWindowSettings {
            width: 1920,
            height: 1080,
            fullscreen: true,
        })
        .unwrap();
        let window = db.minecraft_window_settings().unwrap();
        assert_eq!(
            (window.width, window.height, window.fullscreen),
            (1920, 1080, true)
        );
        assert!(db
            .set_minecraft_window_settings(MinecraftWindowSettings {
                width: 300,
                height: 500,
                fullscreen: false
            })
            .is_err());
    }

    #[test]
    fn process_lifecycle_persists_recent_launch_and_exit_activity() {
        let db = Database::open(":memory:").unwrap();
        let profile = db.create_profile("Lifecycle fixture".into()).unwrap();
        db.record_launch_started(&profile.id, "FixturePlayer")
            .unwrap();
        let recent = db.bootstrap().unwrap().recent_profile_ids;
        assert_eq!(recent.first(), Some(&profile.id));
        db.record_launch_ended(
            &profile.id,
            "FixturePlayer exited Minecraft normally",
            false,
        )
        .unwrap();
        let activity = db.bootstrap().unwrap().activity;
        assert!(activity
            .iter()
            .any(|entry| entry.title == "Started Minecraft"));
        assert!(activity
            .iter()
            .any(|entry| entry.title == "Minecraft exited" && entry.detail.contains("normally")));
        db.record_launch_ended(
            &profile.id,
            "FixturePlayer process exited with code 23",
            true,
        )
        .unwrap();
        let activity = db.bootstrap().unwrap().activity;
        assert!(activity
            .iter()
            .any(|entry| entry.title == "Minecraft closed unexpectedly"
                && entry.kind == "game-crashed"));
    }

    #[test]
    fn migration_v1_to_v2_preserves_profiles_and_adds_default_flags() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE profiles (id TEXT PRIMARY KEY, name TEXT NOT NULL, client_version TEXT NOT NULL, minecraft_version TEXT NOT NULL, installed INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL); INSERT INTO metadata VALUES('schema_version','1'); INSERT INTO profiles VALUES('legacy-id','Legacy profile','1.0','1.21.11',1,42);").unwrap();
        migrate_schema(&mut connection).unwrap();
        let row: (String, String, i64, i64) = connection
            .query_row("SELECT id,name,favorite,hidden FROM profiles", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!(row, ("legacy-id".into(), "Legacy profile".into(), 0, 0));
        let version: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "9");
        let group_name: String = connection
            .query_row(
                "SELECT group_name FROM profiles WHERE id='legacy-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(group_name.is_empty());
        let migrated_snine_version: String = connection
            .query_row(
                "SELECT snineclient_version FROM profiles WHERE id='legacy-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migrated_snine_version, "1.0");
    }

    #[test]
    fn profile_flags_and_recent_selection_survive_database_reopen() {
        let path =
            std::env::temp_dir().join(format!("snine-launcher-{}.sqlite", uuid::Uuid::new_v4()));
        let profile_id;
        {
            let db = Database::open(&path).unwrap();
            let profile = db.create_profile("Persistent profile".into()).unwrap();
            profile_id = profile.id;
            db.set_profile_flag(profile_id.clone(), super::ProfileFlag::Favorite, true)
                .unwrap();
            db.set_profile_flag(profile_id.clone(), super::ProfileFlag::Hidden, true)
                .unwrap();
            db.select_profile("snineclient-default".into()).unwrap();
            db.select_profile(profile_id.clone()).unwrap();
        }
        {
            let db = Database::open(&path).unwrap();
            let state = db.bootstrap().unwrap();
            let profile = state
                .profiles
                .iter()
                .find(|item| item.id == profile_id)
                .unwrap();
            assert!(profile.favorite && profile.hidden && profile.selected);
            assert_eq!(state.recent_profile_ids.first(), Some(&profile_id));
            assert!(state
                .activity
                .iter()
                .any(|entry| entry.kind == "favorite-added"
                    && entry.profile_id.as_deref() == Some(&profile_id)));
            assert!(state
                .activity
                .iter()
                .any(|entry| entry.kind == "profile-hidden"
                    && entry.profile_id.as_deref() == Some(&profile_id)));
            assert!(state
                .activity
                .iter()
                .any(|entry| entry.kind == "profile-selected"
                    && entry.profile_id.as_deref() == Some(&profile_id)));
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn installed_client_state_survives_database_reopen() {
        let path = std::env::temp_dir().join(format!(
            "snine-client-state-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let profile_id;
        {
            let db = Database::open(&path).unwrap();
            profile_id = "snineclient-default".to_owned();
            db.mark_client_installed(&profile_id, "release-123", "1.21.11")
                .unwrap();
        }
        {
            let db = Database::open(&path).unwrap();
            let profile = db
                .bootstrap()
                .unwrap()
                .profiles
                .into_iter()
                .find(|p| p.id == profile_id)
                .unwrap();
            assert!(profile.client_ready);
            assert_eq!(profile.client_version, "release-123");
            assert_eq!(profile.minecraft_version, "1.21.11");
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    #[test]
    fn safe_account_metadata_persists_and_logout_clears_it() {
        let db = Database::open(":memory:").unwrap();
        db.save_account("0123456789abcdef0123456789abcdef", "Player", 1234)
            .unwrap();
        let account = db.safe_account().unwrap().unwrap();
        assert_eq!(account.username, "Player");
        assert_eq!(account.last_authenticated_at, 1234);
        {
            let connection = db.0.lock().unwrap();
            let mut statement = connection
                .prepare("PRAGMA table_info(auth_account)")
                .unwrap();
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(
                columns,
                vec!["singleton", "uuid", "username", "last_authenticated_at"]
            );
            assert!(!columns
                .iter()
                .any(|column| column.contains("token") || column.contains("secret")));
        }
        db.clear_account().unwrap();
        assert!(db.safe_account().unwrap().is_none());
    }
}
