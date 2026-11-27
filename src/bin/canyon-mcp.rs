use canyon_mcp::{
    CanyonError, DEFAULT_LOCALE, Result as CanyonResult,
    client::CanyonClient,
    format::{format_comparison, format_locales, format_product, format_search_table},
    model::{SearchOptions, Sort},
};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use std::{process::ExitCode, sync::LazyLock};

struct Server {
    client: CanyonClient,
}

fn default_locale() -> String {
    DEFAULT_LOCALE.into()
}

fn default_limit() -> usize {
    25
}

fn valid_locale<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    static LOCALE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^[A-Za-z]{2}[-_][A-Za-z]{2}$").unwrap());
    let value = String::deserialize(deserializer)?;
    if LOCALE.is_match(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "expected a language-country locale, for example en-de",
        ))
    }
}

fn present_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

fn valid_limit<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<usize, D::Error> {
    let value = f64::deserialize(deserializer)?;
    if (1.0..=200.0).contains(&value) && value.fract() == 0.0 {
        Ok(value as usize)
    } else {
        Err(serde::de::Error::custom(
            "limit must be a whole number between 1 and 200",
        ))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SearchInput {
    #[serde(default, deserialize_with = "present_string")]
    #[schemars(with = "String")]
    query: Option<String>,
    #[serde(default = "default_locale", deserialize_with = "valid_locale")]
    #[schemars(regex(pattern = r"^[A-Za-z]{2}[-_][A-Za-z]{2}$"))]
    locale: String,
    /// road, gravel, mountain, e-bike, city/touring, or triathlon
    #[serde(default, deserialize_with = "present_string")]
    #[schemars(with = "String")]
    world: Option<String>,
    #[serde(default, deserialize_with = "present_string")]
    #[schemars(with = "String")]
    size: Option<String>,
    #[serde(default)]
    in_stock: bool,
    #[serde(default)]
    sort: Sort,
    #[serde(default = "default_limit", deserialize_with = "valid_limit")]
    #[schemars(range(min = 1, max = 200))]
    limit: usize,
    #[serde(default)]
    refresh: bool,
    #[serde(default)]
    include_gear: bool,
}

fn valid_identifier<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    let identifier = String::deserialize(deserializer)?;
    if identifier.is_empty() {
        Err(serde::de::Error::custom("identifier must not be empty"))
    } else {
        Ok(identifier)
    }
}

fn valid_identifiers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error> {
    let identifiers = Vec::<String>::deserialize(deserializer)?;
    if !(2..=5).contains(&identifiers.len()) || identifiers.iter().any(String::is_empty) {
        Err(serde::de::Error::custom(
            "provide between 2 and 5 non-empty product identifiers",
        ))
    } else {
        Ok(identifiers)
    }
}

#[derive(Deserialize, JsonSchema)]
struct ProductInput {
    #[serde(deserialize_with = "valid_identifier")]
    #[schemars(length(min = 1))]
    identifier: String,
    #[serde(default = "default_locale", deserialize_with = "valid_locale")]
    #[schemars(regex(pattern = r"^[A-Za-z]{2}[-_][A-Za-z]{2}$"))]
    locale: String,
    #[serde(default)]
    refresh: bool,
}

#[derive(Deserialize, JsonSchema)]
struct CompareInput {
    #[serde(deserialize_with = "valid_identifiers")]
    #[schemars(length(min = 2, max = 5), inner(length(min = 1)))]
    identifiers: Vec<String>,
    #[serde(default = "default_locale", deserialize_with = "valid_locale")]
    #[schemars(regex(pattern = r"^[A-Za-z]{2}[-_][A-Za-z]{2}$"))]
    locale: String,
    #[serde(default)]
    refresh: bool,
}

#[derive(Deserialize, JsonSchema)]
struct LocalesInput {
    #[serde(default)]
    refresh: bool,
}

fn tool_result<T: Serialize>(
    result: CanyonResult<T>,
    render: impl FnOnce(&T) -> String,
) -> CallToolResult {
    let mut output = CallToolResult::default();
    let (text, data) = match result {
        Ok(data) => (render(&data), json!({ "data": data })),
        Err(CanyonError { code, message }) => {
            output.is_error = Some(true);
            (
                format!("{message} [{code}]"),
                json!({ "error": { "code": code, "message": message } }),
            )
        }
    };
    output.content = vec![ContentBlock::text(text)];
    output.structured_content = Some(data);
    output
}

#[tool_router]
impl Server {
    #[tool(
        description = "Search Canyon bicycle models with optional locale, world, size, stock, and sorting filters."
    )]
    async fn search_bikes(&self, Parameters(input): Parameters<SearchInput>) -> CallToolResult {
        let result = self
            .client
            .search(SearchOptions {
                query: input.query,
                locale: Some(input.locale),
                world: input.world,
                size: input.size,
                in_stock: input.in_stock,
                sort: input.sort,
                limit: Some(input.limit),
                refresh: input.refresh,
                include_gear: input.include_gear,
            })
            .await;
        tool_result(result, |result| {
            format!(
                "{}\n\nFetched: {}",
                format_search_table(&result.products),
                result.fetched_at
            )
        })
    }

    #[tool(
        description = "Get detailed variants, stock, geometry, and components by Canyon model ID, SKU, or product URL."
    )]
    async fn get_bike(&self, Parameters(input): Parameters<ProductInput>) -> CallToolResult {
        tool_result(
            self.client
                .get_product(&input.identifier, &input.locale, input.refresh)
                .await,
            format_product,
        )
    }

    #[tool(description = "Compare 2–5 Canyon models, including price and important components.")]
    async fn compare_bikes(&self, Parameters(input): Parameters<CompareInput>) -> CallToolResult {
        tool_result(
            self.client
                .compare(&input.identifiers, &input.locale, input.refresh)
                .await,
            |details| format_comparison(details),
        )
    }

    #[tool(description = "List language-country locales present in Canyon's public sitemap index.")]
    async fn list_locales(&self, Parameters(input): Parameters<LocalesInput>) -> CallToolResult {
        tool_result(self.client.list_locales(input.refresh).await, |locales| {
            format_locales(locales)
        })
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_tool_list_changed().build())
            .with_server_info(Implementation::new("canyon-catalog", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Read-only access to Canyon's public bicycle catalog. Search before fetching details when the model ID is unknown. Prices and availability are locale-specific and timestamped. Never claim cached availability is current when stale is true."
            )
    }
}

async fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let server = Server {
        client: CanyonClient::from_env()?,
    };
    server
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("canyon-mcp: {error}");
            ExitCode::FAILURE
        }
    }
}
