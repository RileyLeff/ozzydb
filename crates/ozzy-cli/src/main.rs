use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(name = "ozzy")]
#[command(
    author,
    version,
    about = "Data management platform for scientific computing"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new OzzyDB project
    Init,

    /// Manage datasets
    Data {
        #[command(subcommand)]
        command: DataCommands,
    },

    /// Manage collections
    Collection {
        #[command(subcommand)]
        command: CollectionCommands,
    },

    /// Inspect endpoints
    Endpoint {
        #[command(subcommand)]
        command: EndpointCommands,
    },

    /// Fetch and execute a remote endpoint
    Fetch {
        /// Remote endpoint (owner/project/endpoint[@ref])
        endpoint: String,

        /// Output file path
        #[arg(short, long)]
        output: Option<String>,

        /// Endpoint parameters (key=value, can be repeated)
        #[arg(short, long = "param")]
        params: Vec<String>,

        /// Timeout in seconds for job completion (default: 600)
        #[arg(long, default_value = "600")]
        timeout: u64,
    },

    /// Push current commit to registry
    Push {
        /// Update this ref (defaults to current branch)
        #[arg(long, short)]
        r#ref: Option<String>,

        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Manage secrets
    Secret {
        #[command(subcommand)]
        command: SecretCommands,
    },

    /// Authentication commands
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },

    /// Manage local cache
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },

    /// Scaffold a new transform
    #[command(name = "transform")]
    Transform {
        #[command(subcommand)]
        command: TransformCommands,
    },
}

// --- Subcommand enums ---

#[derive(Subcommand)]
enum DataCommands {
    /// Upload one or more datasets
    Upload {
        /// Files to upload (supports globs)
        files: Vec<String>,

        /// Dataset name (only valid with a single file; defaults to filename stem)
        #[arg(long)]
        name: Option<String>,

        /// Description
        #[arg(long)]
        description: Option<String>,

        /// Tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,

        /// Metadata sidecar TOML file
        #[arg(long)]
        meta: Option<String>,

        /// Add to collection after upload
        #[arg(long)]
        collection: Option<String>,
    },

    /// List datasets
    Ls,

    /// Show dataset details
    Show {
        /// Dataset name
        name: String,
    },

    /// Update dataset metadata
    Describe {
        /// Dataset name
        name: String,

        /// Set description
        #[arg(long)]
        set_description: Option<String>,

        /// Show metadata history
        #[arg(long)]
        history: bool,
    },

    /// Yank a dataset (retract with reason)
    Yank {
        /// Dataset name
        name: String,

        /// Reason for yanking
        #[arg(long)]
        reason: String,
    },

    /// Download a dataset
    Download {
        /// Dataset name
        name: String,

        /// Output file path (defaults to original filename)
        #[arg(short, long)]
        output: Option<String>,
    },
}

#[derive(Subcommand)]
enum CollectionCommands {
    /// Create a collection
    Create {
        /// Collection name
        name: String,
    },

    /// Add members to a collection
    Add {
        /// Collection name
        name: String,

        /// Members to add (data:x, endpoint:y, collection:z)
        members: Vec<String>,
    },

    /// Remove members from a collection
    Rm {
        /// Collection name
        name: String,

        /// Members to remove
        members: Vec<String>,
    },

    /// List collections (or members of a collection)
    Ls {
        /// Collection name (omit to list all)
        name: Option<String>,
    },

    /// Show collection version history
    Log {
        /// Collection name
        name: String,
    },

    /// Show all leaf-level atoms in a collection
    Flatten {
        /// Collection name
        name: String,
    },
}

#[derive(Subcommand)]
enum EndpointCommands {
    /// List endpoints in the current project
    Ls {
        /// Ref to inspect (defaults to current branch)
        #[arg(long, short)]
        r#ref: Option<String>,
    },

    /// Show endpoint details (params, DAG, verification status)
    Show {
        /// Endpoint name
        name: String,

        /// Ref to inspect (defaults to current branch)
        #[arg(long, short)]
        r#ref: Option<String>,
    },

    /// Yank an endpoint version
    Yank {
        /// Endpoint name
        name: String,

        /// Ref or commit SHA of the version to yank
        #[arg(long, short)]
        r#ref: String,

        /// Reason for yanking
        #[arg(long)]
        reason: String,
    },

    /// Show endpoint DAG
    Dag {
        /// Endpoint name
        name: String,

        /// Output format
        #[arg(long, default_value = "ascii")]
        format: String,

        /// Ref to inspect (defaults to current branch)
        #[arg(long, short)]
        r#ref: Option<String>,
    },
}

#[derive(Subcommand)]
enum SecretCommands {
    /// Set a secret
    Set {
        /// Secret name (e.g., GEMINI_API_KEY)
        name: String,
    },

    /// List secrets (names only)
    Ls,

    /// Delete a secret
    Rm {
        /// Secret name
        name: String,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Login to registry via GitHub
    Login,

    /// Logout from registry
    Logout,

    /// Show authentication status
    Status,

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

        /// Token scope: "account" or "project:owner/slug"
        #[arg(long, default_value = "account")]
        scope: String,

        /// Expiration in days
        #[arg(long)]
        expires: Option<u32>,
    },

    /// List API tokens
    Ls,

    /// Revoke an API token
    Revoke {
        /// Token name
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
}

#[derive(Subcommand)]
enum TransformCommands {
    /// Scaffold a new transform file
    Scaffold {
        /// Transform name
        name: String,

        /// Language
        #[arg(long, default_value = "python")]
        lang: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    match cli.command {
        Commands::Init => {
            commands::init::run(&cwd)?;
        }
        Commands::Transform { command } => match command {
            TransformCommands::Scaffold { name, lang } => {
                commands::transform::scaffold(&cwd, &name, &lang)?;
            }
        },
        Commands::Auth { command } => match command {
            AuthCommands::Login => {
                commands::auth::login().await?;
            }
            AuthCommands::Logout => {
                commands::auth::logout()?;
            }
            AuthCommands::Status => {
                commands::auth::status().await?;
            }
            AuthCommands::Token { command } => match command {
                TokenCommands::Create {
                    name,
                    scope,
                    expires,
                } => {
                    commands::auth::token_create(&name, &scope, expires).await?;
                }
                TokenCommands::Ls => {
                    commands::auth::token_list().await?;
                }
                TokenCommands::Revoke { name } => {
                    commands::auth::token_revoke(&name).await?;
                }
            },
        },
        Commands::Fetch {
            endpoint,
            output,
            params,
            timeout,
        } => {
            commands::fetch::run(&endpoint, output.as_deref(), &params, timeout).await?;
        }
        Commands::Cache { command } => match command {
            CacheCommands::Ls => {
                commands::cache::ls()?;
            }
            CacheCommands::Size => {
                commands::cache::size()?;
            }
            CacheCommands::Clear => {
                commands::cache::clear()?;
            }
        },
        _ => {
            eprintln!("Command not yet implemented.");
            eprintln!("See planning/v3/architecture.md for the design.");
            std::process::exit(1);
        }
    }

    Ok(())
}
