use std::{fs, process::Command};

use anyhow::{anyhow, Result};
use er_engine::{paths::ErRoot, storage};

use super::*;

#[test]
fn advertises_browser_annotation_arguments_and_default_images() -> Result<()> {
    let tools = ErMcp::new().tool_router.list_all();
    let tool = tools
        .iter()
        .find(|tool| tool.name == "pr_browser_annotations")
        .ok_or_else(|| anyhow!("browser annotation tool not registered"))?;
    let props = tool
        .input_schema
        .get("properties")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow!("missing tool properties"))?;
    for field in [
        "ref",
        "pr_url",
        "repo",
        "project_id",
        "number",
        "bucket",
        "include_images",
    ] {
        assert!(props.contains_key(field), "missing {field}");
    }
    let args: PrBrowserAnnotationsArgs = serde_json::from_value(json!({}))?;
    assert!(args.include_images.unwrap_or(true));
    assert_eq!(feedback_bucket(args.bucket.as_deref())?, "pr");
    let args: PrBrowserAnnotationsArgs = serde_json::from_value(json!({"include_images":false}))?;
    assert!(!args.include_images.unwrap_or(true));
    Ok(())
}

#[tokio::test]
async fn browser_annotation_local_target_rejects_all_explicit_pr_fields() -> Result<()> {
    for target in [
        json!({"ref":"https://github.com/acme/widgets/pull/42"}),
        json!({"pr_url":"https://github.com/acme/widgets/pull/42"}),
        json!({"repo":"acme/widgets"}),
        json!({"project_id":"project"}),
        json!({"number":42}),
    ] {
        let mut args: PrBrowserAnnotationsArgs = serde_json::from_value(target)?;
        args.bucket = Some("local".into());
        let result = ErMcp::new().pr_browser_annotations(Parameters(args)).await;
        assert!(result.is_err());
    }
    Ok(())
}

// A child process isolates cwd and environment overrides from the parallel harness.
#[cfg(unix)]
#[test]
fn browser_annotation_pr_and_local_buckets_match_feedback_and_tab_storage() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    for args in [
        vec!["init", "-b", "feature/browser"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
        vec![
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(temp.path())
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let bin = temp.path().join("bin");
    fs::create_dir(&bin)?;
    let gh = bin.join("gh");
    fs::write(&gh, "#!/bin/sh\nprintf '%s\\n' '{\"number\":42,\"headRepository\":{\"owner\":{\"login\":\"acme\"},\"name\":\"widgets\"}}'\n")?;
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755))?;
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))?;
    for repo_local in ["0", "1"] {
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "server::browser_annotations_tests::browser_annotation_bucket_child",
                "--nocapture",
            ])
            .current_dir(temp.path())
            .env("ER_MCP_BROWSER_BUCKET_TEST", "1")
            .env("ER_STORAGE_ROOT", temp.path().join("managed"))
            .env("ER_REPO_LOCAL", repo_local)
            .env("PATH", &path)
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[tokio::test]
async fn browser_annotation_bucket_child() -> Result<()> {
    if std::env::var_os("ER_MCP_BROWSER_BUCKET_TEST").is_none() {
        return Ok(());
    }
    let target: PrRefFields = serde_json::from_value(json!({"repo":"acme/widgets", "number":42}))?;
    let pr = resolve_feedback_target(&target, "pr")?;
    let local = resolve_feedback_target(&PrRefFields::default(), "local")?;
    let cwd = std::env::current_dir()?;
    let root = cwd
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF8 fixture path"))?;
    // TabState::comments_dir delegates to ErRoot::er_dir. Its managed PR Diff
    // and local branch views use these roots. MCP PR resolution always uses
    // managed storage, including when the repo-local debug override is enabled.
    let pr_root = storage::resolve_managed_root_for_pr_bucket("acme-widgets", 42);
    let local_root = if storage::use_repo_local_storage() {
        ErRoot::RepoLocal(root.into())
    } else {
        storage::resolve_managed_root_for_view_bucket(
            &storage::slug_repo(root),
            "feature-browser",
            "branch",
        )
    };
    assert_eq!(pr.bucket_path, pr_root.er_dir());
    assert_eq!(local.bucket_path, local_root.er_dir());
    assert_ne!(local.bucket_path, pr.bucket_path);
    for (broken_bucket, working_bucket, working_args) in [
        (
            &pr.bucket_path,
            &local.bucket_path,
            json!({"bucket":"local"}),
        ),
        (
            &local.bucket_path,
            &pr.bucket_path,
            json!({"repo":"acme/widgets", "number":42}),
        ),
    ] {
        fs::create_dir_all(broken_bucket)?;
        fs::create_dir_all(working_bucket)?;
        fs::write(
            std::path::Path::new(broken_bucket).join("browser-annotations.json"),
            "broken",
        )?;
        fs::write(
            std::path::Path::new(working_bucket).join("browser-annotations.json"),
            r#"{"version":2,"next_number":1,"annotations":[]}"#,
        )?;
        let args = serde_json::from_value(working_args)?;
        let response = ErMcp::new()
            .pr_browser_annotations(Parameters(args))
            .await?;
        assert_eq!(response.is_error, Some(false));
        let broken_args = if broken_bucket == &pr.bucket_path {
            json!({"repo":"acme/widgets", "number":42})
        } else {
            json!({"bucket":"local"})
        };
        let args = serde_json::from_value(broken_args)?;
        let error = ErMcp::new()
            .pr_browser_annotations(Parameters(args))
            .await
            .err()
            .ok_or_else(|| anyhow!("broken metadata must fail"))?;
        assert!(error.message.contains("malformed"));
        assert!(!error.message.contains(root));
    }
    Ok(())
}
