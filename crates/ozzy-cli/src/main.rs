use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(name = "ozzy")]
#[command(author, version, about = "Data management platform for scientific computing")]
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

    /// Execute an endpoint locally (uses local working directory)
    Run {
        /// Endpoint name
        endpoint: String,

        /// Output file path
        #[arg(short, long)]
        output: Option<String>,

        /// Force re-execution (ignore cache)
        #[arg(long)]
        force: bool,

        /// Endpoint parameters (key=value, can be repeated)
        #[arg(short, long = "param")]
        params: Vec<String>,
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
    },

    /// Push current commit to registry
    Push,

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
    /// Upload a dataset
    Upload {
        /// File to upload
        file: String,

        /// Dataset name (defaults to filename stem)
        #[arg(long)]
        name: Option<String>,

        /// Description
        #[arg(long)]
        description: Option<String>,

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

        /// Scopes (comma-separated)
        #[arg(long, default_value = "read,write")]
        scopes: String,

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

    match cli.command {
        _ => {
            eprintln!("ozzy v2 - command not yet implemented");
            eprintln!("See planning/v2_architecture.md for the design.");
            std::process::exit(1);
        }
    }
}
