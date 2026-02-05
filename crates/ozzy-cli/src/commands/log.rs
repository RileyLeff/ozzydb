use anyhow::Result;
use chrono::Local;
use ozzy_core::Project;

pub async fn show(num: usize) -> Result<()> {
    let project = Project::find_current()?;

    let commits = project.list_commits(num)?;

    if commits.is_empty() {
        println!("No commits yet.");
        println!();
        println!("Create your first commit with: ozzy commit -m \"Initial commit\"");
        return Ok(());
    }

    for commit in &commits {
        let local_time = commit.timestamp.with_timezone(&Local);
        let short_hash = &commit.hash[..12];

        println!("commit {} ({})", short_hash, local_time.format("%Y-%m-%d %H:%M"));
        println!("Author: {}", commit.author);
        println!();
        println!("    {}", commit.message);
        println!();

        // Show summary of changes
        let ds_count = commit.data_sources.len();
        let tf_count = commit.transforms.len();
        let ep_count = commit.endpoints.len();
        println!("    {} data source(s), {} transform(s), {} endpoint(s)", ds_count, tf_count, ep_count);
        println!();
    }

    if commits.len() == num {
        println!("(showing {} most recent commits)", num);
    }

    Ok(())
}
