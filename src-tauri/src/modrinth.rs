use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const API: &str = "https://api.modrinth.com/v2";
const RESPONSE_LIMIT: usize = 2 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthProject {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub icon_url: String,
    pub downloads: u64,
    pub categories: Vec<String>,
    pub game_versions: Vec<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
    #[allow(dead_code)]
    total_hits: usize,
}
#[derive(Deserialize)]
struct SearchHit {
    project_id: String,
    slug: String,
    title: String,
    description: String,
    author: String,
    icon_url: Option<String>,
    downloads: u64,
    categories: Vec<String>,
    versions: Vec<String>,
    project_type: String,
}

#[derive(Deserialize)]
struct ProjectIconResponse {
    id: String,
    slug: String,
    icon_url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthProjectIcon {
    pub project_id: String,
    pub slug: String,
    pub icon_url: String,
}

struct CacheEntry {
    expires: Instant,
    projects: Vec<ModrinthProject>,
}
pub struct ModrinthService {
    client: Client,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl ModrinthService {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(8))
            .user_agent(concat!(
                "SNineLauncher/",
                env!("CARGO_PKG_VERSION"),
                " (Modrinth API client)"
            ))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub async fn search_fabric_mods(
        &self,
        query: &str,
        minecraft_version: &str,
    ) -> Result<Vec<ModrinthProject>, String> {
        self.search_projects(query, minecraft_version, "mod", Some("fabric"))
            .await
    }

    pub async fn search_resource_packs(
        &self,
        query: &str,
        minecraft_version: &str,
    ) -> Result<Vec<ModrinthProject>, String> {
        self.search_projects(query, minecraft_version, "resourcepack", None)
            .await
    }

