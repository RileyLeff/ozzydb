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

    /// Manage first-class artifacts
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommands,
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

        /// Endpoint parameters as key=JSON_VALUE. Strings must be quoted JSON.
        #[arg(short, long = "param")]
        params: Vec<String>,

        /// Endpoint input bindings as input_name=artifact_uuid
        #[arg(long = "input")]
        inputs: Vec<String>,

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

#[derive(Subcommand)]
enum ArtifactCommands {
    /// Upload one or more blob artifacts
    Upload {
        /// Files to upload
        files: Vec<String>,

        /// Explicit content type override
        #[arg(long)]
        content_type: Option<String>,
    },

    /// List artifacts in the current project
    Ls,

    /// Show artifact details
    Show {
        /// Artifact UUID
        artifact_id: String,
    },

    /// Download a blob artifact
    Download {
        /// Artifact UUID
        artifact_id: String,

        /// Output file path (defaults to artifact-<uuid>.<ext>)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Create a bundle manifest artifact from named entries
    Bundle {
        /// Bundle entries as name=artifact_uuid
        #[arg(long = "entry")]
        entries: Vec<String>,
    },

    /// Create a collection manifest artifact from artifact UUIDs
    Collection {
        /// Collection item artifact UUIDs
        items: Vec<String>,
    },

    /// Declare or verify artifact conformance
    Conformance {
        /// Artifact UUID
        artifact_id: String,

        /// Version-pinned published type reference (for example std/Foo@2)
        #[arg(long = "type")]
        type_ref: String,

        /// Declare conformance without immediate verification
        #[arg(long, default_value_t = false)]
        no_verify: bool,
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

    /// Show endpoint details
    Show {
        /// Endpoint name
        name: String,

        /// Ref to inspect (defaults to current branch)
        #[arg(long, short)]
        r#ref: Option<String>,
    },

    /// Show endpoint DAG
    Dag {
        /// Endpoint name
        name: String,

        /// Output format (mermaid or json)
        #[arg(long, default_value = "mermaid", value_parser = ["mermaid", "json"])]
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
            inputs,
            timeout,
        } => {
            commands::fetch::run(&endpoint, output.as_deref(), &params, &inputs, timeout).await?;
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
        Commands::Push { r#ref, message } => {
            commands::push::run(r#ref.as_deref(), message.as_deref()).await?;
        }
        Commands::Artifact { command } => match command {
            ArtifactCommands::Upload {
                files,
                content_type,
            } => {
                commands::artifact::upload(&files, content_type.as_deref()).await?;
            }
            ArtifactCommands::Ls => {
                commands::artifact::ls().await?;
            }
            ArtifactCommands::Show { artifact_id } => {
                commands::artifact::show(&artifact_id).await?;
            }
            ArtifactCommands::Download {
                artifact_id,
                output,
            } => {
                commands::artifact::download(&artifact_id, output.as_deref()).await?;
            }
            ArtifactCommands::Bundle { entries } => {
                commands::artifact::bundle(&entries).await?;
            }
            ArtifactCommands::Collection { items } => {
                commands::artifact::collection(&items).await?;
            }
            ArtifactCommands::Conformance {
                artifact_id,
                type_ref,
                no_verify,
            } => {
                commands::artifact::conformance(&artifact_id, &type_ref, !no_verify).await?;
            }
        },
        Commands::Endpoint { command } => match command {
            EndpointCommands::Ls { r#ref } => {
                commands::endpoint::ls(r#ref.as_deref()).await?;
            }
            EndpointCommands::Show { name, r#ref } => {
                commands::endpoint::show(&name, r#ref.as_deref()).await?;
            }
            EndpointCommands::Dag {
                name,
                format,
                r#ref,
            } => {
                commands::endpoint::dag(&name, &format, r#ref.as_deref()).await?;
            }
        },
        Commands::Secret { command } => match command {
            SecretCommands::Set { name } => {
                commands::secret::set(&name).await?;
            }
            SecretCommands::Ls => {
                commands::secret::ls().await?;
            }
            SecretCommands::Rm { name } => {
                commands::secret::rm(&name).await?;
            }
        },
    }

    Ok(())
}
