use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

/// Parse key=value parameter
fn parse_param(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid param format: '{}' (expected key=value)", s))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

/// Parse input mapping: name:source or just source (defaults to "main")
fn parse_input_mapping(s: &str) -> Result<(String, String), String> {
    if let Some(pos) = s.find(':') {
        Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
    } else {
        Ok(("main".to_string(), s.to_string()))
    }
}

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

        /// Transform parameters (key=value, can be repeated)
        #[arg(short, long = "param", value_parser = parse_param)]
        params: Vec<(String, String)>,
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

    /// Authentication commands
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },

    /// Manage remote registries
    Remote {
        #[command(subcommand)]
        command: RemoteCommands,
    },

    /// Push project to remote registry
    Push {
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,

        /// Remote name (defaults to 'origin')
        #[arg(long, default_value = "origin")]
        remote: String,
    },

    /// Pull project from remote registry
    Pull {
        /// Remote name (defaults to 'origin')
        #[arg(long, default_value = "origin")]
        remote: String,

        /// Ref to pull (branch or tag, defaults to 'main')
        #[arg(long, default_value = "main")]
        r#ref: String,
    },

    /// Fetch and execute a remote endpoint
    Fetch {
        /// Remote endpoint (format: owner/project/endpoint[@ref])
        endpoint: String,

        /// Output file path (parquet)
        #[arg(short, long)]
        output: Option<String>,

        /// Transform parameters (key=value, can be repeated)
        #[arg(short, long = "param", value_parser = parse_param)]
        params: Vec<(String, String)>,

        /// Registry URL (defaults to configured origin)
        #[arg(long)]
        registry: Option<String>,
    },

    /// Manage tags
    Tag {
        #[command(subcommand)]
        command: TagCommands,
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

        /// Input data source(s) - format: [name:]source (can be repeated for multi-input)
        /// Examples: --input raw, --input main:raw --input meta:metadata
        #[arg(long, value_parser = parse_input_mapping)]
        input: Vec<(String, String)>,

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

    /// Clear all cached entries
    Clear,

    /// Push local cache entries to remote
    Push {
        /// Push all entries (default: only entries for current project)
        #[arg(long)]
        all: bool,

        /// Push specific hash only
        #[arg(long)]
        hash: Option<String>,

        /// Show what would be pushed without actually pushing
        #[arg(long)]
        dry_run: bool,
    },

    /// Pull cache entries from remote
    Pull {
        /// Pull all entries (default: only entries for current platform)
        #[arg(long)]
        all: bool,

        /// Pull specific hash only
        #[arg(long)]
        hash: Option<String>,

        /// Show what would be pulled without actually pulling
        #[arg(long)]
        dry_run: bool,
    },

    /// Sync local and remote cache
    Sync {
        /// Sync direction
        #[arg(long, default_value = "both")]
        direction: String,

        /// Show what would be synced without actually syncing
        #[arg(long)]
        dry_run: bool,
    },

    /// Show cache status (local and remote)
    Status,
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Login to registry via GitHub
    Login {
        /// Registry URL (defaults to https://registry.ozzydb.dev)
        #[arg(long)]
        registry: Option<String>,
    },

    /// Logout from registry
    Logout {
        /// Registry URL (defaults to https://registry.ozzydb.dev)
        #[arg(long)]
        registry: Option<String>,
    },

    /// Manage API tokens
    Token {
        #[command(subcommand)]
        command: TokenCommands,
    },
}

#[derive(Subcommand)]
enum TokenCommands {
    /// Create a new API token
    Create {
        /// Token name
        name: String,

        /// Scopes for the token (comma-separated)
        #[arg(long, default_value = "read,write")]
        scopes: String,

        /// Expiration in days
        #[arg(long)]
        expires: Option<u32>,

        /// Registry URL
        #[arg(long)]
        registry: Option<String>,
    },

    /// List API tokens
    Ls {
        /// Registry URL
        #[arg(long)]
        registry: Option<String>,
    },

    /// Revoke an API token
    Revoke {
        /// Token name
        name: String,

        /// Registry URL
        #[arg(long)]
        registry: Option<String>,
    },
}

#[derive(Subcommand)]
enum RemoteCommands {
    /// Add a remote registry
    Add {
        /// Remote name
        name: String,

        /// Registry URL
        url: String,
    },

    /// Remove a remote
    Rm {
        /// Remote name
        name: String,
    },

    /// List remotes
    Ls,
}

#[derive(Subcommand)]
enum TagCommands {
    /// Create a tag
    Create {
        /// Tag name
        name: String,

        /// Tag message
        #[arg(short, long)]
        message: Option<String>,
    },

    /// List tags
    Ls,

    /// Delete a tag
    Rm {
        /// Tag name
        name: String,
    },

    /// Show tag details
    Show {
        /// Tag name
        name: String,
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
        Commands::Run { endpoint, output, force, params } => {
            commands::run::execute(&endpoint, output.as_deref(), force, &params).await
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
            CacheCommands::Clear => {
                commands::cache::clear().await
            }
            CacheCommands::Push { all, hash, dry_run } => {
                commands::cache::push(all, hash.as_deref(), dry_run).await
            }
            CacheCommands::Pull { all, hash, dry_run } => {
                commands::cache::pull(all, hash.as_deref(), dry_run).await
            }
            CacheCommands::Sync { direction, dry_run } => {
                commands::cache::sync(&direction, dry_run).await
            }
            CacheCommands::Status => {
                commands::cache::status().await
            }
        },
        Commands::Auth { command } => match command {
            AuthCommands::Login { registry } => {
                commands::auth::login(registry.as_deref()).await
            }
            AuthCommands::Logout { registry } => {
                commands::auth::logout(registry.as_deref()).await
            }
            AuthCommands::Token { command } => match command {
                TokenCommands::Create { name, scopes, expires, registry } => {
                    let scope_list: Vec<String> = scopes.split(',').map(|s| s.trim().to_string()).collect();
                    commands::auth::token_create(&name, &scope_list, expires, registry.as_deref()).await
                }
                TokenCommands::Ls { registry } => {
                    commands::auth::token_list(registry.as_deref()).await
                }
                TokenCommands::Revoke { name, registry } => {
                    commands::auth::token_revoke(&name, registry.as_deref()).await
                }
            },
        },
        Commands::Remote { command } => match command {
            RemoteCommands::Add { name, url } => {
                commands::remote::add(&name, &url).await
            }
            RemoteCommands::Rm { name } => {
                commands::remote::remove(&name).await
            }
            RemoteCommands::Ls => {
                commands::remote::list().await
            }
        },
        Commands::Push { message, remote } => {
            commands::push::run(message.as_deref(), Some(&remote)).await
        }
        Commands::Pull { remote, r#ref } => {
            commands::pull::run(Some(&remote), Some(&r#ref)).await
        }
        Commands::Fetch { endpoint, output, params, registry: _ } => {
            // Registry is parsed from the endpoint reference itself
            commands::fetch::run(&endpoint, output.as_deref(), &params).await
        }
        Commands::Tag { command } => match command {
            TagCommands::Create { name, message } => {
                commands::tag::create(&name, message.as_deref()).await
            }
            TagCommands::Ls => {
                commands::tag::list().await
            }
            TagCommands::Rm { name } => {
                commands::tag::delete(&name).await
            }
            TagCommands::Show { name } => {
                commands::tag::show(&name).await
            }
        },
    }
}
