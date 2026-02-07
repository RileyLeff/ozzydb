use anyhow::Result;
use ozzy_core::{Project, commit as commit_lib};
use std::fs;

pub async fn show() -> Result<()> {
    let project = Project::find_current()?;

    println!(
        "Project: {}/{}",
        project.config.project.owner, project.config.project.name
    );
    println!(
        "Branch: {}",
        project.config.refs.head.replace("refs/heads/", "")
    );

    // Show current HEAD
    if let Some(head) = project.head_commit()? {
        println!("HEAD: {}", &head[..12]);
    } else {
        println!("HEAD: (no commits)");
    }
    println!();

    // Check for changes
    let current_data = commit_lib::collect_data_sources(&project)?;
    let current_transforms = commit_lib::collect_transforms(&project)?;

    let last_commit = project.latest_commit()?;

    // Data sources
    let mut data_changes = Vec::new();
    for (name, ds) in &current_data {
        if let Some(ref commit) = last_commit {
            if let Some(old_ds) = commit.data_sources.get(name) {
                if ds.hash != old_ds.hash {
                    data_changes.push(format!("  modified: data/{}.parquet", name));
                }
            } else {
                data_changes.push(format!("  new:      data/{}.parquet", name));
            }
        } else {
            data_changes.push(format!("  new:      data/{}.parquet", name));
        }
    }

    if let Some(ref commit) = last_commit {
        for name in commit.data_sources.keys() {
            if !current_data.contains_key(name) {
                data_changes.push(format!("  deleted:  data/{}.parquet", name));
            }
        }
    }

    // Transforms
    let mut transform_changes = Vec::new();
    for (name, tf) in &current_transforms {
        if let Some(ref commit) = last_commit {
            if let Some(old_tf) = commit.transforms.get(name) {
                if tf.hash != old_tf.hash {
                    transform_changes.push(format!("  modified: {}", tf.source_path));
                }
            } else {
                transform_changes.push(format!("  new:      {}", tf.source_path));
            }
        } else {
            transform_changes.push(format!("  new:      {}", tf.source_path));
        }
    }

    if let Some(ref commit) = last_commit {
        for (name, tf) in &commit.transforms {
            if !current_transforms.contains_key(name) {
                transform_changes.push(format!("  deleted:  {}", tf.source_path));
            }
        }
    }

    // Staged endpoints
    let mut staged_endpoints = Vec::new();
    let staged_dir = project.ozzy_dir().join("staged_endpoints");
    if staged_dir.exists() {
        for entry in fs::read_dir(&staged_dir)? {
            let entry = entry?;
            let path = entry.path();
            match path.extension().and_then(|e| e.to_str()) {
                Some("json") => {
                    if let Some(stem) = path.file_stem() {
                        let name = stem.to_string_lossy().to_string();
                        staged_endpoints.push(format!("  new:      endpoint/{}", name));
                    }
                }
                Some("deleted") => {
                    if let Some(stem) = path.file_stem() {
                        let name = stem.to_string_lossy().to_string();
                        staged_endpoints.push(format!("  deleted:  endpoint/{}", name));
                    }
                }
                _ => {}
            }
        }
    }

    // Print status
    if data_changes.is_empty() && transform_changes.is_empty() && staged_endpoints.is_empty() {
        println!("Nothing to commit, working directory clean.");
    } else {
        if !data_changes.is_empty() {
            println!("Data sources:");
            for change in &data_changes {
                println!("{}", change);
            }
            println!();
        }

        if !transform_changes.is_empty() {
            println!("Transforms:");
            for change in &transform_changes {
                println!("{}", change);
            }
            println!();
        }

        if !staged_endpoints.is_empty() {
            println!("Staged endpoints:");
            for ep in &staged_endpoints {
                println!("{}", ep);
            }
            println!();
        }

        println!("Use 'ozzy commit -m \"message\"' to commit these changes.");
    }

    Ok(())
}
