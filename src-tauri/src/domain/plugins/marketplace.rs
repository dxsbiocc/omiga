use super::*;

pub(super) const MARKETPLACE_FILE_NAME: &str = "marketplace.json";
const BUILTIN_GIT_URL: &str = "https://github.com/dxsbiocc/omiga-plugins.git";
const MAX_REMOTE_MARKETPLACE_BYTES: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceInterface {
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceRemote {
    pub url: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default, alias = "repositoryURL")]
    pub repository_url: Option<String>,
    #[serde(default, alias = "changelogURL")]
    pub changelog_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PluginInstallPolicy {
    NotAvailable,
    Available,
    InstalledByDefault,
}

#[allow(clippy::derivable_impls)]
impl Default for PluginInstallPolicy {
    fn default() -> Self {
        Self::Available
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PluginAuthPolicy {
    OnInstall,
    OnUse,
}

#[allow(clippy::derivable_impls)]
impl Default for PluginAuthPolicy {
    fn default() -> Self {
        Self::OnInstall
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawMarketplaceManifest {
    pub(super) name: String,
    #[serde(default)]
    pub(super) interface: Option<MarketplaceInterface>,
    #[serde(default)]
    pub(super) remote: Option<MarketplaceRemote>,
    #[serde(default)]
    pub(super) plugins: Vec<RawMarketplacePlugin>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawMarketplacePlugin {
    pub(super) name: String,
    pub(super) source: RawMarketplacePluginSource,
    #[serde(default)]
    pub(super) version: Option<String>,
    #[serde(default)]
    pub(super) policy: RawMarketplacePluginPolicy,
    #[serde(default)]
    pub(super) category: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawMarketplacePluginSource {
    #[serde(default)]
    pub(super) source: String,
    pub(super) path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawMarketplacePluginPolicy {
    #[serde(default)]
    pub(super) installation: PluginInstallPolicy,
    #[serde(default)]
    pub(super) authentication: PluginAuthPolicy,
}

impl Default for RawMarketplacePluginPolicy {
    fn default() -> Self {
        Self {
            installation: PluginInstallPolicy::Available,
            authentication: PluginAuthPolicy::OnInstall,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMarketplaceEntry {
    pub name: String,
    pub path: String,
    pub interface: Option<MarketplaceInterface>,
    pub remote: Option<MarketplaceRemote>,
    pub plugins: Vec<PluginSummary>,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceRemoteCheckResult {
    pub name: String,
    pub path: String,
    pub remote: MarketplaceRemote,
    pub state: String,
    pub label: String,
    pub message: String,
    pub local_digest: Option<String>,
    pub remote_digest: Option<String>,
    pub remote_plugin_count: Option<usize>,
    pub changed_plugins: Vec<String>,
    pub checked_at: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MarketplaceSourceKind {
    Local,
    Remote,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserMarketplaceSource {
    pub id: String,
    pub kind: MarketplaceSourceKind,
    pub location: String,
    pub label: Option<String>,
    pub enabled: bool,
    pub added_at: String,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum MarketplaceSourceViewKind {
    Builtin,
    Local,
    Remote,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSourceView {
    pub id: String,
    pub kind: MarketplaceSourceViewKind,
    pub location: String,
    pub label: Option<String>,
    pub enabled: bool,
    pub removable: bool,
    pub added_at: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct BuiltinMarketplaceStatus {
    pub ok: bool,
    pub source: String,
    pub path: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub id: String,
    pub ok: bool,
    pub message: String,
    pub marketplace_name: Option<String>,
    pub plugin_count: Option<usize>,
}
pub(super) fn user_marketplace_cache_dir(source_id: &str) -> Result<PathBuf, String> {
    validate_segment(source_id, "marketplace source id")?;
    Ok(omiga_home().join("marketplaces").join(source_id))
}

pub(super) fn user_marketplace_cache_manifest_path(source_id: &str) -> Result<PathBuf, String> {
    Ok(user_marketplace_cache_dir(source_id)?.join(MARKETPLACE_FILE_NAME))
}

pub(super) fn default_url_validation_project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
pub fn dev_builtin_marketplace_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .parent()
        .expect("workspace root")
        .join("omiga-plugins")
        .join(MARKETPLACE_FILE_NAME)
}

pub(super) fn builtin_marketplace_cache_dir() -> PathBuf {
    omiga_home().join("marketplaces").join("builtin")
}

pub(super) fn builtin_marketplace_cache_manifest_path() -> PathBuf {
    builtin_marketplace_cache_dir().join(MARKETPLACE_FILE_NAME)
}

pub(super) fn builtin_env_override_path() -> Option<PathBuf> {
    std::env::var_os("OMIGA_PLUGINS_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

pub(super) fn resolve_marketplace_json_override(path: PathBuf) -> Option<PathBuf> {
    let candidate = if path.is_dir() {
        path.join(MARKETPLACE_FILE_NAME)
    } else {
        path
    };
    if candidate.file_name().and_then(|name| name.to_str()) == Some(MARKETPLACE_FILE_NAME)
        && candidate.is_file()
    {
        Some(candidate)
    } else {
        None
    }
}

pub(super) fn resolve_builtin_env_marketplace_path() -> Option<PathBuf> {
    builtin_env_override_path().and_then(resolve_marketplace_json_override)
}

pub(super) fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn resolve_builtin_marketplace_path(
    env_override: Option<PathBuf>,
    dev_sibling: Option<PathBuf>,
    cache_marketplace: PathBuf,
) -> Option<PathBuf> {
    if let Some(path) = env_override.and_then(resolve_marketplace_json_override) {
        return Some(path);
    }
    if let Some(path) = dev_sibling.filter(|path| path.is_file()) {
        return Some(path);
    }
    cache_marketplace.is_file().then_some(cache_marketplace)
}

pub fn builtin_marketplace_path() -> Option<PathBuf> {
    resolve_builtin_marketplace_path(
        builtin_env_override_path(),
        Some(dev_builtin_marketplace_path()),
        builtin_marketplace_cache_manifest_path(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BuiltinMarketplaceSource {
    Env(PathBuf),
    Dev(PathBuf),
    GithubCache(PathBuf),
    GithubRemote,
}

fn builtin_marketplace_source() -> BuiltinMarketplaceSource {
    if let Some(path) = resolve_builtin_env_marketplace_path() {
        return BuiltinMarketplaceSource::Env(path);
    }

    let dev_path = dev_builtin_marketplace_path();
    if dev_path.is_file() {
        return BuiltinMarketplaceSource::Dev(dev_path);
    }

    let cache_path = builtin_marketplace_cache_manifest_path();
    if cache_path.is_file() {
        BuiltinMarketplaceSource::GithubCache(cache_path)
    } else {
        BuiltinMarketplaceSource::GithubRemote
    }
}

pub fn marketplace_paths(
    _project_root: Option<&Path>,
    _resource_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut push_path = |path: PathBuf| {
        let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if path.is_file() && seen.insert(key) {
            paths.push(path);
        }
    };
    if let Some(path) = builtin_marketplace_path() {
        push_path(path);
    }
    for source in read_config()
        .marketplaces
        .into_iter()
        .filter(|source| source.enabled)
    {
        match source.kind {
            MarketplaceSourceKind::Local => {
                match resolve_user_local_marketplace_path(&source.location) {
                    Ok(path) => push_path(path),
                    Err(err) => {
                        tracing::warn!(
                            source_id = %source.id,
                            location = %source.location,
                            "skipping configured plugin marketplace source: {err}"
                        );
                    }
                }
            }
            MarketplaceSourceKind::Remote => {
                let path = match user_marketplace_cache_manifest_path(&source.id) {
                    Ok(path) => path,
                    Err(err) => {
                        tracing::warn!(
                            source_id = %source.id,
                            location = %source.location,
                            "skipping configured remote plugin marketplace source: {err}"
                        );
                        continue;
                    }
                };
                if !path.is_file() {
                    tracing::warn!(
                        source_id = %source.id,
                        location = %source.location,
                        path = %path.display(),
                        "skipping configured remote plugin marketplace source: cached marketplace does not exist"
                    );
                    continue;
                }
                match read_marketplace(&path) {
                    Ok(_) => push_path(path),
                    Err(err) => {
                        tracing::warn!(
                            source_id = %source.id,
                            location = %source.location,
                            path = %path.display(),
                            "skipping configured remote plugin marketplace source: {err}"
                        );
                    }
                }
            }
        }
    }
    paths
}

pub(super) fn resolve_user_local_marketplace_path(location: &str) -> Result<PathBuf, String> {
    let trimmed = location.trim();
    if trimmed.is_empty() {
        return Err("local marketplace source path must not be empty".to_string());
    }
    let input = PathBuf::from(trimmed);
    let metadata = fs::metadata(&input).map_err(|err| {
        format!(
            "local marketplace source `{}` does not exist: {err}",
            input.display()
        )
    })?;

    let candidate = if metadata.is_dir() {
        let dir = fs::canonicalize(&input).map_err(|err| {
            format!(
                "canonicalize local marketplace directory `{}`: {err}",
                input.display()
            )
        })?;
        dir.join(MARKETPLACE_FILE_NAME)
    } else if metadata.is_file() {
        let path = fs::canonicalize(&input).map_err(|err| {
            format!(
                "canonicalize local marketplace file `{}`: {err}",
                input.display()
            )
        })?;
        if path.file_name().and_then(|name| name.to_str()) != Some(MARKETPLACE_FILE_NAME) {
            return Err(format!(
                "local marketplace source file must be named `{MARKETPLACE_FILE_NAME}`"
            ));
        }
        path
    } else {
        return Err(format!(
            "local marketplace source `{}` must be a file or directory",
            input.display()
        ));
    };

    if !candidate.is_file() {
        return Err(format!(
            "local marketplace source `{}` does not contain `{MARKETPLACE_FILE_NAME}`",
            input.display()
        ));
    }
    let path = fs::canonicalize(&candidate).map_err(|err| {
        format!(
            "canonicalize local marketplace file `{}`: {err}",
            candidate.display()
        )
    })?;
    read_marketplace(&path)
        .map_err(|err| format!("invalid local marketplace `{}`: {err}", path.display()))?;
    Ok(path)
}

pub(super) fn normalized_user_local_marketplace_location(
    location: &str,
) -> Result<(String, PathBuf), String> {
    let input = PathBuf::from(location.trim());
    let marketplace_path = resolve_user_local_marketplace_path(location)?;
    let metadata = fs::metadata(&input).map_err(|err| {
        format!(
            "local marketplace source `{}` does not exist: {err}",
            input.display()
        )
    })?;
    let location_path = if metadata.is_dir() {
        fs::canonicalize(&input).map_err(|err| {
            format!(
                "canonicalize local marketplace directory `{}`: {err}",
                input.display()
            )
        })?
    } else {
        marketplace_path.clone()
    };
    let location = location_path
        .to_str()
        .ok_or_else(|| "local marketplace source path must be valid UTF-8".to_string())?
        .to_string();
    Ok((location, marketplace_path))
}

pub fn marketplace_plugin_source_root(
    plugin_id: &str,
    project_root: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Option<PathBuf> {
    let plugin_id = PluginId::parse(plugin_id).ok()?;
    for path in marketplace_paths(project_root, resource_dir) {
        let Ok(marketplace) = read_marketplace(&path) else {
            continue;
        };
        if marketplace.name != plugin_id.marketplace {
            continue;
        }
        for entry in &marketplace.plugins {
            if entry.name == plugin_id.name {
                return resolve_marketplace_source_path(&path, &entry.source).ok();
            }
        }
    }
    None
}

pub(super) fn marketplace_root_dir(marketplace_path: &Path) -> PathBuf {
    let parent = marketplace_path.parent().unwrap_or_else(|| Path::new("."));
    if parent.file_name().and_then(|s| s.to_str()) == Some("plugins")
        && parent
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            == Some(".omiga")
    {
        return parent.parent().unwrap_or(parent).to_path_buf();
    }
    parent.to_path_buf()
}
pub fn list_user_marketplace_sources() -> Vec<UserMarketplaceSource> {
    read_config().marketplaces
}

pub(super) fn builtin_marketplace_label(path: &Path) -> Option<String> {
    match read_marketplace(path) {
        Ok(marketplace) => marketplace
            .interface
            .and_then(|interface| interface.display_name)
            .filter(|display_name| !display_name.trim().is_empty())
            .or(Some(marketplace.name)),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                "failed to read built-in plugin marketplace source label: {err}"
            );
            None
        }
    }
}

pub fn list_marketplace_source_views() -> Vec<MarketplaceSourceView> {
    let mut views = Vec::new();
    let (location, label) = match builtin_marketplace_source() {
        BuiltinMarketplaceSource::Env(path) | BuiltinMarketplaceSource::Dev(path) => {
            (path_to_string(&path), builtin_marketplace_label(&path))
        }
        BuiltinMarketplaceSource::GithubCache(path) => (
            BUILTIN_GIT_URL.to_string(),
            builtin_marketplace_label(&path),
        ),
        BuiltinMarketplaceSource::GithubRemote => (BUILTIN_GIT_URL.to_string(), None),
    };
    views.push(MarketplaceSourceView {
        id: "builtin".to_string(),
        kind: MarketplaceSourceViewKind::Builtin,
        location,
        label,
        enabled: true,
        removable: false,
        added_at: None,
    });

    views.extend(
        list_user_marketplace_sources()
            .into_iter()
            .map(|source| MarketplaceSourceView {
                id: source.id,
                kind: match source.kind {
                    MarketplaceSourceKind::Local => MarketplaceSourceViewKind::Local,
                    MarketplaceSourceKind::Remote => MarketplaceSourceViewKind::Remote,
                },
                location: source.location,
                label: source.label,
                enabled: source.enabled,
                removable: true,
                added_at: Some(source.added_at),
            }),
    );

    views
}

pub(super) fn normalized_marketplace_label(label: Option<String>) -> Option<String> {
    label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn new_marketplace_source_id(prefix: &str, config: &PluginConfigFile) -> String {
    let existing_ids = config
        .marketplaces
        .iter()
        .map(|source| source.id.clone())
        .collect::<HashSet<_>>();
    let mut id = format!("{prefix}-{}", uuid::Uuid::new_v4());
    while existing_ids.contains(&id) {
        id = format!("{prefix}-{}", uuid::Uuid::new_v4());
    }
    id
}

pub(super) fn normalized_remote_marketplace_location_for_compare(location: &str) -> String {
    reqwest::Url::parse(location.trim())
        .map(|url| url.to_string())
        .unwrap_or_else(|_| location.trim().to_string())
}

pub(super) fn validate_remote_marketplace_url(
    location: &str,
    project_root: &Path,
    resolve_dns: bool,
) -> Result<String, String> {
    let trimmed = location.trim();
    if trimmed.is_empty() {
        return Err("remote marketplace source URL must not be empty".to_string());
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|err| format!("Invalid URL: {err}"))?;
    if parsed.scheme() != "https" {
        return Err("remote marketplace source URL must use https".to_string());
    }
    crate::domain::tools::web_safety::validate_public_http_url(project_root, trimmed, resolve_dns)
        .map_err(|err| format!("remote marketplace source URL is not allowed: {err}"))?;
    Ok(parsed.to_string())
}

pub(super) fn add_local_user_marketplace_source(
    location: String,
    label: Option<String>,
) -> Result<UserMarketplaceSource, String> {
    let (location, marketplace_path) = normalized_user_local_marketplace_location(&location)?;
    let mut config = read_config();
    for source in &config.marketplaces {
        if source.kind != MarketplaceSourceKind::Local {
            continue;
        }
        match resolve_user_local_marketplace_path(&source.location) {
            Ok(existing_path) if existing_path == marketplace_path => {
                return Err(format!(
                    "local marketplace source `{}` is already configured",
                    marketplace_path.display()
                ));
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(
                    source_id = %source.id,
                    location = %source.location,
                    "ignoring invalid configured plugin marketplace source while checking duplicates: {err}"
                );
            }
        }
    }

    let source = UserMarketplaceSource {
        id: new_marketplace_source_id("local", &config),
        kind: MarketplaceSourceKind::Local,
        location,
        label: normalized_marketplace_label(label),
        enabled: true,
        added_at: chrono::Utc::now().to_rfc3339(),
    };
    config.marketplaces.push(source.clone());
    write_config(&config)?;
    Ok(source)
}

pub(super) fn add_remote_user_marketplace_source(
    location: String,
    label: Option<String>,
    project_root: Option<&Path>,
) -> Result<UserMarketplaceSource, String> {
    let default_project_root;
    let project_root = match project_root {
        Some(project_root) => project_root,
        None => {
            default_project_root = default_url_validation_project_root();
            default_project_root.as_path()
        }
    };
    let location = validate_remote_marketplace_url(&location, project_root, false)?;
    let duplicate_key = normalized_remote_marketplace_location_for_compare(&location);
    let mut config = read_config();
    for source in &config.marketplaces {
        if source.kind == MarketplaceSourceKind::Remote
            && normalized_remote_marketplace_location_for_compare(&source.location) == duplicate_key
        {
            return Err(format!(
                "remote marketplace source URL `{location}` is already configured"
            ));
        }
    }

    let source = UserMarketplaceSource {
        id: new_marketplace_source_id("remote", &config),
        kind: MarketplaceSourceKind::Remote,
        location,
        label: normalized_marketplace_label(label),
        enabled: true,
        added_at: chrono::Utc::now().to_rfc3339(),
    };
    config.marketplaces.push(source.clone());
    write_config(&config)?;
    Ok(source)
}

pub fn add_user_marketplace_source(
    kind: MarketplaceSourceKind,
    location: String,
    label: Option<String>,
) -> Result<UserMarketplaceSource, String> {
    add_user_marketplace_source_with_project_root(kind, location, label, None)
}

pub fn add_user_marketplace_source_with_project_root(
    kind: MarketplaceSourceKind,
    location: String,
    label: Option<String>,
    project_root: Option<&Path>,
) -> Result<UserMarketplaceSource, String> {
    match kind {
        MarketplaceSourceKind::Local => add_local_user_marketplace_source(location, label),
        MarketplaceSourceKind::Remote => {
            add_remote_user_marketplace_source(location, label, project_root)
        }
    }
}

const GIT_REQUIRED_MESSAGE: &str =
    "git is required to add remote marketplace sources; install git or use a local path";

pub(super) fn git_failure_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    format!("git exited with status {}", output.status)
}

pub(super) fn run_git_command(command: &mut Command, action: &str) -> Result<(), String> {
    let output = match command.output() {
        Ok(output) => output,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(GIT_REQUIRED_MESSAGE.to_string());
        }
        Err(err) => return Err(format!("run git {action}: {err}")),
    };
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "git {action} failed: {}",
        git_failure_message(&output)
    ))
}

pub(super) fn clone_or_update_marketplace_repo(
    remote_url: &str,
    dest: &Path,
) -> Result<(), String> {
    if dest.exists() && !valid_git_work_tree(dest)? {
        remove_marketplace_cache_dest(dest)?;
    }

    if !dest.exists() {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create marketplace cache dir: {err}"))?;
        }
        let mut command = Command::new("git");
        command
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--quiet")
            .arg(remote_url)
            .arg(dest);
        return run_git_command(&mut command, "clone");
    }

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dest)
        .arg("pull")
        .arg("--ff-only")
        .arg("--quiet");
    run_git_command(&mut command, "pull")
}

pub(super) fn valid_git_work_tree(dest: &Path) -> Result<bool, String> {
    if !dest.is_dir() || !dest.join(".git").exists() {
        return Ok(false);
    }

    let output = match Command::new("git")
        .arg("-C")
        .arg(dest)
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(GIT_REQUIRED_MESSAGE.to_string());
        }
        Err(err) => return Err(format!("run git rev-parse: {err}")),
    };
    if !output.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

pub(super) fn remove_marketplace_cache_dest(dest: &Path) -> Result<(), String> {
    if dest.is_dir() {
        fs::remove_dir_all(dest)
    } else {
        fs::remove_file(dest)
    }
    .map_err(|err| {
        format!(
            "remove invalid marketplace cache `{}`: {err}",
            dest.display()
        )
    })
}

pub(super) fn builtin_marketplace_status(
    ok: bool,
    source: &str,
    path: Option<&Path>,
    message: impl Into<String>,
) -> BuiltinMarketplaceStatus {
    BuiltinMarketplaceStatus {
        ok,
        source: source.to_string(),
        path: path.map(path_to_string),
        message: message.into(),
    }
}

pub(crate) fn ensure_builtin_marketplace() -> Result<BuiltinMarketplaceStatus, String> {
    if let Some(raw_env_path) = builtin_env_override_path() {
        if let Some(path) = resolve_marketplace_json_override(raw_env_path.clone()) {
            if let Err(err) = read_marketplace(&path) {
                return Ok(builtin_marketplace_status(
                    false,
                    "env",
                    Some(&path),
                    format!(
                        "OMIGA_PLUGINS_DIR built-in marketplace override `{}` is invalid: {err}",
                        path.display()
                    ),
                ));
            }
            return Ok(builtin_marketplace_status(
                true,
                "env",
                Some(&path),
                "Using OMIGA_PLUGINS_DIR built-in marketplace override",
            ));
        }
        tracing::warn!(
            location = %raw_env_path.display(),
            "OMIGA_PLUGINS_DIR does not point to an existing marketplace.json; falling back"
        );
    }

    let dev_path = dev_builtin_marketplace_path();
    if dev_path.is_file() {
        if let Err(err) = read_marketplace(&dev_path) {
            return Ok(builtin_marketplace_status(
                false,
                "dev",
                Some(&dev_path),
                format!(
                    "Development built-in marketplace `{}` is invalid: {err}",
                    dev_path.display()
                ),
            ));
        }
        return Ok(builtin_marketplace_status(
            true,
            "dev",
            Some(&dev_path),
            "Using development built-in marketplace",
        ));
    }

    let cache_dir = builtin_marketplace_cache_dir();
    if let Err(err) = clone_or_update_marketplace_repo(BUILTIN_GIT_URL, &cache_dir) {
        return Ok(builtin_marketplace_status(
            false,
            "github",
            None,
            format!(
                "Unable to clone or update the built-in marketplace from {BUILTIN_GIT_URL}: {err}. Check git and network access, then retry."
            ),
        ));
    }

    let marketplace_path = cache_dir.join(MARKETPLACE_FILE_NAME);
    if !marketplace_path.is_file() {
        return Ok(builtin_marketplace_status(
            false,
            "github",
            None,
            format!(
                "The built-in marketplace clone from {BUILTIN_GIT_URL} did not contain `{MARKETPLACE_FILE_NAME}` at `{}`. Remove `{}` and retry.",
                marketplace_path.display(),
                cache_dir.display()
            ),
        ));
    }

    if let Err(err) = read_marketplace(&marketplace_path) {
        return Ok(builtin_marketplace_status(
            false,
            "github",
            None,
            format!(
                "The built-in marketplace cache at `{}` is invalid: {err}. Remove `{}` and retry.",
                marketplace_path.display(),
                cache_dir.display()
            ),
        ));
    }

    Ok(builtin_marketplace_status(
        true,
        "github",
        Some(&marketplace_path),
        "Built-in marketplace is available from GitHub cache",
    ))
}

pub fn ensure_builtin_marketplace_source(
    _project_root: Option<String>,
) -> Result<BuiltinMarketplaceStatus, String> {
    ensure_builtin_marketplace()
}

pub(super) fn refresh_success_result(
    id: &str,
    message: impl Into<String>,
    marketplace: RawMarketplaceManifest,
) -> RefreshResult {
    RefreshResult {
        id: id.to_string(),
        ok: true,
        message: message.into(),
        marketplace_name: Some(marketplace.name),
        plugin_count: Some(marketplace.plugins.len()),
    }
}

pub(super) fn refresh_error_result(id: &str, message: impl Into<String>) -> RefreshResult {
    RefreshResult {
        id: id.to_string(),
        ok: false,
        message: message.into(),
        marketplace_name: None,
        plugin_count: None,
    }
}

pub(super) fn refresh_builtin_local_result(id: &str, source: &str, path: &Path) -> RefreshResult {
    match read_marketplace(path) {
        Ok(marketplace) => refresh_success_result(
            id,
            format!("Built-in marketplace source `{source}` does not require refresh"),
            marketplace,
        ),
        Err(err) => {
            tracing::warn!(
                source = source,
                path = %path.display(),
                "built-in marketplace source does not require refresh, but metadata could not be read: {err}"
            );
            refresh_error_result(
                id,
                format!(
                    "Built-in marketplace source `{source}` at `{}` is invalid: {err}",
                    path.display()
                ),
            )
        }
    }
}

pub(super) fn refresh_builtin_github_result(id: &str) -> RefreshResult {
    let cache_dir = builtin_marketplace_cache_dir();
    if let Err(err) = clone_or_update_marketplace_repo(BUILTIN_GIT_URL, &cache_dir) {
        return refresh_error_result(
            id,
            format!("Unable to refresh the built-in marketplace from {BUILTIN_GIT_URL}: {err}"),
        );
    }

    let marketplace_path = cache_dir.join(MARKETPLACE_FILE_NAME);
    match read_marketplace(&marketplace_path) {
        Ok(marketplace) => refresh_success_result(
            id,
            "Built-in GitHub marketplace source refreshed",
            marketplace,
        ),
        Err(err) => refresh_error_result(
            id,
            format!(
                "invalid built-in marketplace cache `{}`: {err}",
                marketplace_path.display()
            ),
        ),
    }
}

pub(super) fn refresh_builtin_marketplace_source(id: &str) -> RefreshResult {
    match builtin_marketplace_source() {
        BuiltinMarketplaceSource::Env(path) => refresh_builtin_local_result(id, "env", &path),
        BuiltinMarketplaceSource::Dev(path) => refresh_builtin_local_result(id, "dev", &path),
        BuiltinMarketplaceSource::GithubCache(_) | BuiltinMarketplaceSource::GithubRemote => {
            refresh_builtin_github_result(id)
        }
    }
}

pub fn refresh_user_marketplace_source(id: &str) -> Result<RefreshResult, String> {
    let project_root = default_url_validation_project_root();
    refresh_user_marketplace_source_with_project_root(id, &project_root)
}

pub fn refresh_user_marketplace_source_with_project_root(
    id: &str,
    project_root: &Path,
) -> Result<RefreshResult, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("marketplace source id must not be empty".to_string());
    }
    if id == "builtin" {
        return Ok(refresh_builtin_marketplace_source(id));
    }

    let source = read_config()
        .marketplaces
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| format!("marketplace source `{id}` was not found"))?;

    match source.kind {
        MarketplaceSourceKind::Local => match resolve_user_local_marketplace_path(&source.location)
        {
            Ok(path) => {
                let marketplace = read_marketplace(&path)?;
                Ok(refresh_success_result(
                    id,
                    "Local marketplace source is available",
                    marketplace,
                ))
            }
            Err(err) => Ok(refresh_error_result(id, err)),
        },
        MarketplaceSourceKind::Remote => {
            let remote_url =
                match validate_remote_marketplace_url(&source.location, project_root, true) {
                    Ok(url) => url,
                    Err(err) => return Ok(refresh_error_result(id, err)),
                };
            let cache_dir = user_marketplace_cache_dir(&source.id)?;
            if let Err(err) = clone_or_update_marketplace_repo(&remote_url, &cache_dir) {
                return Ok(refresh_error_result(id, err));
            }
            let marketplace_path = cache_dir.join(MARKETPLACE_FILE_NAME);
            match read_marketplace(&marketplace_path) {
                Ok(marketplace) => Ok(refresh_success_result(
                    id,
                    "Remote marketplace source refreshed",
                    marketplace,
                )),
                Err(err) => Ok(refresh_error_result(
                    id,
                    format!(
                        "invalid remote marketplace `{}`: {err}",
                        marketplace_path.display()
                    ),
                )),
            }
        }
    }
}

pub fn remove_user_marketplace_source(id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("marketplace source id must not be empty".to_string());
    }
    let mut config = read_config();
    let removed = config
        .marketplaces
        .iter()
        .find(|source| source.id == id)
        .cloned();
    let Some(removed_source) = removed.as_ref() else {
        return Err(format!("marketplace source `{id}` was not found"));
    };
    let remote_cache_dir = if removed_source.kind == MarketplaceSourceKind::Remote {
        Some(user_marketplace_cache_dir(&removed_source.id)?)
    } else {
        None
    };
    config.marketplaces.retain(|source| source.id != id);
    write_config(&config)?;
    if let Some(cache_dir) = remote_cache_dir {
        if cache_dir.exists() {
            if let Err(err) = fs::remove_dir_all(&cache_dir) {
                tracing::warn!(
                    source_id = %removed_source.id,
                    path = %cache_dir.display(),
                    "failed to remove remote marketplace cache dir: {err}"
                );
            }
        }
    }
    Ok(())
}

pub fn set_user_marketplace_source_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("marketplace source id must not be empty".to_string());
    }
    let mut config = read_config();
    let Some(source) = config
        .marketplaces
        .iter_mut()
        .find(|source| source.id == id)
    else {
        return Err(format!("marketplace source `{id}` was not found"));
    };
    source.enabled = enabled;
    write_config(&config)
}
pub(super) fn read_marketplace(path: &Path) -> Result<RawMarketplaceManifest, String> {
    let raw = fs::read_to_string(path).map_err(|err| format!("read marketplace: {err}"))?;
    serde_json::from_str(&raw).map_err(|err| format!("parse marketplace: {err}"))
}

pub(super) fn resolve_marketplace_source_path(
    marketplace_path: &Path,
    source: &RawMarketplacePluginSource,
) -> Result<PathBuf, String> {
    if source.source.trim().is_empty() || source.source == "local" {
        let root = marketplace_root_dir(marketplace_path);
        return resolve_safe_relative_path(&root, &source.path, "plugin source path");
    }
    Err(format!("unsupported plugin source `{}`", source.source))
}

pub(super) fn plugin_summary_from_marketplace_entry(
    marketplace_path: &Path,
    marketplace_name: &str,
    entry: &RawMarketplacePlugin,
    config: &PluginConfigFile,
) -> Result<PluginSummary, String> {
    let source_path = resolve_marketplace_source_path(marketplace_path, &entry.source)?;
    let plugin_id = PluginId::new(&entry.name, marketplace_name)?;
    let key = plugin_id.key();
    let installed_path = active_plugin_root(&plugin_id);
    let contribution_root = installed_path.as_deref().unwrap_or(&source_path);
    let source_manifest = load_plugin_manifest(&source_path);
    let installed_manifest = installed_path.as_deref().and_then(load_plugin_manifest);
    let manifest = installed_manifest.as_ref().or(source_manifest.as_ref());
    let retrieval = manifest
        .and_then(|manifest| plugin_retrieval_summary(manifest.retrieval.as_ref(), &key, config));
    let interface = manifest
        .and_then(|manifest| manifest.interface.clone())
        .map(|mut interface| {
            if interface.category.is_none() {
                interface.category = entry.category.clone();
            }
            interface
        });
    let templates = plugin_template_summary(contribution_root, &key, config);
    let operators =
        crate::domain::operators::list_operator_summaries_for_plugin_root(&key, contribution_root);
    let environments = plugin_environment_summaries(contribution_root, &key, config);
    let sync = plugin_sync_summary(&source_path, installed_path.as_deref());
    let changelog = plugin_changelog_summary(&source_path, source_manifest.as_ref());
    Ok(PluginSummary {
        id: key.clone(),
        name: entry.name.clone(),
        marketplace_name: marketplace_name.to_string(),
        marketplace_path: marketplace_path.to_string_lossy().into_owned(),
        source_path: source_path.to_string_lossy().into_owned(),
        installed: installed_path.is_some(),
        installed_path: installed_path.map(|path| path.to_string_lossy().into_owned()),
        enabled: is_plugin_enabled(config, &key),
        install_policy: entry.policy.installation.clone(),
        auth_policy: entry.policy.authentication.clone(),
        interface,
        retrieval,
        operators,
        templates,
        environments,
        sync,
        changelog,
    })
}

pub(super) fn plugin_summary_from_installed_root(
    plugin_id: &PluginId,
    plugin_root: &Path,
    config: &PluginConfigFile,
) -> PluginSummary {
    let manifest = load_plugin_manifest(plugin_root);
    let key = plugin_id.key();
    let retrieval = manifest
        .as_ref()
        .and_then(|manifest| plugin_retrieval_summary(manifest.retrieval.as_ref(), &key, config));
    let templates = plugin_template_summary(plugin_root, &key, config);
    let operators =
        crate::domain::operators::list_operator_summaries_for_plugin_root(&key, plugin_root);
    let environments = plugin_environment_summaries(plugin_root, &key, config);
    let changelog = plugin_changelog_summary(plugin_root, manifest.as_ref());
    let marketplace_path = plugin_root
        .parent()
        .unwrap_or(plugin_root)
        .to_string_lossy()
        .into_owned();
    PluginSummary {
        id: key.clone(),
        name: plugin_id.name.clone(),
        marketplace_name: plugin_id.marketplace.clone(),
        marketplace_path,
        source_path: plugin_root.to_string_lossy().into_owned(),
        installed_path: Some(plugin_root.to_string_lossy().into_owned()),
        installed: true,
        enabled: is_plugin_enabled(config, &key),
        install_policy: PluginInstallPolicy::Available,
        auth_policy: PluginAuthPolicy::OnUse,
        interface: manifest.and_then(|manifest| manifest.interface),
        retrieval,
        operators,
        templates,
        environments,
        sync: None,
        changelog,
    }
}

pub(super) fn cached_plugin_ids_in_cache(cache_root: &Path) -> Vec<PluginId> {
    let mut ids = Vec::new();
    let Ok(marketplaces) = fs::read_dir(cache_root) else {
        return ids;
    };
    for marketplace in marketplaces.flatten() {
        let Ok(file_type) = marketplace.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Ok(marketplace_name) = marketplace.file_name().into_string() else {
            continue;
        };
        let Ok(plugins) = fs::read_dir(marketplace.path()) else {
            continue;
        };
        for plugin in plugins.flatten() {
            let Ok(file_type) = plugin.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let Ok(plugin_name) = plugin.file_name().into_string() else {
                continue;
            };
            if let Ok(plugin_id) = PluginId::new(&plugin_name, &marketplace_name) {
                ids.push(plugin_id);
            }
        }
    }
    ids.sort_by_key(PluginId::key);
    ids.dedup_by(|a, b| a.key() == b.key());
    ids
}

pub(super) fn migrate_legacy_plugin_cache(cache_root: &Path) -> Result<usize, String> {
    let store_root = plugin_store_root_from_cache_root(cache_root);
    let mut migrated = 0;
    for plugin_id in cached_plugin_ids_in_cache(cache_root) {
        let legacy_base = plugin_base_root_in_cache(cache_root, &plugin_id);
        let Some(legacy_root) = active_plugin_root_in_cache(cache_root, &plugin_id) else {
            continue;
        };
        let Some(manifest) = load_plugin_manifest(&legacy_root) else {
            continue;
        };
        let kind = plugin_kind_for_manifest(&legacy_root, None, &manifest);
        let target = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
        if target.exists() {
            remove_path_if_exists(&legacy_base)?;
            continue;
        }
        let parent = target
            .parent()
            .ok_or_else(|| format!("plugin install path has no parent: {}", target.display()))?;
        fs::create_dir_all(parent).map_err(|err| format!("create plugin install dir: {err}"))?;
        fs::rename(&legacy_root, &target).map_err(|err| {
            format!(
                "move legacy plugin `{}` from {} to {}: {err}",
                plugin_id.key(),
                legacy_root.display(),
                target.display()
            )
        })?;
        remove_path_if_exists(&legacy_base)?;
        migrated += 1;
    }
    Ok(migrated)
}

pub(super) fn legacy_plugin_cache_migration_status(
    cache_root: &Path,
) -> Result<LegacyPluginCacheMigrationStatus, String> {
    let store_root = plugin_store_root_from_cache_root(cache_root);
    let mut status = LegacyPluginCacheMigrationStatus::default();
    for plugin_id in cached_plugin_ids_in_cache(cache_root) {
        let Some(legacy_root) = active_plugin_root_in_cache(cache_root, &plugin_id) else {
            continue;
        };
        let Some(manifest) = load_plugin_manifest(&legacy_root) else {
            continue;
        };
        let kind = plugin_kind_for_manifest(&legacy_root, None, &manifest);
        let target = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
        if target.exists() {
            status.entries_to_remove += 1;
        } else {
            status.entries_to_migrate += 1;
        }
    }
    Ok(status)
}

pub(super) fn migrate_legacy_plugin_cache_best_effort(cache_root: &Path) {
    match migrate_legacy_plugin_cache(cache_root) {
        Ok(migrated) if migrated > 0 => {
            tracing::info!(count = migrated, "migrated legacy plugin cache entries")
        }
        Ok(_) => {}
        Err(err) => tracing::warn!("failed to migrate legacy plugin cache entries: {err}"),
    }
}

pub(super) fn refresh_configured_builtin_plugins(
    config: &PluginConfigFile,
    cache_root: &Path,
) -> Result<usize, String> {
    let Some(marketplace_path) = builtin_marketplace_path() else {
        return Ok(0);
    };
    let marketplace = read_marketplace(&marketplace_path)?;
    let store_root = plugin_store_root_from_cache_root(cache_root);
    let mut refreshed = 0;

    for entry in &marketplace.plugins {
        let plugin_id = PluginId::new(&entry.name, &marketplace.name)?;
        if !config.plugins.contains_key(&plugin_id.key()) {
            continue;
        }
        if entry.policy.installation == PluginInstallPolicy::NotAvailable {
            continue;
        }
        let source_path = resolve_marketplace_source_path(&marketplace_path, &entry.source)?;
        let Some(manifest) = load_plugin_manifest(&source_path) else {
            continue;
        };
        if manifest.name != entry.name {
            continue;
        }
        let kind = plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest);
        let target_base = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
        if migrate_stale_typed_plugin_root(&store_root, &plugin_id, &target_base)? {
            refreshed += 1;
        }
    }

    Ok(refreshed)
}

pub(super) fn configured_builtin_plugins_to_refresh(
    config: &PluginConfigFile,
    cache_root: &Path,
) -> Result<usize, String> {
    let Some(marketplace_path) = builtin_marketplace_path() else {
        return Ok(0);
    };
    let marketplace = read_marketplace(&marketplace_path)?;
    let store_root = plugin_store_root_from_cache_root(cache_root);
    let mut to_refresh = 0;

    for entry in &marketplace.plugins {
        let plugin_id = PluginId::new(&entry.name, &marketplace.name)?;
        if !config.plugins.contains_key(&plugin_id.key()) {
            continue;
        }
        if entry.policy.installation == PluginInstallPolicy::NotAvailable {
            continue;
        }
        let source_path = resolve_marketplace_source_path(&marketplace_path, &entry.source)?;
        let Some(manifest) = load_plugin_manifest(&source_path) else {
            continue;
        };
        if manifest.name != entry.name {
            continue;
        }
        let kind = plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest);
        let target_base = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
        if stale_typed_plugin_root_candidate(&store_root, &plugin_id, &target_base).is_some() {
            to_refresh += 1;
        }
    }

    Ok(to_refresh)
}

pub(super) fn migrate_stale_typed_plugin_root(
    store_root: &Path,
    plugin_id: &PluginId,
    target_base: &Path,
) -> Result<bool, String> {
    // This is a typed-root migration only. It intentionally moves the user's
    // existing installed plugin tree instead of copying marketplace source
    // content, so read/list paths never bypass explicit sync conflict checks.
    let Some(stale_base) = stale_typed_plugin_root_candidate(store_root, plugin_id, target_base)
    else {
        return Ok(false);
    };
    let parent = target_base.parent().ok_or_else(|| {
        format!(
            "plugin install path has no parent: {}",
            target_base.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|err| format!("create plugin install dir: {err}"))?;
    fs::rename(&stale_base, target_base).map_err(|err| {
        format!(
            "move configured plugin `{}` from {} to {}: {err}",
            plugin_id.key(),
            stale_base.display(),
            target_base.display()
        )
    })?;
    Ok(true)
}

pub(super) fn stale_typed_plugin_root_candidate(
    store_root: &Path,
    plugin_id: &PluginId,
    target_base: &Path,
) -> Option<PathBuf> {
    if target_base.exists() {
        return None;
    }
    typed_plugin_base_roots(store_root, plugin_id)
        .into_iter()
        .filter(|candidate| candidate != target_base && candidate.exists())
        .find(|candidate| active_plugin_root_in_base(candidate).is_some())
}

pub(super) fn unlisted_installed_plugin_summaries(
    config: &PluginConfigFile,
    listed_ids: &HashSet<String>,
    cache_root: &Path,
) -> Vec<PluginSummary> {
    migrate_legacy_plugin_cache_best_effort(cache_root);
    configured_plugin_ids(config)
        .into_iter()
        .filter_map(|plugin_id| {
            let key = plugin_id.key();
            if listed_ids.contains(&key) {
                return None;
            }
            let plugin_root = active_plugin_root_from_roots(cache_root, &plugin_id)?;
            Some(plugin_summary_from_installed_root(
                &plugin_id,
                &plugin_root,
                config,
            ))
        })
        .collect()
}

pub fn list_plugin_marketplaces(
    project_root: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Vec<PluginMarketplaceEntry> {
    let cache_root = plugin_cache_root();
    migrate_legacy_plugin_cache_best_effort(&cache_root);
    let config = read_config();
    let mut out = Vec::new();
    let mut listed_ids = HashSet::new();
    for path in marketplace_paths(project_root, resource_dir) {
        let marketplace = match read_marketplace(&path) {
            Ok(marketplace) => marketplace,
            Err(err) => {
                tracing::warn!(path = %path.display(), "skipping plugin marketplace: {err}");
                continue;
            }
        };
        let mut plugins = Vec::new();
        for entry in &marketplace.plugins {
            match plugin_summary_from_marketplace_entry(&path, &marketplace.name, entry, &config) {
                Ok(summary) => {
                    if !listed_ids.insert(summary.id.clone()) {
                        continue;
                    }
                    plugins.push(summary);
                }
                Err(err) => {
                    tracing::warn!(path = %path.display(), plugin = entry.name, "skipping plugin: {err}")
                }
            }
        }
        if plugins.is_empty() && marketplace.remote.is_none() {
            continue;
        }
        out.push(PluginMarketplaceEntry {
            name: marketplace.name,
            path: path.to_string_lossy().into_owned(),
            interface: marketplace.interface,
            remote: marketplace.remote,
            plugins,
        });
    }
    let installed_plugins = unlisted_installed_plugin_summaries(&config, &listed_ids, &cache_root);
    if !installed_plugins.is_empty() {
        out.push(PluginMarketplaceEntry {
            name: "installed-plugins".to_string(),
            path: plugin_store_root_from_cache_root(&cache_root)
                .to_string_lossy()
                .into_owned(),
            interface: Some(MarketplaceInterface {
                display_name: Some("Installed plugins".to_string()),
            }),
            remote: None,
            plugins: installed_plugins,
        });
    }
    out
}

pub(super) fn marketplace_raw_digest(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|bytes| format!("sha256:{}", sha256_hex(&bytes)))
}

pub(super) fn marketplace_entry_versions(
    marketplace_path: &Path,
    marketplace: &RawMarketplaceManifest,
) -> BTreeMap<String, Option<String>> {
    let mut versions = BTreeMap::new();
    for entry in &marketplace.plugins {
        let version = entry.version.clone().or_else(|| {
            resolve_marketplace_source_path(marketplace_path, &entry.source)
                .ok()
                .and_then(|source_path| load_plugin_manifest(&source_path))
                .and_then(|manifest| manifest.version)
        });
        versions.insert(entry.name.clone(), version);
    }
    versions
}

pub(super) fn remote_marketplace_versions(
    marketplace: &RawMarketplaceManifest,
) -> BTreeMap<String, Option<String>> {
    marketplace
        .plugins
        .iter()
        .map(|entry| (entry.name.clone(), entry.version.clone()))
        .collect()
}

pub(crate) fn changed_marketplace_plugins(
    local_versions: &BTreeMap<String, Option<String>>,
    remote_versions: &BTreeMap<String, Option<String>>,
) -> Vec<String> {
    let mut names = BTreeSet::new();
    names.extend(local_versions.keys().cloned());
    names.extend(remote_versions.keys().cloned());
    names
        .into_iter()
        .filter(
            |name| match (local_versions.get(name), remote_versions.get(name)) {
                (None, Some(_)) | (Some(_), None) => true,
                (Some(local), Some(remote)) => match (local, remote) {
                    (Some(local), Some(remote)) => local != remote,
                    (None, Some(_)) => true,
                    // A remote marketplace without per-plugin versions can still
                    // signal manifest-level changes via digest; do not mark every
                    // existing plugin as changed just because the remote omitted
                    // optional version metadata.
                    _ => false,
                },
                (None, None) => false,
            },
        )
        .collect()
}

pub(super) fn marketplace_check_error(
    path: &Path,
    marketplace: RawMarketplaceManifest,
    remote: MarketplaceRemote,
    local_digest: Option<String>,
    message: String,
) -> MarketplaceRemoteCheckResult {
    MarketplaceRemoteCheckResult {
        name: marketplace.name,
        path: path.to_string_lossy().into_owned(),
        remote,
        state: "error".to_string(),
        label: "Remote check failed".to_string(),
        message,
        local_digest,
        remote_digest: None,
        remote_plugin_count: None,
        changed_plugins: Vec::new(),
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub(super) fn redirect_target_allowed(project_root: &Path, url: &str) -> bool {
    crate::domain::tools::web_safety::validate_public_http_url(project_root, url, true).is_ok()
}

async fn check_one_remote_marketplace(
    path: &Path,
    marketplace: RawMarketplaceManifest,
    project_root: &Path,
    client: &reqwest::Client,
) -> MarketplaceRemoteCheckResult {
    let Some(remote) = marketplace.remote.clone() else {
        unreachable!("caller filters marketplaces without remote metadata");
    };
    let local_digest = marketplace_raw_digest(path);
    if let Err(err) =
        crate::domain::tools::web_safety::validate_public_http_url(project_root, &remote.url, true)
    {
        return marketplace_check_error(
            path,
            marketplace,
            remote,
            local_digest,
            format!("Remote marketplace URL is not allowed: {err}"),
        );
    }

    let response = match client
        .get(&remote.url)
        .header(reqwest::header::USER_AGENT, "Omiga")
        .header(
            reqwest::header::ACCEPT,
            "application/json,text/plain;q=0.9,*/*;q=0.1",
        )
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            return marketplace_check_error(
                path,
                marketplace,
                remote,
                local_digest,
                format!("Remote marketplace request failed: {err}"),
            );
        }
    };
    let status = response.status();
    if !status.is_success() {
        return marketplace_check_error(
            path,
            marketplace,
            remote,
            local_digest,
            format!("Remote marketplace returned HTTP {status}."),
        );
    }
    if let Some(length) = response.content_length() {
        if length as usize > MAX_REMOTE_MARKETPLACE_BYTES {
            return marketplace_check_error(
                path,
                marketplace,
                remote,
                local_digest,
                format!("Remote marketplace is too large: {length} bytes."),
            );
        }
    }
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(err) => {
            return marketplace_check_error(
                path,
                marketplace,
                remote,
                local_digest,
                format!("Read remote marketplace body failed: {err}"),
            );
        }
    };
    if bytes.len() > MAX_REMOTE_MARKETPLACE_BYTES {
        return marketplace_check_error(
            path,
            marketplace,
            remote,
            local_digest,
            format!("Remote marketplace is too large: {} bytes.", bytes.len()),
        );
    }
    let remote_digest = format!("sha256:{}", sha256_hex(&bytes));
    let remote_marketplace = match serde_json::from_slice::<RawMarketplaceManifest>(&bytes) {
        Ok(remote_marketplace) => remote_marketplace,
        Err(err) => {
            return marketplace_check_error(
                path,
                marketplace,
                remote,
                local_digest,
                format!("Parse remote marketplace failed: {err}"),
            );
        }
    };
    if remote_marketplace.name != marketplace.name {
        let local_name = marketplace.name.clone();
        return MarketplaceRemoteCheckResult {
            name: local_name.clone(),
            path: path.to_string_lossy().into_owned(),
            remote,
            state: "error".to_string(),
            label: "Remote mismatch".to_string(),
            message: format!(
                "Remote marketplace name `{}` does not match local `{}`.",
                remote_marketplace.name, local_name
            ),
            local_digest,
            remote_digest: Some(remote_digest),
            remote_plugin_count: Some(remote_marketplace.plugins.len()),
            changed_plugins: Vec::new(),
            checked_at: chrono::Utc::now().to_rfc3339(),
        };
    }

    let changed_plugins = changed_marketplace_plugins(
        &marketplace_entry_versions(path, &marketplace),
        &remote_marketplace_versions(&remote_marketplace),
    );
    let digest_changed = local_digest
        .as_ref()
        .map(|digest| digest != &remote_digest)
        .unwrap_or(true);
    let update_available = digest_changed || !changed_plugins.is_empty();
    let (state, label, message) = if update_available {
        (
            "updateAvailable",
            "Remote update available",
            if changed_plugins.is_empty() {
                "Remote marketplace manifest differs from the local copy.".to_string()
            } else {
                format!(
                    "Remote marketplace differs for {} plugin{}.",
                    changed_plugins.len(),
                    if changed_plugins.len() == 1 { "" } else { "s" }
                )
            },
        )
    } else {
        (
            "upToDate",
            "Remote up to date",
            "Remote marketplace manifest matches the local copy.".to_string(),
        )
    };

    MarketplaceRemoteCheckResult {
        name: marketplace.name,
        path: path.to_string_lossy().into_owned(),
        remote,
        state: state.to_string(),
        label: label.to_string(),
        message,
        local_digest,
        remote_digest: Some(remote_digest),
        remote_plugin_count: Some(remote_marketplace.plugins.len()),
        changed_plugins,
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub async fn check_remote_plugin_marketplaces(
    project_root: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Vec<MarketplaceRemoteCheckResult> {
    let policy_root = project_root
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let redirect_policy_root = policy_root.clone();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.stop();
            }
            if redirect_target_allowed(&redirect_policy_root, attempt.url().as_str()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!("failed to create remote marketplace client: {err}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for path in marketplace_paths(project_root, resource_dir) {
        let marketplace = match read_marketplace(&path) {
            Ok(marketplace) => marketplace,
            Err(err) => {
                tracing::warn!(path = %path.display(), "skipping remote marketplace check: {err}");
                continue;
            }
        };
        if marketplace.remote.is_none() {
            continue;
        }
        out.push(check_one_remote_marketplace(&path, marketplace, &policy_root, &client).await);
    }
    out
}

pub fn read_plugin(marketplace_path: &Path, plugin_name: &str) -> Result<PluginDetail, String> {
    migrate_legacy_plugin_cache_best_effort(&plugin_cache_root());
    let marketplace = read_marketplace(marketplace_path)?;
    let config = read_config();
    let entry = marketplace
        .plugins
        .iter()
        .find(|entry| entry.name == plugin_name)
        .ok_or_else(|| format!("plugin `{plugin_name}` not found in `{}`", marketplace.name))?;
    let summary =
        plugin_summary_from_marketplace_entry(marketplace_path, &marketplace.name, entry, &config)?;
    let source_path = PathBuf::from(&summary.source_path);
    let manifest = load_plugin_manifest(&source_path)
        .ok_or_else(|| "missing or invalid plugin manifest".to_string())?;
    Ok(PluginDetail {
        summary,
        description: manifest.description.clone(),
        changelog: plugin_changelog_summary(&source_path, Some(&manifest)),
        skills: plugin_skill_summaries(&source_path, &manifest),
        mcp_servers: plugin_mcp_server_names(&source_path, &manifest),
        apps: plugin_app_ids(&source_path, &manifest),
    })
}
pub(crate) fn copy_marketplace_resource_runner_assets(
    marketplace_path: &Path,
    _marketplace_name: &str,
    cache_root: &Path,
) -> Result<bool, String> {
    let marketplace_root = marketplace_root_dir(marketplace_path);
    let canonical_source = marketplace_root.join(RESOURCE_RUNNERS_DIR);
    let legacy_source = marketplace_root.join(LEGACY_SOURCE_RUNNERS_DIR);
    let source = if canonical_source.is_dir() {
        canonical_source
    } else {
        legacy_source
    };
    if !source.is_dir() {
        return Ok(false);
    }
    let target = plugin_store_root_from_cache_root(cache_root)
        .join(PluginKind::Resource.dir_name())
        .join(RESOURCE_RUNNERS_DIR);
    copy_dir_recursive(&source, &target)?;
    let legacy_target = plugin_store_root_from_cache_root(cache_root)
        .join(PluginKind::Resource.dir_name())
        .join(LEGACY_SOURCE_RUNNERS_DIR);
    copy_dir_recursive(&source, &legacy_target)?;
    Ok(true)
}

pub(crate) fn copy_marketplace_nested_resource_utils(
    marketplace_path: &Path,
    cache_root: &Path,
) -> Result<bool, String> {
    let marketplace_root = marketplace_root_dir(marketplace_path);
    let source = marketplace_root
        .join("plugins")
        .join(PluginKind::Resource.dir_name())
        .join(RESOURCE_UTILS_DIR);
    if !source.is_dir() {
        return Ok(false);
    }
    let target = plugin_store_root_from_cache_root(cache_root)
        .join(PluginKind::Resource.dir_name())
        .join(RESOURCE_UTILS_DIR);
    copy_dir_recursive(&source, &target)?;
    Ok(true)
}

pub(crate) fn copy_marketplace_shared_resource_assets(
    marketplace_path: &Path,
    marketplace_name: &str,
    cache_root: &Path,
) -> Result<bool, String> {
    let copied_legacy =
        copy_marketplace_resource_runner_assets(marketplace_path, marketplace_name, cache_root)?;
    let copied_utils = copy_marketplace_nested_resource_utils(marketplace_path, cache_root)?;
    Ok(copied_legacy || copied_utils)
}
