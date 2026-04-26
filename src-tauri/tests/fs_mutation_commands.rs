use omiga_lib::commands::fs::{
    create_directory_scoped, create_file_scoped, delete_fs_entry_scoped, list_directory_scoped,
    read_file_scoped, rename_fs_entry_scoped, write_file_scoped,
};
use tempfile::tempdir;

fn path_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn filetree_mutation_commands_create_rename_delete_entries() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let workspace_root = path_string(root);

    let folder = root.join("created");
    let created_folder = create_directory_scoped(path_string(&folder), workspace_root.clone())
        .await
        .expect("create directory");
    assert!(std::path::Path::new(&created_folder).is_dir());

    let file = folder.join("note.md");
    let created_file = create_file_scoped(path_string(&file), workspace_root.clone())
        .await
        .expect("create file");
    assert!(std::path::Path::new(&created_file).is_file());

    let listing = list_directory_scoped(
        path_string(&folder),
        None,
        None,
        Some(workspace_root.clone()),
    )
    .await
    .expect("list directory");
    assert_eq!(listing.total, 1);
    assert_eq!(listing.entries[0].name, "note.md");
    assert!(!listing.entries[0].is_directory);

    let renamed = folder.join("renamed.md");
    let renamed_file = rename_fs_entry_scoped(
        path_string(&file),
        path_string(&renamed),
        workspace_root.clone(),
    )
    .await
    .expect("rename file");
    assert!(std::path::Path::new(&renamed_file).is_file());
    assert!(!file.exists());
    assert!(renamed.exists());

    delete_fs_entry_scoped(path_string(&renamed), workspace_root.clone())
        .await
        .expect("delete file");
    assert!(!renamed.exists());

    delete_fs_entry_scoped(path_string(&folder), workspace_root)
        .await
        .expect("delete folder");
    assert!(!folder.exists());
}

#[tokio::test]
async fn filetree_mutation_commands_reject_conflicts_and_missing_paths() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let workspace_root = path_string(root);
    let a = root.join("a.txt");
    let b = root.join("b.txt");

    create_file_scoped(path_string(&a), workspace_root.clone())
        .await
        .expect("create a");
    create_file_scoped(path_string(&b), workspace_root.clone())
        .await
        .expect("create b");

    assert!(create_file_scoped(path_string(&a), workspace_root.clone())
        .await
        .is_err());
    assert!(
        rename_fs_entry_scoped(path_string(&a), path_string(&b), workspace_root.clone())
            .await
            .is_err()
    );
    assert!(
        delete_fs_entry_scoped(path_string(&root.join("missing.txt")), workspace_root)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn filetree_mutation_commands_reject_high_risk_paths() {
    let temp = tempdir().expect("tempdir");
    let workspace_root = path_string(temp.path());

    let relative = "relative-filetree-risk.txt";
    assert!(
        create_file_scoped(relative.to_string(), workspace_root.clone())
            .await
            .is_err()
    );
    assert!(!std::path::Path::new(relative).exists());

    assert!(delete_fs_entry_scoped(
        std::path::MAIN_SEPARATOR.to_string(),
        workspace_root.clone()
    )
    .await
    .is_err());

    if let Some(home) = dirs::home_dir() {
        assert!(
            delete_fs_entry_scoped(path_string(&home), workspace_root.clone())
                .await
                .is_err()
        );
    }

    let nested = temp.path().join("safe").join("nested.txt");
    create_file_scoped(path_string(&nested), workspace_root)
        .await
        .expect("nested create still allowed");
    assert!(nested.is_file());
}

#[tokio::test]
async fn filetree_mutation_commands_require_workspace_scope() {
    let workspace = tempdir().expect("workspace");
    let outside = tempdir().expect("outside");
    let workspace_root = path_string(workspace.path());

    let inside = workspace.path().join("inside.txt");
    create_file_scoped(path_string(&inside), workspace_root.clone())
        .await
        .expect("inside create allowed");
    assert!(inside.is_file());

    let outside_file = outside.path().join("outside.txt");
    assert!(
        create_file_scoped(path_string(&outside_file), workspace_root.clone())
            .await
            .is_err()
    );
    assert!(!outside_file.exists());

    let renamed_outside = outside.path().join("renamed.txt");
    assert!(rename_fs_entry_scoped(
        path_string(&inside),
        path_string(&renamed_outside),
        workspace_root.clone(),
    )
    .await
    .is_err());
    assert!(inside.exists());
    assert!(!renamed_outside.exists());

    assert!(
        delete_fs_entry_scoped(workspace_root.clone(), workspace_root.clone())
            .await
            .is_err()
    );

    let relative_root = "relative-workspace-root";
    let child = workspace.path().join("child.txt");
    assert!(
        create_file_scoped(path_string(&child), relative_root.to_string())
            .await
            .is_err()
    );
    assert!(!child.exists());
}

#[tokio::test]
async fn write_file_requires_workspace_scope() {
    let workspace = tempdir().expect("workspace");
    let outside = tempdir().expect("outside");
    let workspace_root = path_string(workspace.path());

    let inside = workspace.path().join("editable.txt");
    std::fs::write(&inside, "before").expect("seed inside");
    let written = write_file_scoped(
        path_string(&inside),
        "after".to_string(),
        None,
        workspace_root.clone(),
    )
    .await
    .expect("write inside");
    assert_eq!(written.bytes_written, "after".len());
    assert_eq!(
        std::fs::read_to_string(&inside).expect("read inside"),
        "after"
    );

    let outside_file = outside.path().join("outside.txt");
    std::fs::write(&outside_file, "outside-before").expect("seed outside");
    assert!(write_file_scoped(
        path_string(&outside_file),
        "outside-after".to_string(),
        None,
        workspace_root.clone(),
    )
    .await
    .is_err());
    assert_eq!(
        std::fs::read_to_string(&outside_file).expect("read outside"),
        "outside-before"
    );

    assert!(write_file_scoped(
        path_string(&inside),
        "blocked".to_string(),
        None,
        "relative-workspace-root".to_string(),
    )
    .await
    .is_err());
    assert_eq!(
        std::fs::read_to_string(&inside).expect("read inside"),
        "after"
    );
}

#[tokio::test]
async fn read_commands_require_workspace_scope() {
    let workspace = tempdir().expect("workspace");
    let outside = tempdir().expect("outside");
    let workspace_root = path_string(workspace.path());

    let inside = workspace.path().join("readable.txt");
    std::fs::write(&inside, "inside").expect("seed inside");
    let inside_read = read_file_scoped(
        path_string(&inside),
        None,
        None,
        Some(workspace_root.clone()),
    )
    .await
    .expect("read inside");
    assert_eq!(inside_read.content, "inside");

    let listing = list_directory_scoped(
        path_string(workspace.path()),
        None,
        None,
        Some(workspace_root.clone()),
    )
    .await
    .expect("list workspace");
    assert_eq!(listing.total, 1);
    assert_eq!(listing.entries[0].name, "readable.txt");

    let outside_file = outside.path().join("outside.txt");
    std::fs::write(&outside_file, "outside").expect("seed outside");
    assert!(read_file_scoped(
        path_string(&outside_file),
        None,
        None,
        Some(workspace_root.clone()),
    )
    .await
    .is_err());
    assert!(read_file_scoped(path_string(&inside), None, None, None)
        .await
        .is_err());
    assert!(list_directory_scoped(
        path_string(outside.path()),
        None,
        None,
        Some(workspace_root)
    )
    .await
    .is_err());
}
