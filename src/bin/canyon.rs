use canyon_mcp::{
    DEFAULT_LOCALE,
    client::CanyonClient,
    format::{format_comparison, format_locales, format_product, format_search_table, to_csv},
    model::{SearchOptions, Sort},
    timestamp,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

#[derive(Parser)]
#[command(
    name = "canyon",
    version,
    about = "Query Canyon's public bicycle catalog"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Search and filter bicycle models
    Search {
        #[command(flatten)]
        search: SearchArgs,
        #[arg(long)]
        json: bool,
    },
    /// Get a product by model ID, SKU, or full Canyon URL
    Product {
        identifier: String,
        #[command(flatten)]
        store: Store,
        #[arg(long)]
        json: bool,
    },
    /// Compare 2–5 products by model ID, SKU, or URL
    Compare {
        #[arg(required = true, num_args = 1..)]
        identifiers: Vec<String>,
        #[command(flatten)]
        store: Store,
        #[arg(long)]
        json: bool,
    },
    /// Export search results as JSON or CSV
    Export {
        #[command(flatten)]
        search: SearchArgs,
        #[arg(short, long, value_enum, default_value = "json")]
        format: ExportFormat,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// List locales published in Canyon's sitemap index
    Locales {
        #[arg(long)]
        refresh: bool,
        #[arg(long)]
        json: bool,
    },
    /// Poll a model for variant stock changes. Stop with Ctrl-C.
    Watch {
        identifier: String,
        #[arg(short, long, default_value = DEFAULT_LOCALE)]
        locale: String,
        #[arg(short, long)]
        size: Option<String>,
        /// Poll interval in seconds, with a minimum of 60
        #[arg(long, default_value = "300", value_parser = positive_integer)]
        interval: u64,
        #[arg(long)]
        once: bool,
    },
    /// Show HTTP cache statistics
    Cache,
}

#[derive(Args)]
struct Store {
    /// Canyon language-country locale
    #[arg(short, long, default_value = DEFAULT_LOCALE)]
    locale: String,
    /// Revalidate cached responses
    #[arg(long)]
    refresh: bool,
}

#[derive(Args)]
struct SearchArgs {
    query: Option<String>,
    #[command(flatten)]
    store: Store,
    /// road, gravel, mountain, e-bike, city/touring, or triathlon
    #[arg(short, long)]
    world: Option<String>,
    /// Frame size, for example M or 2XL
    #[arg(short, long)]
    size: Option<String>,
    /// Only models with a matching in-stock variant
    #[arg(long)]
    in_stock: bool,
    #[arg(long, value_enum, default_value = "availability")]
    sort: Sort,
    /// Maximum result count (1–200)
    #[arg(long, default_value = "25", value_parser = result_limit)]
    limit: usize,
    /// Include clothing, parts and accessories
    #[arg(long)]
    include_gear: bool,
}

impl From<SearchArgs> for SearchOptions {
    fn from(value: SearchArgs) -> Self {
        Self {
            query: value.query,
            locale: Some(value.store.locale),
            world: value.world,
            size: value.size,
            in_stock: value.in_stock,
            sort: value.sort,
            limit: Some(value.limit),
            refresh: value.store.refresh,
            include_gear: value.include_gear,
        }
    }
}

#[derive(Clone, ValueEnum)]
enum ExportFormat {
    Json,
    Csv,
}

fn positive_integer(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|number| {
            !value.starts_with('0')
                && value.bytes().all(|c| c.is_ascii_digit())
                && (1..=9_007_199_254_740_991).contains(number)
        })
        .ok_or_else(|| format!("Expected a positive whole number, received '{value}'."))
}

fn result_limit(value: &str) -> Result<usize, String> {
    let limit = positive_integer(value)?;
    if limit > 200 {
        return Err("The maximum result count is 200.".into());
    }
    Ok(limit as usize)
}

fn write_line(text: &str) -> io::Result<()> {
    writeln!(io::stdout().lock(), "{text}")
}

fn json(value: &impl Serialize) -> serde_json::Result<String> {
    serde_json::to_string_pretty(value)
}

async fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    let client = CanyonClient::from_env()?;
    match command {
        Command::Search {
            search,
            json: machine_readable,
        } => {
            let result = client.search(search.into()).await?;
            if machine_readable {
                write_line(&json(&result)?)?;
            } else {
                write_line(&format_search_table(&result.products))?;
                eprintln!(
                    "\nShowing {} of {}; fetched {}{}",
                    result.products.len(),
                    result.total,
                    result.fetched_at,
                    if result.stale { " (stale cache)" } else { "" }
                );
            }
        }
        Command::Product {
            identifier,
            store,
            json: machine_readable,
        } => {
            let detail = client
                .get_product(&identifier, &store.locale, store.refresh)
                .await?;
            let output = if machine_readable {
                json(&detail)?
            } else {
                format_product(&detail)
            };
            write_line(&output)?;
        }
        Command::Compare {
            identifiers,
            store,
            json: machine_readable,
        } => {
            let products = client
                .compare(&identifiers, &store.locale, store.refresh)
                .await?;
            let output = if machine_readable {
                json(&products)?
            } else {
                format_comparison(&products)
            };
            write_line(&output)?;
        }
        Command::Export {
            search,
            format,
            output,
        } => {
            let result = client.search(search.into()).await?;
            let body = match format {
                ExportFormat::Csv => to_csv(&result.products),
                ExportFormat::Json => json(&result)? + "\n",
            };
            if let Some(path) = output {
                std::fs::write(&path, body)?;
                eprintln!(
                    "Wrote {} products to {}",
                    result.products.len(),
                    path.display()
                );
            } else {
                io::stdout().lock().write_all(body.as_bytes())?;
            }
        }
        Command::Locales {
            refresh,
            json: machine_readable,
        } => {
            let locales = client.list_locales(refresh).await?;
            let output = if machine_readable {
                json(&locales)?
            } else {
                format_locales(&locales)
            };
            write_line(&output)?;
        }
        Command::Watch {
            mut identifier,
            locale,
            size,
            interval,
            once,
        } => {
            let size = size.map(|value| value.to_uppercase());
            let mut previous = None;
            loop {
                let mut product = client.get_product(&identifier, &locale, true).await?;
                identifier.clone_from(&product.url);
                product.variants.retain(|variant| {
                    size.as_ref().is_none_or(|size| {
                        variant
                            .size
                            .as_ref()
                            .is_some_and(|value| value.to_uppercase() == *size)
                    })
                });
                let snapshot: Vec<_> = product
                    .variants
                    .iter()
                    .map(|variant| {
                        (
                            variant.sku.clone(),
                            variant.color.clone(),
                            variant.size.clone(),
                            variant.availability,
                            variant.price.clone(),
                        )
                    })
                    .collect();
                if previous.as_ref() != Some(&snapshot) {
                    write_line(&format!("[{}] {}", timestamp(), product.name))?;
                    write_line(&format_product(&product))?;
                    previous = Some(snapshot);
                }
                if once {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(interval.max(60))).await;
            }
        }
        Command::Cache => write_line(&json(&client.cache_stats()?)?)?,
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run(Cli::parse().command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if let Some(error) = error.downcast_ref::<canyon_mcp::CanyonError>() {
                eprintln!("canyon: {} [{}]", error.message, error.code);
            } else if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
            {
                return ExitCode::SUCCESS;
            } else {
                eprintln!("canyon: {error}");
            }
            ExitCode::FAILURE
        }
    }
}
