use crate::settings::CookieBrowser;
use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieCheck {
    /// The registrable domain the check ran against, for the UI to name.
    pub domain: String,
    pub status: CookieStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CookieStatus {
    Present,
    Absent,
    Unknown,
}

#[tauri::command]
pub async fn cookie_check(url: String, browser: CookieBrowser) -> Result<CookieCheck, String> {
    tokio::task::spawn_blocking(move || check(&url, browser))
        .await
        .map_err(|e| e.to_string())
}

fn check(url: &str, browser: CookieBrowser) -> CookieCheck {
    sweep_stale_copies();
    let Some(domain) = registrable_domain(url) else {
        return CookieCheck {
            domain: String::new(),
            status: CookieStatus::Unknown,
        };
    };
    let status = status_for(store_holds_domain(browser, &domain));
    CookieCheck { domain, status }
}

/// A check that could not run is `Unknown`, never `Absent`. A warning that the
/// browser holds no cookies for a site it is signed in to is worse than no
/// warning at all.
fn status_for(store: Result<bool, String>) -> CookieStatus {
    match store {
        Ok(true) => CookieStatus::Present,
        Ok(false) => CookieStatus::Absent,
        Err(_) => CookieStatus::Unknown,
    }
}

/// Derives the registrable domain of a URL: `instagram.com` from
/// `https://www.instagram.com/reel/DcxYeDDvkUG/`.
///
/// The rule is the last two labels rather than the public suffix list, so a host
/// under a multi-label suffix collapses to the suffix itself: `bbc.co.uk` yields
/// `co.uk`, which then also matches cookies from unrelated `.co.uk` sites.
/// Address literals and single-label hosts are returned whole.
fn registrable_domain(url: &str) -> Option<String> {
    let host = host_of(url)?;
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Some(host);
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.iter().any(|label| label.is_empty()) {
        return None;
    }
    match labels.len() {
        0 => None,
        1 => Some(host),
        n => Some(labels[n - 2..].join(".")),
    }
}

fn host_of(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let after_scheme = trimmed.split_once("://").map_or(trimmed, |(_, rest)| rest);
    let authority = after_scheme.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match authority.strip_prefix('[') {
        Some(bracketed) => bracketed.split_once(']')?.0,
        None => authority.split(':').next()?,
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let plausible = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
    plausible.then_some(host)
}

/// A stored cookie host belongs to `domain` when it is that domain or a
/// subdomain of it. Both stores mark a domain cookie with a leading dot, which
/// the subdomain arm covers.
fn host_matches(stored_host: &str, domain: &str) -> bool {
    if domain.is_empty() {
        return false;
    }
    let host = stored_host.trim().to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn store_holds_domain(browser: CookieBrowser, domain: &str) -> Result<bool, String> {
    let databases = cookie_databases(browser);
    if databases.is_empty() {
        return Err("no cookie database found".to_string());
    }
    let query = host_query(browser);
    let mut unreadable = None;
    for database in databases {
        match database_holds_domain(&database, query, domain) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(e) => unreadable = Some(e),
        }
    }
    // A store that would not open could be the profile holding the cookie, so
    // `Absent` is only reported once every located store was read.
    match unreadable {
        Some(e) => Err(e),
        None => Ok(false),
    }
}

fn host_query(browser: CookieBrowser) -> &'static str {
    match browser {
        CookieBrowser::Firefox => "SELECT DISTINCT host FROM moz_cookies",
        CookieBrowser::Chrome => "SELECT DISTINCT host_key FROM cookies",
    }
}

fn database_holds_domain(database: &Path, query: &str, domain: &str) -> Result<bool, String> {
    let copy = TempCopy::of(database).map_err(|e| e.to_string())?;
    let conn = Connection::open(&copy.database).map_err(|e| e.to_string())?;
    let mut statement = conn.prepare(query).map_err(|e| e.to_string())?;
    let mut rows = statement.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let host: Option<String> = row.get(0).ok().flatten();
        if host.is_some_and(|host| host_matches(&host, domain)) {
            return Ok(true);
        }
    }
    Ok(false)
}

static COPY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempCopy {
    dir: PathBuf,
    database: PathBuf,
}

