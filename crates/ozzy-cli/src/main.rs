use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(name = "ozzy")]
#[command(author, version, about = "Version control for data transformations", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new OzzyDB project
    Init {
        /// Project name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,

        /// Owner username or organization
        #[arg(long)]
        owner: Option<String>,
    },

    /// Manage raw data sources
    Data {
        #[command(subcommand)]
        command: DataCommands,
    },

    /// Manage transforms
    Transform {
        #[command(subcommand)]
        command: TransformCommands,
    },

    /// Manage endpoints (named pipelines)
    Endpoint {
        #[command(subcommand)]
        command: EndpointCommands,
    },

    /// Show the transform DAG
    Dag {
        /// Output format
        #[arg(long, default_value = "ascii")]
        format: String,

        /// Endpoint to visualize (defaults to all)
        endpoint: Option<String>,
    },

    /// Execute an endpoint locally
    Run {
        /// Endpoint name
        endpoint: String,

        /// Output file path (parquet)
        #[arg(short, long)]
        output: Option<String>,

        /// Force re-execution (ignore cache)
        #[arg(long)]
        force: bool,
    },

    /// Create a new commit
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Show commit history
    Log {
        /// Number of commits to show
        #[arg(short, long, default_value = "10")]
        num: usize,
    },

    /// Show project status
    Status,

    /// Manage local cache
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },
}

#[derive(Subcommand)]
enum DataCommands {
    /// Add a raw data source
    Add {
        /// Path to parquet file
        file: String,

        /// Name for the data source
        #[arg(long)]
        name: String,
    },

    /// List data sources
    Ls,

    /// Remove a data source
    Rm {
        /// Name of the data source
        name: String,
    },

    /// Show schema of a data source
    Schema {
        /// Name of the data source
        name: String,
    },
}

#[derive(Subcommand)]
enum TransformCommands {
    /// Add a transform
    Add {
        /// Path to transform file (e.g., transforms/qc.py:quality_control)
        file: String,

        /// Override the transform name (defaults to function name)
        #[arg(long)]
        name: Option<String>,
    },

    /// List transforms
    Ls,

    /// Remove a transform
    Rm {
        /// Name of the transform
        name: String,
    },

    /// Test a transform on sample data
    Test {
        /// Name of the transform
        name: String,

        /// Number of rows to sample
        #[arg(long, default_value = "1000")]
        sample: usize,
    },
}

#[derive(Subcommand)]
enum EndpointCommands {
    /// Create an endpoint
    Create {
        /// Endpoint name
        name: String,

        /// Input data source
        #[arg(long)]
        input: String,

        /// Transforms to apply (in order)
        #[arg(long, value_delimiter = ',')]
        transforms: Vec<String>,
    },

    /// List endpoints
    Ls,

    /// Remove an endpoint
    Rm {
        /// Endpoint name
        name: String,
    },

    /// Show endpoint details
    Show {
        /// Endpoint name
        name: String,
    },
}

#[derive(Subcommand)]
enum CacheCommands {
    /// List cached entries
    Ls,

    /// Show cache size
    Size,

    /// Clear cache
    Clear {
        /// Only clear entries for a specific project
        #[arg(long)]
        project: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name, owner } => {
            commands::init::run(name, owner).await
        }
        Commands::Data { command } => match command {
            DataCommands::Add { file, name } => {
                commands::data::add(&file, &name).await
            }
            DataCommands::Ls => {
                commands::data::list().await
            }
            DataCommands::Rm { name } => {
                commands::data::remove(&name).await
            }
            DataCommands::Schema { name } => {
                commands::data::schema(&name).await
            }
        },
        Commands::Transform { command } => match command {
            TransformCommands::Add { file, name } => {
                commands::transform::add(&file, name.as_deref()).await
            }
            TransformCommands::Ls => {
                commands::transform::list().await
            }
            TransformCommands::Rm { name } => {
                commands::transform::remove(&name).await
            }
            TransformCommands::Test { name, sample } => {
                commands::transform::test(&name, sample).await
            }
        },
        Commands::Endpoint { command } => match command {
            EndpointCommands::Create { name, input, transforms } => {
                commands::endpoint::create(&name, &input, &transforms).await
            }
            EndpointCommands::Ls => {
                commands::endpoint::list().await
            }
            EndpointCommands::Rm { name } => {
                commands::endpoint::remove(&name).await
            }
            EndpointCommands::Show { name } => {
                commands::endpoint::show(&name).await
            }
        },
        Commands::Dag { format, endpoint } => {
            commands::dag::show(&format, endpoint.as_deref()).await
        }
        Commands::Run { endpoint, output, force } => {
            commands::run::execute(&endpoint, output.as_deref(), force).await
        }
        Commands::Commit { message } => {
            commands::commit::create(message.as_deref()).await
        }
        Commands::Log { num } => {
            commands::log::show(num).await
        }
        Commands::Status => {
            commands::status::show().await
        }
        Commands::Cache { command } => match command {
            CacheCommands::Ls => {
                commands::cache::list().await
            }
            CacheCommands::Size => {
                commands::cache::size().await
            }
            CacheCommands::Clear { project } => {
                commands::cache::clear(project.as_deref()).await
            }
        },
    }
}
