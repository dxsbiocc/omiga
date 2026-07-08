use super::*;

#[derive(Debug, Clone)]
pub(super) struct RequestImageAttachment {
    media_type: String,
    data: String,
}

fn image_media_type_from_path(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

fn resolve_request_attachment_path(project_root: &Path, raw_path: &str) -> PathBuf {
    let trimmed = raw_path.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    }
}

pub(super) async fn load_request_image_attachments(
    project_root: &Path,
    paths: &[String],
) -> Vec<RequestImageAttachment> {
    const MAX_IMAGES: usize = 6;
    const MAX_BYTES: u64 = 20 * 1024 * 1024;
    let canonical_project = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let mut images = Vec::new();
    let mut seen = HashSet::new();

    for raw in paths {
        if images.len() >= MAX_IMAGES {
            break;
        }
        let trimmed = raw.trim();
        let raw_is_absolute = trimmed.starts_with("~/") || PathBuf::from(trimmed).is_absolute();
        let candidate = resolve_request_attachment_path(project_root, raw);
        let Some(media_type) = image_media_type_from_path(&candidate) else {
            continue;
        };
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if !raw_is_absolute && !canonical.starts_with(&canonical_project) {
            continue;
        }
        if !seen.insert(canonical.clone()) {
            continue;
        }
        let Ok(meta) = tokio::fs::metadata(&canonical).await else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_BYTES {
            continue;
        }
        let Ok(bytes) = tokio::fs::read(&canonical).await else {
            continue;
        };
        images.push(RequestImageAttachment {
            media_type: media_type.to_string(),
            data: BASE64.encode(bytes),
        });
    }

    images
}

pub(super) fn append_image_attachments_to_latest_user_message(
    messages: &mut [LlmMessage],
    attachments: &[RequestImageAttachment],
) {
    if attachments.is_empty() {
        return;
    }
    let Some(message) = messages
        .iter_mut()
        .rev()
        .find(|message| message.role == LlmRole::User)
    else {
        return;
    };
    for attachment in attachments {
        message.content.push(LlmContent::Image {
            source: crate::llm::ImageSource::Base64 {
                media_type: attachment.media_type.clone(),
                data: attachment.data.clone(),
            },
        });
    }
}