impl TempCopy {
    /// Both browsers hold their cookie store open while they run, so the query
    /// reads a copy instead. The `-wal` sidecar travels with it or recent
    /// writes are invisible; it is renamed in step with the database so the
    /// `<database>-wal` pairing survives and SQLite replays the log on open.
    fn of(source: &Path) -> std::io::Result<Self> {
        let dir = copy_root().join(copy_name());
        std::fs::create_dir_all(&dir)?;
        let copy = TempCopy {
            database: dir.join("cookies.sqlite"),
            dir,
        };
        std::fs::copy(source, &copy.database)?;
        let wal = wal_sidecar(source);
        if wal.exists() {
            std::fs::copy(&wal, wal_sidecar(&copy.database))?;
        }
        Ok(copy)
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Every copy lives under one directory so leftovers are identifiable.
fn copy_root() -> PathBuf {
    std::env::temp_dir().join("media-dlp-cookies")
}

fn copy_name() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        COPY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

/// `Drop` removes a copy on every in-process path, but killing the app mid
/// check leaves one behind, so a check clears what earlier runs left. Copies
/// named for this process may belong to a check running right now. Failures
/// are ignored: another instance can hold a copy open, and a sweep that cannot
/// run is not a reason to fail the check.
fn sweep_stale_copies() {
    sweep_stale_copies_in(&copy_root());
}

fn sweep_stale_copies_in(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mine = format!("{}-", std::process::id());
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(&mine) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn wal_sidecar(database: &Path) -> PathBuf {
    let mut name = database.file_name().unwrap_or_default().to_os_string();
    name.push("-wal");
    database.with_file_name(name)
}

fn cookie_databases(browser: CookieBrowser) -> Vec<PathBuf> {
    match browser {
        CookieBrowser::Firefox => firefox_cookie_databases(),
        CookieBrowser::Chrome => chrome_cookie_databases(),
    }
}

/// Firefox writes `cookies.sqlite` into the profile directory, and a root is
/// sometimes the profile's parent and sometimes its grandparent.
fn firefox_cookie_databases() -> Vec<PathBuf> {
    let mut databases = Vec::new();
    for root in firefox_roots() {
        let nested = root.join("Profiles");
        let mut dirs = vec![root.clone(), nested.clone()];
        dirs.extend(child_dirs(&root));
        dirs.extend(child_dirs(&nested));
        for dir in dirs {
            push_existing(&mut databases, dir.join("cookies.sqlite"));
        }
    }
    databases
}

/// Chrome writes `Cookies` into each profile directory, under `Network` since
/// Chrome 96 and directly in the profile before that.
fn chrome_cookie_databases() -> Vec<PathBuf> {
    let mut databases = Vec::new();
    for root in chrome_roots() {
        let mut dirs = vec![root.clone()];
        dirs.extend(child_dirs(&root));
        for dir in dirs {
            push_existing(&mut databases, dir.join("Cookies"));
            push_existing(&mut databases, dir.join("Network").join("Cookies"));
        }
    }
    databases
}

fn firefox_roots() -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        [
            env_dir("APPDATA").map(|d| d.join(r"Mozilla\Firefox\Profiles")),
            env_dir("LOCALAPPDATA").map(|d| {
                d.join(r"Packages\Mozilla.Firefox_n80bbvh6b1yt2\LocalCache\Roaming\Mozilla\Firefox\Profiles")
            }),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else if cfg!(target_os = "macos") {
        home()
            .map(|h| h.join("Library/Application Support/Firefox/Profiles"))
            .into_iter()
            .collect()
    } else {
        [
            config_home().map(|c| c.join("mozilla/firefox")),
            home().map(|h| h.join(".mozilla/firefox")),
            home().map(|h| h.join(".var/app/org.mozilla.firefox/config/mozilla/firefox")),
            home().map(|h| h.join(".var/app/org.mozilla.firefox/.mozilla/firefox")),
            home().map(|h| h.join("snap/firefox/common/.mozilla/firefox")),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

fn chrome_roots() -> Vec<PathBuf> {
    let root = if cfg!(target_os = "windows") {
        env_dir("LOCALAPPDATA").map(|d| d.join(r"Google\Chrome\User Data"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/Google/Chrome"))
    } else {
        config_home().map(|c| c.join("google-chrome"))
    };
    root.into_iter().collect()
}

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn home() -> Option<PathBuf> {
    env_dir(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
}

fn config_home() -> Option<PathBuf> {
    env_dir("XDG_CONFIG_HOME").or_else(|| home().map(|h| h.join(".config")))
}

fn child_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

fn push_existing(databases: &mut Vec<PathBuf>, candidate: PathBuf) {
    if candidate.is_file() && !databases.contains(&candidate) {
        databases.push(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_host_is_its_own_registrable_domain() {
        assert_eq!(
            registrable_domain("https://instagram.com/reel/x").as_deref(),
            Some("instagram.com")
        );
    }

    #[test]
    fn a_www_host_drops_to_the_last_two_labels() {
        assert_eq!(
            registrable_domain("https://www.instagram.com/reel/DcxYeDDvkUG/").as_deref(),
            Some("instagram.com")
        );
    }

    #[test]
    fn a_port_is_not_part_of_the_domain() {
        assert_eq!(
            registrable_domain("https://www.example.com:8443/watch").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn a_path_and_query_are_stripped() {
        assert_eq!(
            registrable_domain("https://m.youtube.com/watch?v=abc&t=30#frag").as_deref(),
            Some("youtube.com")
        );
    }

    #[test]
    fn a_bare_address_literal_is_kept_whole() {
        assert_eq!(
            registrable_domain("http://192.168.1.10:8080/v").as_deref(),
            Some("192.168.1.10")
        );
        assert_eq!(
            registrable_domain("http://[::1]:8080/v").as_deref(),
            Some("::1")
        );
    }

    #[test]
    fn an_unparseable_string_has_no_domain() {
        assert_eq!(registrable_domain("not a url"), None);
        assert_eq!(registrable_domain(""), None);
        assert_eq!(registrable_domain("https://"), None);
    }

    #[test]
    fn a_multi_label_suffix_collapses_to_the_suffix() {
        assert_eq!(
            registrable_domain("https://www.bbc.co.uk/iplayer").as_deref(),
            Some("co.uk")
        );
    }

    #[test]
    fn an_exact_host_matches() {
        assert!(host_matches("instagram.com", "instagram.com"));
    }

    #[test]
    fn a_leading_dot_domain_cookie_matches() {
        assert!(host_matches(".instagram.com", "instagram.com"));
    }

    #[test]
    fn a_subdomain_host_matches() {
        assert!(host_matches("www.instagram.com", "instagram.com"));
        assert!(host_matches("i.cdn.instagram.com", "instagram.com"));
    }

    #[test]
    fn a_host_sharing_only_a_suffix_of_letters_does_not_match() {
        assert!(!host_matches("notinstagram.com", "instagram.com"));
        assert!(!host_matches("instagram.com.evil.test", "instagram.com"));
    }

    #[test]
    fn host_matching_ignores_case() {
        assert!(host_matches(".Instagram.COM", "instagram.com"));
    }

    #[test]
    fn a_readable_store_with_a_row_is_present() {
        assert_eq!(status_for(Ok(true)), CookieStatus::Present);
    }

    #[test]
    fn a_readable_store_without_a_row_is_absent() {
        assert_eq!(status_for(Ok(false)), CookieStatus::Absent);
    }

    #[test]
    fn a_store_that_could_not_be_read_is_unknown_not_absent() {
        assert_eq!(
            status_for(Err("no cookie database found".to_string())),
            CookieStatus::Unknown
        );
    }

    #[test]
    fn an_unparseable_url_reports_unknown_without_a_domain() {
        let check = check("not a url", CookieBrowser::Firefox);
        assert_eq!(check.status, CookieStatus::Unknown);
        assert!(check.domain.is_empty());
    }

    fn test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "media-dlp-cookies-test-{}-{}",
            std::process::id(),
            COPY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn firefox_store(hosts: &[&str]) -> (PathBuf, PathBuf) {
        let dir = test_dir();
        let database = dir.join("cookies.sqlite");
        let conn = Connection::open(&database).unwrap();
        conn.execute("CREATE TABLE moz_cookies (host TEXT)", [])
            .unwrap();
        for host in hosts {
            conn.execute("INSERT INTO moz_cookies (host) VALUES (?1)", [host])
                .unwrap();
        }
        drop(conn);
        (dir, database)
    }

    #[test]
    fn a_store_holding_the_domain_is_found_through_the_copy() {
        let (dir, database) = firefox_store(&[".instagram.com", "accounts.google.com"]);
        let query = host_query(CookieBrowser::Firefox);
        assert_eq!(
            database_holds_domain(&database, query, "instagram.com"),
            Ok(true)
        );
        assert_eq!(
            database_holds_domain(&database, query, "notinstagram.com"),
            Ok(false)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_row_still_in_the_write_ahead_log_is_visible_through_the_copy() {
        let (dir, database) = firefox_store(&["example.com"]);
        let conn = Connection::open(&database).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute(
            "INSERT INTO moz_cookies (host) VALUES ('.instagram.com')",
            [],
        )
        .unwrap();
        let found = database_holds_domain(
            &database,
            host_query(CookieBrowser::Firefox),
            "instagram.com",
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found, Ok(true));
    }

    #[test]
    fn a_missing_store_is_an_error_rather_than_an_empty_answer() {
        let missing = std::env::temp_dir().join("media-dlp-cookies-does-not-exist/cookies.sqlite");
        assert!(
            database_holds_domain(
                &missing,
                host_query(CookieBrowser::Firefox),
                "instagram.com"
            )
            .is_err()
        );
    }

    #[test]
    fn a_sweep_removes_copies_from_earlier_runs_and_spares_this_one() {
        let root = test_dir();
        let stale = root.join(format!("{}-0", std::process::id() + 1));
        let live = root.join(copy_name());
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::create_dir_all(&live).unwrap();

        sweep_stale_copies_in(&root);

        let (stale_exists, live_exists) = (stale.exists(), live.exists());
        let _ = std::fs::remove_dir_all(&root);
        assert!(!stale_exists);
        assert!(live_exists);
    }

    #[test]
    fn a_sweep_of_a_directory_that_is_not_there_is_not_an_error() {
        sweep_stale_copies_in(&std::env::temp_dir().join("media-dlp-cookies-does-not-exist"));
    }

    #[test]
    fn the_temporary_copy_is_deleted_when_it_goes_out_of_scope() {
        let (dir, database) = firefox_store(&["example.com"]);
        let copy_dir = {
            let copy = TempCopy::of(&database).unwrap();
            assert!(copy.database.is_file());
            copy.dir.clone()
        };
        assert!(!copy_dir.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