    pub async fn project_icons(
        &self,
        project_ids: &[String],
    ) -> Result<Vec<ModrinthProjectIcon>, String> {
        let ids: Vec<String> = project_ids
            .iter()
            .filter(|id| valid_id(id))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .take(100)
            .collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut url =
            Url::parse(&format!("{API}/projects")).map_err(|_| "Modrinth endpoint is invalid")?;
        let ids_json = serde_json::to_string(&ids).map_err(|_| "Could not encode project IDs")?;
        url.query_pairs_mut().append_pair("ids", &ids_json);
        let response = self.client.get(url).send().await.map_err(|_| {
            "Modrinth is unavailable. Check your connection and try again.".to_string()
        })?;
        if !response.status().is_success() {
            return Err(format!(
                "Modrinth icon lookup failed (HTTP {}).",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length as usize > RESPONSE_LIMIT)
        {
            return Err("Modrinth icon response exceeded the allowed size.".into());
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| "Could not read Modrinth icon metadata.")?;
        if body.len() > RESPONSE_LIMIT {
            return Err("Modrinth icon response exceeded the allowed size.".into());
        }
        let projects: Vec<ProjectIconResponse> = serde_json::from_slice(&body)
            .map_err(|_| "Modrinth returned invalid project icon metadata.")?;
        Ok(projects
            .into_iter()
            .filter(|project| valid_id(&project.id))
            .map(|project| ModrinthProjectIcon {
                project_id: project.id,
                slug: project.slug,
                icon_url: safe_icon_url(project.icon_url.as_deref().unwrap_or_default()),
            })
            .collect())
    }

    async fn search_projects(
        &self,
        query: &str,
        minecraft_version: &str,
        project_type: &str,
        category: Option<&str>,
    ) -> Result<Vec<ModrinthProject>, String> {
        let query = query.trim();
        if (!query.is_empty() && query.len() < 2)
            || query.len() > 120
            || minecraft_version.len() > 32
            || !minecraft_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return Err(
                "Search needs at least two characters and a valid Minecraft version.".into(),
            );
        }
        let key = format!(
            "{}\n{}\n{}\n{}",
            project_type,
            category.unwrap_or_default(),
            query.to_lowercase(),
            minecraft_version
        );
        if let Some(cached) = self
            .cache
            .lock()
            .map_err(|_| "Mod search cache is unavailable")?
            .get(&key)
        {
            if Instant::now() < cached.expires {
                return Ok(cached.projects.clone());
            }
        }
        let mut url =
            Url::parse(&format!("{API}/search")).map_err(|_| "Modrinth endpoint is invalid")?;
        let facets = if let Some(category) = category {
            format!("[[\"project_type:{project_type}\"],[\"categories:{category}\"],[\"versions:{minecraft_version}\"]]")
        } else {
            format!("[[\"project_type:{project_type}\"],[\"versions:{minecraft_version}\"]]")
        };
        {
            let mut query_pairs = url.query_pairs_mut();
            if !query.is_empty() {
                query_pairs.append_pair("query", query);
            }
            query_pairs
                .append_pair("index", "downloads")
                .append_pair("limit", "48")
                .append_pair("facets", &facets);
        }
        if url.scheme() != "https" || url.host_str() != Some("api.modrinth.com") {
            return Err("Modrinth endpoint is not trusted.".into());
        }
        let response = self.client.get(url).send().await.map_err(|_| {
            "Modrinth is unavailable. Check your connection and try again.".to_string()
        })?;
        if !response.status().is_success() {
            return Err(format!(
                "Modrinth search failed (HTTP {}).",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length as usize > RESPONSE_LIMIT)
        {
            return Err("Modrinth search response exceeded the allowed size.".into());
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| "Could not read Modrinth results.".to_string())?;
        if body.len() > RESPONSE_LIMIT {
            return Err("Modrinth search response exceeded the allowed size.".into());
        }
        let decoded: SearchResponse = serde_json::from_slice(&body)
            .map_err(|_| "Modrinth returned invalid search metadata.".to_string())?;
        let projects = compatible_projects(decoded.hits, minecraft_version, project_type, category);
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "Mod search cache is unavailable")?;
        cache.retain(|_, entry| Instant::now() < entry.expires);
        if cache.len() >= 64 {
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
            }
        }
        cache.insert(
            key,
            CacheEntry {
                expires: Instant::now() + CACHE_TTL,
                projects: projects.clone(),
            },
        );
        Ok(projects)
    }
}

fn compatible_projects(
    hits: Vec<SearchHit>,
    minecraft_version: &str,
    project_type: &str,
    category: Option<&str>,
) -> Vec<ModrinthProject> {
    hits.into_iter()
        .filter(|hit| {
            hit.project_type == project_type
                && category
                    .is_none_or(|category| hit.categories.iter().any(|item| item == category))
                && hit
                    .versions
                    .iter()
                    .any(|version| version == minecraft_version)
                && valid_id(&hit.project_id)
        })
        .map(|hit| ModrinthProject {
            project_id: hit.project_id,
            slug: hit.slug,
            title: hit.title,
            description: hit.description,
            author: hit.author,
            icon_url: safe_icon_url(hit.icon_url.as_deref().unwrap_or_default()),
            downloads: hit.downloads,
            categories: hit.categories,
            game_versions: hit.versions,
        })
        .collect()
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn safe_icon_url(value: &str) -> String {
    Url::parse(value)
        .ok()
        .filter(|url| {
            url.scheme() == "https"
                && matches!(
                    url.host_str(),
                    Some("cdn.modrinth.com" | "cdn-raw.modrinth.com")
                )
        })
        .map(|url| url.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{compatible_projects, safe_icon_url, valid_id, SearchHit};

    fn hit(
        project_id: &str,
        project_type: &str,
        categories: &[&str],
        versions: &[&str],
    ) -> SearchHit {
        SearchHit {
            project_id: project_id.into(),
            slug: project_id.into(),
            title: project_id.into(),
            description: String::new(),
            author: "author".into(),
            icon_url: Some("https://cdn.modrinth.com/data/project/icon.png".into()),
            downloads: 10,
            categories: categories.iter().map(|item| (*item).into()).collect(),
            versions: versions.iter().map(|item| (*item).into()).collect(),
            project_type: project_type.into(),
        }
    }

    #[test]
    fn project_identifiers_and_artwork_hosts_are_restricted() {
        assert!(valid_id("fabric-api"));
        assert!(!valid_id("../fabric-api"));
        assert_eq!(
            safe_icon_url("https://cdn.modrinth.com/data/abc/icon.png"),
            "https://cdn.modrinth.com/data/abc/icon.png"
        );
        assert!(safe_icon_url("http://evil.invalid/a.png").is_empty());
    }

    #[test]
    fn search_results_only_include_compatible_fabric_mods() {
        let results = compatible_projects(
            vec![
                hit("sodium", "mod", &["fabric", "optimization"], &["1.21.11"]),
                hit("forge-only", "mod", &["forge"], &["1.21.11"]),
                hit("wrong-version", "mod", &["fabric"], &["1.21.10"]),
                hit("resource-pack", "resourcepack", &["fabric"], &["1.21.11"]),
                hit("../unsafe", "mod", &["fabric"], &["1.21.11"]),
            ],
            "1.21.11",
            "mod",
            Some("fabric"),
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].slug, "sodium");
    }

    #[test]
    fn resource_pack_search_accepts_only_matching_resource_pack_projects() {
        let results = compatible_projects(
            vec![
                hit("pack", "resourcepack", &[], &["1.21.11"]),
                hit("mod", "mod", &["fabric"], &["1.21.11"]),
                hit("old-pack", "resourcepack", &[], &["1.20.1"]),
            ],
            "1.21.11",
            "resourcepack",
            None,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].slug, "pack");
    }
}
