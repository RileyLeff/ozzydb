use anyhow::Result;
use ozzy_core::Project;
use std::env;

pub async fn run(name: Option<String>, owner: Option<String>) -> Result<()> {
    let current_dir = env::current_dir()?;

    // Default name to directory name
    let name = name.unwrap_or_else(|| {
        current_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string())
    });

    // Default owner to current user
    let owner = owner.unwrap_or_else(|| {
        env::var("USER")
            .or_else(|_| env::var("USERNAME"))
            .unwrap_or_else(|_| "user".to_string())
    });

    match Project::init(&current_dir, &name, &owner) {
        Ok(_) => {
            println!("Initialized OzzyDB project: {}/{}", owner, name);
            println!();
            println!("Created:");
            println!("  ozzy.toml     - Project configuration");
            println!("  .ozzy/        - Internal data (commits, refs, objects)");
            println!("  data/         - Place raw data files here");
            println!("  transforms/   - Place transform scripts here");
            println!();
            println!("Next steps:");
            println!("  1. Add raw data:      ozzy data add mydata.parquet --name raw");
            println!("  2. Add a transform:   ozzy transform add transforms/qc.py");
            println!("  3. Create an endpoint: ozzy endpoint create corrected --input raw --transforms qc");
            println!("  4. Run the pipeline:  ozzy run corrected");
            Ok(())
        }
        Err(ozzy_core::Error::ProjectAlreadyExists) => {
            println!("Project already initialized in this directory.");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
