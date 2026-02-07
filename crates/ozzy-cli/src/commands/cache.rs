use anyhow::Result;
use ozzy_core::cache::{self, LocalCache};

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
