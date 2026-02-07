use anyhow::Result;
use ozzy_core::Project;
use ozzy_core::cache::{self, CacheBackend, LocalCache, TieredCache};

pub async fn list() -> Result<()> {
    let cache = LocalCache::open()?;
    let entries = cache.list()?;

    if entries.is_empty() {
        println!("Cache is empty.");
        return Ok(());
    }

    println!("Cached entries ({}):", entries.len());
    println!();

    for entry in &entries {
        let size = cache::format_size(entry.byte_size.unwrap_or(0));
        let rows = entry
            .row_count
            .map(|r| format!("{} rows", r))
            .unwrap_or_default();
        let accessed = entry.last_accessed.format("%Y-%m-%d %H:%M");

        println!("  {}...", &entry.materialized_hash[..16]);
        println!("    Platform: {}", entry.platform);
        println!("    Size: {} {}", size, rows);
        println!("    Accessed: {} ({} times)", accessed, entry.access_count);
        println!();
    }

    Ok(())
}

pub async fn size() -> Result<()> {
    let cache = LocalCache::open()?;

    let total_size = cache.total_size()?;
    let count = cache.count()?;

    println!("Cache statistics:");
    println!("  Location: {}", cache::default_cache_dir().display());
    println!("  Entries: {}", count);
    println!("  Total size: {}", cache::format_size(total_size));

    Ok(())
}

pub async fn clear() -> Result<()> {
    let cache = LocalCache::open()?;

    let count = cache.count()?;
    let size = cache.total_size()?;

    if count == 0 {
        println!("Cache is already empty.");
        return Ok(());
    }

    cache.clear()?;

    println!(
        "Cleared {} cache entries ({})",
        count,
        cache::format_size(size)
    );

    Ok(())
}

pub async fn push(all: bool, hash: Option<&str>, dry_run: bool) -> Result<()> {
    let project = Project::find_current()?;
    let tiered_config = project.config.cache.to_tiered_config();
    let tiered = TieredCache::new(&tiered_config).await?;

    if !tiered.has_remote() {
        println!("Remote cache not configured.");
        println!();
        println!("Configure remote cache in ozzy.toml:");
        println!();
        println!("  [cache.remote]");
        println!("  enabled = true");
        println!("  bucket = \"your-bucket-name\"");
        println!("  region = \"us-west-2\"");
        return Ok(());
    }

    if let Some(h) = hash {
        // Push specific hash
        if dry_run {
            if tiered.contains_local(h)? {
                println!("Would push: {}...", &h[..h.len().min(16)]);
            } else {
                println!("Hash not found in local cache: {}", h);
            }
        } else {
            if tiered.push(h).await? {
                println!("Pushed: {}...", &h[..h.len().min(16)]);
            } else {
                println!("Failed to push or not found: {}", h);
            }
        }
    } else {
        // Push all pending
        let pending = tiered.pending_push().await?;

        if pending.is_empty() {
            println!("All local entries are already in remote cache.");
            return Ok(());
        }

        if dry_run {
            println!("Would push {} entries:", pending.len());
            for entry in &pending {
                let size = cache::format_size(entry.byte_size.unwrap_or(0));
                println!("  {}... ({})", &entry.materialized_hash[..16], size);
            }
        } else {
            let stats = tiered.push_all().await?;
            println!("Push complete:");
            println!("  Pushed: {}", stats.pushed);
            println!("  Skipped (already remote): {}", stats.skipped);
            if stats.errors > 0 {
                println!("  Errors: {}", stats.errors);
            }
        }
    }

    let _ = all; // Reserved for future cross-project push

    Ok(())
}

pub async fn pull(all: bool, hash: Option<&str>, dry_run: bool) -> Result<()> {
    let project = Project::find_current()?;
    let tiered_config = project.config.cache.to_tiered_config();
    let tiered = TieredCache::new(&tiered_config).await?;

    if !tiered.has_remote() {
        println!("Remote cache not configured.");
        println!();
        println!("Configure remote cache in ozzy.toml:");
        println!();
        println!("  [cache.remote]");
        println!("  enabled = true");
        println!("  bucket = \"your-bucket-name\"");
        println!("  region = \"us-west-2\"");
        return Ok(());
    }

    if let Some(h) = hash {
        // Pull specific hash
        if dry_run {
            if tiered.contains_remote(h).await? {
                println!("Would pull: {}...", &h[..h.len().min(16)]);
            } else {
                println!("Hash not found in remote cache: {}", h);
            }
        } else {
            if tiered.pull(h).await? {
                println!("Pulled: {}...", &h[..h.len().min(16)]);
            } else {
                println!("Failed to pull or not found: {}", h);
            }
        }
    } else {
        // Pull all pending
        let pending = tiered.pending_pull().await?;

        if pending.is_empty() {
            println!("All remote entries are already in local cache.");
            return Ok(());
        }

        if dry_run {
            println!("Would pull {} entries:", pending.len());
            for entry in &pending {
                let size = cache::format_size(entry.byte_size.unwrap_or(0));
                println!("  {}... ({})", &entry.materialized_hash[..16], size);
            }
        } else {
            let stats = tiered.pull_all().await?;
            println!("Pull complete:");
            println!("  Pulled: {}", stats.pulled);
            println!("  Skipped (already local): {}", stats.skipped);
            if stats.errors > 0 {
                println!("  Errors: {}", stats.errors);
            }
        }
    }

    let _ = all; // Reserved for future cross-platform pull

    Ok(())
}

pub async fn sync(direction: &str, dry_run: bool) -> Result<()> {
    match direction {
        "push" => push(false, None, dry_run).await,
        "pull" => pull(false, None, dry_run).await,
        "both" => {
            println!("Pulling from remote...");
            pull(false, None, dry_run).await?;
            println!();
            println!("Pushing to remote...");
            push(false, None, dry_run).await
        }
        _ => {
            anyhow::bail!(
                "Invalid sync direction: '{}'. Use 'push', 'pull', or 'both'.",
                direction
            );
        }
    }
}

pub async fn status() -> Result<()> {
    let project = Project::find_current()?;
    let tiered_config = project.config.cache.to_tiered_config();
    let tiered = TieredCache::new(&tiered_config).await?;

    let cache_status = tiered.status().await?;

    println!("Cache Status");
    println!("============");
    println!();

    println!("Local cache:");
    println!("  Location: {}", cache::default_cache_dir().display());
    println!("  Entries: {}", cache_status.local_count);
    println!(
        "  Total size: {}",
        cache::format_size(cache_status.local_size)
    );
    println!();

    if cache_status.remote_configured {
        println!("Remote cache: configured");
        println!(
            "  Entries (current platform): {}",
            cache_status.remote_count
        );
        if !cache_status.remote_platforms.is_empty() {
            println!("  Platforms: {}", cache_status.remote_platforms.join(", "));
        }
        println!();

        let pending_push = tiered.pending_push().await?.len();
        let pending_pull = tiered.pending_pull().await?.len();

        println!("Sync status:");
        println!("  Synced: {}", cache_status.synced_count);
        println!("  Pending push: {}", pending_push);
        println!("  Pending pull: {}", pending_pull);
    } else {
        println!("Remote cache: not configured");
        println!();
        println!("To enable remote cache, add to ozzy.toml:");
        println!();
        println!("  [cache.remote]");
        println!("  enabled = true");
        println!("  bucket = \"your-bucket-name\"");
        println!("  region = \"us-west-2\"");
    }

    Ok(())
}
