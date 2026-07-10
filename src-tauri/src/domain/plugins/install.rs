use super::*;

pub const PLUGIN_INSTALL_STATE_RELATIVE_PATH: &str = ".omiga-plugin/install-state.json";
pub const PLUGIN_SYNC_CONFLICTS_RELATIVE_DIR: &str = ".omiga-plugin/sync-conflicts";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSyncSummary {
    pub state: String,
    pub label: String,
    pub message: String,
    pub source_digest: Option<String>,
    pub installed_digest: Option<String>,
    pub installed_from_digest: Option<String>,
    pub changed_count: usize,
    pub local_modified_count: usize,
    pub conflict_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginSyncResult {
    pub plugin_id: String,
    pub status: String,
    pub installed_path: String,
    pub updated: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub kept_local: Vec<String>,
    pub conflicts: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginInstallState {
    #[serde(default = "plugin_install_state_schema_version")]
    pub(crate) schema_version: u32,
    pub(crate) plugin_id: String,
    pub(crate) installed_from_version: Option<String>,
    pub(crate) installed_from_digest: String,
    pub(crate) installed_at: String,
    pub(crate) last_synced_at: String,
    #[serde(default)]
    pub(crate) files: BTreeMap<String, String>,
}

fn plugin_install_state_schema_version() -> u32 {
    1
}

pub(crate) fn plugin_install_state_path(plugin_root: &Path) -> PathBuf {
    plugin_root.join(PLUGIN_INSTALL_STATE_RELATIVE_PATH)
}

pub(crate) fn plugin_relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let rel = path
        .strip_prefix(root)
        .map_err(|err| format!("derive plugin relative path: {err}"))?;
    Ok(rel
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/"))
}

pub(crate) fn plugin_sync_internal_path(relative: &str) -> bool {
    relative == PLUGIN_INSTALL_STATE_RELATIVE_PATH
        || relative.starts_with(&format!("{PLUGIN_SYNC_CONFLICTS_RELATIVE_DIR}/"))
}

pub(crate) fn plugin_file_hashes(plugin_root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    if !plugin_root.is_dir() {
        return Ok(files);
    }
    for entry in walkdir::WalkDir::new(plugin_root)
        .follow_links(false)
        .into_iter()
    {
        let entry = entry.map_err(|err| format!("walk plugin files: {err}"))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = plugin_relative_path(plugin_root, entry.path())?;
        if relative.is_empty() || plugin_sync_internal_path(&relative) {
            continue;
        }
        let bytes = fs::read(entry.path()).map_err(|err| {
            format!(
                "read plugin file `{}` for digest: {err}",
                entry.path().display()
            )
        })?;
        files.insert(relative, format!("sha256:{}", sha256_hex(&bytes)));
    }
    Ok(files)
}

pub(crate) fn plugin_tree_digest(files: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (relative, hash) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(hash.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{:x}", hasher.finalize())
}

pub(crate) fn read_plugin_install_state(plugin_root: &Path) -> Option<PluginInstallState> {
    fs::read_to_string(plugin_install_state_path(plugin_root))
        .ok()
        .and_then(|raw| serde_json::from_str::<PluginInstallState>(&raw).ok())
}

pub(crate) fn write_plugin_install_state(
    plugin_root: &Path,
    state: &PluginInstallState,
) -> Result<(), String> {
    let path = plugin_install_state_path(plugin_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create plugin state dir: {err}"))?;
    }
    let raw = serde_json::to_string_pretty(state)
        .map_err(|err| format!("serialize plugin install state: {err}"))?;
    fs::write(&path, format!("{raw}\n")).map_err(|err| format!("write plugin state: {err}"))
}

pub(crate) fn plugin_install_state_conflict_error(
    requested_plugin_id: &PluginId,
    existing_plugin_id: &str,
    plugin_root: &Path,
) -> String {
    format!(
        "cross-marketplace plugin install conflict for `{}`: typed plugin root `{}` is already owned by `{}`. Plugins with the same name currently share this typed install root; uninstall the old marketplace source first or wait for a future migration before installing or syncing `{}`.",
        requested_plugin_id.key(),
        plugin_root.display(),
        existing_plugin_id,
        requested_plugin_id.key()
    )
}

pub(crate) fn ensure_plugin_install_state_matches(
    plugin_root: &Path,
    plugin_id: &PluginId,
) -> Result<(), String> {
    let Some(state) = read_plugin_install_state(plugin_root) else {
        return Ok(());
    };
    if state.plugin_id == plugin_id.key() {
        return Ok(());
    }
    Err(plugin_install_state_conflict_error(
        plugin_id,
        &state.plugin_id,
        plugin_root,
    ))
}

pub(crate) fn record_plugin_install_state(
    plugin_root: &Path,
    plugin_id: &PluginId,
    version: Option<String>,
    installed_at: Option<String>,
) -> Result<PluginInstallState, String> {
    let files = plugin_file_hashes(plugin_root)?;
    let now = chrono::Utc::now().to_rfc3339();
    let installed_at = installed_at.unwrap_or_else(|| now.clone());
    let state = PluginInstallState {
        schema_version: plugin_install_state_schema_version(),
        plugin_id: plugin_id.key(),
        installed_from_version: version,
        installed_from_digest: plugin_tree_digest(&files),
        installed_at,
        last_synced_at: now,
        files,
    };
    write_plugin_install_state(plugin_root, &state)?;
    Ok(state)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PluginSyncPlan {
    pub(crate) updated: Vec<String>,
    pub(crate) added: Vec<String>,
    pub(crate) removed: Vec<String>,
    pub(crate) kept_local: Vec<String>,
    pub(crate) conflicts: Vec<String>,
}

impl PluginSyncPlan {
    fn changed_count(&self) -> usize {
        self.updated.len() + self.added.len() + self.removed.len()
    }

    fn local_modified_count(&self) -> usize {
        self.kept_local.len() + self.conflicts.len()
    }
}

pub(crate) fn plugin_sync_plan(
    base_files: Option<&BTreeMap<String, String>>,
    current_files: &BTreeMap<String, String>,
    source_files: &BTreeMap<String, String>,
) -> PluginSyncPlan {
    let mut plan = PluginSyncPlan::default();
    let mut paths = BTreeSet::new();
    paths.extend(current_files.keys().cloned());
    paths.extend(source_files.keys().cloned());
    if let Some(base_files) = base_files {
        paths.extend(base_files.keys().cloned());
    }

    for path in paths {
        let base = base_files.and_then(|files| files.get(&path));
        let current = current_files.get(&path);
        let source = source_files.get(&path);

        if current == source {
            continue;
        }

        match base {
            Some(base_hash) => {
                if current == Some(base_hash) {
                    match source {
                        Some(_) => plan.updated.push(path),
                        None => plan.removed.push(path),
                    }
                } else if source == Some(base_hash) {
                    plan.kept_local.push(path);
                } else {
                    plan.conflicts.push(path);
                }
            }
            None => match (current, source) {
                (None, Some(_)) => plan.added.push(path),
                (Some(_), None) => plan.kept_local.push(path),
                (Some(_), Some(_)) => plan.conflicts.push(path),
                (None, None) => {}
            },
        }
    }

    plan
}

pub(crate) fn plugin_force_sync_plan(
    current_files: &BTreeMap<String, String>,
    source_files: &BTreeMap<String, String>,
) -> PluginSyncPlan {
    let mut plan = PluginSyncPlan::default();
    let mut paths = BTreeSet::new();
    paths.extend(current_files.keys().cloned());
    paths.extend(source_files.keys().cloned());
    for path in paths {
        match (current_files.get(&path), source_files.get(&path)) {
            (Some(current), Some(source)) if current != source => plan.updated.push(path),
            (None, Some(_)) => plan.added.push(path),
            (Some(_), None) => plan.removed.push(path),
            _ => {}
        }
    }
    plan
}

pub(crate) fn plugin_sync_summary(
    source_path: &Path,
    installed_path: Option<&Path>,
) -> Option<PluginSyncSummary> {
    let installed_path = installed_path?;
    let source_files = plugin_file_hashes(source_path).ok()?;
    let current_files = plugin_file_hashes(installed_path).ok()?;
    let source_digest = plugin_tree_digest(&source_files);
    let installed_digest = plugin_tree_digest(&current_files);
    let state = read_plugin_install_state(installed_path);
    let base_files = state.as_ref().map(|state| &state.files);
    let plan = plugin_sync_plan(base_files, &current_files, &source_files);
    let installed_from_digest = state
        .as_ref()
        .map(|state| state.installed_from_digest.clone());
    let upstream_changed = installed_from_digest
        .as_ref()
        .map(|digest| digest != &source_digest)
        .unwrap_or(installed_digest != source_digest);
    let local_modified = state
        .as_ref()
        .map(|state| state.files != current_files)
        .unwrap_or(installed_digest != source_digest);
    let (state_name, label, message) = if plan.conflicts.is_empty()
        && plan.changed_count() == 0
        && !local_modified
    {
        (
            "upToDate",
            "Up to date",
            "Installed plugin files match the marketplace source.",
        )
    } else if state.is_none() && !upstream_changed && !local_modified {
        (
            "unknown",
            "Track sync",
            "Installed plugin matches the marketplace source but has no install-state snapshot yet.",
        )
    } else if plan.conflicts.is_empty() && upstream_changed && !local_modified {
        (
            "updateAvailable",
            "Update available",
            "Marketplace source changed; safe sync can update the user copy.",
        )
    } else if plan.conflicts.is_empty() && !upstream_changed && local_modified {
        (
            "localModified",
            "Local edits",
            "User plugin files differ from the last installed snapshot.",
        )
    } else if plan.conflicts.is_empty() {
        (
            "updateAvailable",
            "Sync available",
            "Safe sync can apply non-conflicting marketplace changes.",
        )
    } else {
        (
            "conflictRisk",
            "Review sync",
            "Marketplace and user plugin files changed in overlapping paths; safe sync will keep local files.",
        )
    };

    Some(PluginSyncSummary {
        state: state_name.to_string(),
        label: label.to_string(),
        message: message.to_string(),
        source_digest: Some(source_digest),
        installed_digest: Some(installed_digest),
        installed_from_digest,
        changed_count: plan.changed_count(),
        local_modified_count: plan.local_modified_count(),
        conflict_count: plan.conflicts.len(),
    })
}

pub(crate) fn copy_plugin_relative_file(
    source_root: &Path,
    target_root: &Path,
    relative: &str,
) -> Result<(), String> {
    let source = source_root.join(relative);
    let target = target_root.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create plugin sync dir: {err}"))?;
    }
    fs::copy(&source, &target)
        .map(|_| ())
        .map_err(|err| format!("copy synced plugin file `{relative}`: {err}"))
}

pub(crate) fn remove_plugin_relative_file(
    target_root: &Path,
    relative: &str,
) -> Result<(), String> {
    let target = target_root.join(relative);
    if !target.exists() {
        return Ok(());
    }
    fs::remove_file(&target).map_err(|err| format!("remove synced plugin file `{relative}`: {err}"))
}

pub(crate) fn prepare_plugin_root_install(
    source: &Path,
    target_base: &Path,
    plugin_id: &PluginId,
    version: Option<String>,
    installed_at: Option<String>,
) -> Result<PathBuf, String> {
    if let Some(existing_root) = active_plugin_root_in_base(target_base) {
        ensure_plugin_install_state_matches(&existing_root, plugin_id)?;
    }
    let parent = target_base.parent().ok_or_else(|| {
        format!(
            "plugin install path has no parent: {}",
            target_base.display()
        )
    })?;
    fs::create_dir_all(parent).map_err(|err| format!("create plugin install dir: {err}"))?;
    let staged_base = parent.join(format!(".install-{}", uuid::Uuid::new_v4()));
    copy_dir_recursive(source, &staged_base)?;
    record_plugin_install_state(&staged_base, plugin_id, version, installed_at).inspect_err(
        |_| {
            let _ = remove_path_if_exists(&staged_base);
        },
    )?;
    Ok(staged_base)
}

pub(crate) fn activate_staged_plugin_root(
    staged_base: &Path,
    target_base: &Path,
) -> Result<Option<PathBuf>, String> {
    let parent = target_base.parent().ok_or_else(|| {
        format!(
            "plugin install path has no parent: {}",
            target_base.display()
        )
    })?;
    let backup_base = parent.join(format!(".install-backup-{}", uuid::Uuid::new_v4()));
    if target_base.exists() {
        fs::rename(target_base, &backup_base).map_err(|err| {
            let _ = remove_path_if_exists(&staged_base);
            format!(
                "stage existing plugin install {} for replacement: {}",
                target_base.display(),
                err
            )
        })?;
    }
    if let Err(err) = fs::rename(&staged_base, target_base) {
        let rollback = if backup_base.exists() {
            match fs::rename(&backup_base, target_base) {
                Ok(()) => "existing install restored".to_string(),
                Err(rollback_err) => format!("restore failed: {rollback_err}"),
            }
        } else {
            "no existing install to restore".to_string()
        };
        let _ = remove_path_if_exists(&staged_base);
        return Err(format!("activate plugin install entry: {err}; {rollback}"));
    }
    Ok(backup_base.exists().then_some(backup_base))
}

pub(crate) fn rollback_activated_plugin_root(
    target_base: &Path,
    backup_base: Option<&Path>,
) -> Result<(), String> {
    remove_path_if_exists(target_base)?;
    if let Some(backup_base) = backup_base {
        if backup_base.exists() {
            fs::rename(backup_base, target_base)
                .map_err(|err| format!("restore plugin install backup: {err}"))?;
        }
    }
    Ok(())
}

pub(crate) fn rollback_after_plugin_enable_failure(
    target_base: &Path,
    backup_base: Option<&Path>,
    primary_err: String,
) -> String {
    match rollback_activated_plugin_root(target_base, backup_base) {
        Ok(()) => primary_err,
        Err(rollback_err) => {
            tracing::warn!(
                path = %target_base.display(),
                "failed to roll back activated plugin install after enable error: {rollback_err}"
            );
            format!(
                "{primary_err}; additionally failed to roll back activated plugin install: {rollback_err}"
            )
        }
    }
}

pub(crate) fn cleanup_plugin_root_backup_best_effort(backup_base: Option<PathBuf>) {
    let Some(backup_base) = backup_base else {
        return;
    };
    if let Err(err) = remove_path_if_exists(&backup_base) {
        tracing::warn!(
            path = %backup_base.display(),
            "failed to remove plugin install backup after successful activation: {err}"
        );
    }
}

pub(crate) fn remove_other_typed_plugin_roots_best_effort(
    store_root: &Path,
    plugin_id: &PluginId,
    keep_base: &Path,
) {
    if let Err(err) = remove_other_typed_plugin_roots(store_root, plugin_id, keep_base) {
        tracing::warn!(
            plugin_id = %plugin_id.key(),
            keep = %keep_base.display(),
            "failed to remove stale typed plugin roots after activation: {err}"
        );
    }
}

pub(crate) fn remove_other_typed_plugin_roots(
    store_root: &Path,
    plugin_id: &PluginId,
    keep_base: &Path,
) -> Result<(), String> {
    for candidate in typed_plugin_base_roots(store_root, plugin_id) {
        if candidate == keep_base {
            continue;
        }
        remove_path_if_exists(&candidate)?;
    }
    Ok(())
}

pub fn install_plugin(
    marketplace_path: &Path,
    plugin_name: &str,
) -> Result<PluginInstallResult, String> {
    let marketplace = read_marketplace(marketplace_path)?;
    let entry = marketplace
        .plugins
        .iter()
        .find(|entry| entry.name == plugin_name)
        .ok_or_else(|| format!("plugin `{plugin_name}` not found in `{}`", marketplace.name))?;
    if entry.policy.installation == PluginInstallPolicy::NotAvailable {
        return Err(format!(
            "plugin `{plugin_name}` is not available for install"
        ));
    }
    let source_path = resolve_marketplace_source_path(marketplace_path, &entry.source)?;
    if !source_path.is_dir() {
        return Err(format!(
            "plugin source path is not a directory: {}",
            source_path.display()
        ));
    }
    let manifest = load_plugin_manifest(&source_path)
        .ok_or_else(|| "missing or invalid plugin manifest".to_string())?;
    if manifest.name != entry.name {
        return Err(format!(
            "plugin manifest name `{}` does not match marketplace plugin name `{}`",
            manifest.name, entry.name
        ));
    }
    let plugin_id = PluginId::new(&entry.name, &marketplace.name)?;
    let kind = plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest);
    let store_root = plugin_store_root();
    let target_base = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
    copy_marketplace_shared_resource_assets(
        marketplace_path,
        &marketplace.name,
        &plugin_cache_root(),
    )?;
    let staged_path = prepare_plugin_root_install(
        &source_path,
        &target_base,
        &plugin_id,
        manifest.version.clone(),
        None,
    )?;
    let backup_path = activate_staged_plugin_root(&staged_path, &target_base)?;
    if let Err(err) = set_plugin_enabled(&plugin_id.key(), true) {
        return Err(rollback_after_plugin_enable_failure(
            &target_base,
            backup_path.as_deref(),
            err,
        ));
    }
    cleanup_plugin_root_backup_best_effort(backup_path);
    remove_other_typed_plugin_roots_best_effort(&store_root, &plugin_id, &target_base);
    Ok(PluginInstallResult {
        plugin_id: plugin_id.key(),
        installed_path: target_base.to_string_lossy().into_owned(),
        auth_policy: entry.policy.authentication.clone(),
    })
}

pub fn sync_plugin(
    plugin_id: &str,
    marketplace_path: &Path,
    plugin_name: Option<&str>,
    force: bool,
) -> Result<PluginSyncResult, String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    let marketplace = read_marketplace(marketplace_path)?;
    if marketplace.name != plugin_id.marketplace {
        return Err(format!(
            "plugin `{}` belongs to marketplace `{}`, not `{}`",
            plugin_id.key(),
            plugin_id.marketplace,
            marketplace.name
        ));
    }
    let plugin_name = plugin_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&plugin_id.name);
    let entry = marketplace
        .plugins
        .iter()
        .find(|entry| entry.name == plugin_name)
        .ok_or_else(|| format!("plugin `{plugin_name}` not found in `{}`", marketplace.name))?;
    if entry.name != plugin_id.name {
        return Err(format!(
            "plugin id `{}` does not match marketplace entry `{}`",
            plugin_id.name, entry.name
        ));
    }
    let source_path = resolve_marketplace_source_path(marketplace_path, &entry.source)?;
    if !source_path.is_dir() {
        return Err(format!(
            "plugin source path is not a directory: {}",
            source_path.display()
        ));
    }
    let manifest = load_plugin_manifest(&source_path)
        .ok_or_else(|| "missing or invalid plugin manifest".to_string())?;
    if manifest.name != entry.name {
        return Err(format!(
            "plugin manifest name `{}` does not match marketplace plugin name `{}`",
            manifest.name, entry.name
        ));
    }
    let installed_path = active_plugin_root(&plugin_id)
        .ok_or_else(|| format!("plugin `{}` is not installed", plugin_id.key()))?;
    ensure_plugin_install_state_matches(&installed_path, &plugin_id)?;

    let source_files = plugin_file_hashes(&source_path)?;
    let current_files = plugin_file_hashes(&installed_path)?;
    let install_state = read_plugin_install_state(&installed_path);
    let base_files = install_state.as_ref().map(|state| &state.files);
    let plan = plugin_sync_plan(base_files, &current_files, &source_files);

    if force {
        let force_plan = plugin_force_sync_plan(&current_files, &source_files);
        let kind = plugin_kind_for_manifest(&source_path, entry.category.as_deref(), &manifest);
        let store_root = plugin_store_root();
        let target_base = plugin_base_root_for_kind(&store_root, kind, &plugin_id);
        copy_marketplace_shared_resource_assets(
            marketplace_path,
            &marketplace.name,
            &plugin_cache_root(),
        )?;
        let installed_at = install_state
            .as_ref()
            .map(|state| state.installed_at.clone());
        let staged_path = prepare_plugin_root_install(
            &source_path,
            &target_base,
            &plugin_id,
            manifest.version.clone(),
            installed_at,
        )?;
        let backup_path = activate_staged_plugin_root(&staged_path, &target_base)?;
        cleanup_plugin_root_backup_best_effort(backup_path);
        remove_other_typed_plugin_roots_best_effort(&store_root, &plugin_id, &target_base);
        return Ok(PluginSyncResult {
            plugin_id: plugin_id.key(),
            status: "forceSynced".to_string(),
            installed_path: target_base.to_string_lossy().into_owned(),
            updated: force_plan.updated,
            added: force_plan.added,
            removed: force_plan.removed,
            kept_local: Vec::new(),
            conflicts: Vec::new(),
            message: "Force synced plugin from marketplace source; local edits were overwritten."
                .to_string(),
        });
    }

    for relative in plan.updated.iter().chain(plan.added.iter()) {
        copy_plugin_relative_file(&source_path, &installed_path, relative)?;
    }
    for relative in &plan.removed {
        remove_plugin_relative_file(&installed_path, relative)?;
    }
    copy_marketplace_shared_resource_assets(
        marketplace_path,
        &marketplace.name,
        &plugin_cache_root(),
    )?;

    let conflicts = plan.conflicts.clone();
    let kept_local = plan.kept_local.clone();
    let updated = plan.updated.clone();
    let added = plan.added.clone();
    let removed = plan.removed.clone();
    let status = if conflicts.is_empty() {
        let installed_at = install_state
            .as_ref()
            .map(|state| state.installed_at.clone());
        record_plugin_install_state(
            &installed_path,
            &plugin_id,
            manifest.version.clone(),
            installed_at,
        )?;
        if updated.is_empty() && added.is_empty() && removed.is_empty() {
            "upToDate"
        } else {
            "synced"
        }
    } else if updated.is_empty() && added.is_empty() && removed.is_empty() {
        "conflicts"
    } else {
        "partial"
    }
    .to_string();
    let message = if status == "upToDate" {
        "Plugin is already up to date.".to_string()
    } else if status == "synced" {
        format!(
            "Synced plugin: {} updated, {} added, {} removed.",
            updated.len(),
            added.len(),
            removed.len()
        )
    } else if status == "partial" {
        format!(
            "Partially synced plugin; {} conflict{} kept local.",
            conflicts.len(),
            if conflicts.len() == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "No files were changed because {} conflict{} need review.",
            conflicts.len(),
            if conflicts.len() == 1 { "" } else { "s" }
        )
    };

    Ok(PluginSyncResult {
        plugin_id: plugin_id.key(),
        status,
        installed_path: installed_path.to_string_lossy().into_owned(),
        updated,
        added,
        removed,
        kept_local,
        conflicts,
        message,
    })
}

pub fn uninstall_plugin(plugin_id: &str) -> Result<(), String> {
    let plugin_id = PluginId::parse(plugin_id)?;
    for target in typed_plugin_base_roots(&plugin_store_root(), &plugin_id) {
        remove_path_if_exists(&target)?;
    }
    remove_path_if_exists(&plugin_base_root_in_cache(&plugin_cache_root(), &plugin_id))?;
    let mut config = read_config();
    config.plugins.remove(&plugin_id.key());
    write_config(&config)
}
