use crate::{
    CanyonError, DEFAULT_LOCALE, Result,
    cache::CacheStats,
    http::{CanyonHttpClient, assert_allowed_canyon_url},
    model::{BikeDetail, BikeSummary, Locale, SearchOptions, SearchResult, Sort},
    normalize_locale, parser,
};
use regex::Regex;
use std::{sync::LazyLock, time::Duration};
use url::Url;

const ORIGIN: &str = "https://www.canyon.com";
const SEARCH_MAX_AGE: Duration = Duration::from_secs(30 * 60);
const PRODUCT_MAX_AGE: Duration = Duration::from_secs(10 * 60);
const DAILY_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub struct CanyonClient {
    http: CanyonHttpClient,
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn refinements(options: &SearchOptions) -> Result<Vec<(&'static str, String)>> {
    let mut filters = Vec::with_capacity(3);
    if options.in_stock {
        filters.push(("isInStock", "In-stock".into()));
    }
    if let Some(size) = trimmed(options.size.as_deref()) {
        filters.push(("pc_rahmengroesse", size.to_uppercase()));
    }
    if let Some(world) = trimmed(options.world.as_deref()) {
        let value = match world.to_ascii_lowercase().as_str() {
            "mountain" => "Mountain",
            "road" => "Road",
            "e-bike" | "ebike" => "E-Bike",
            "gravel" => "Gravel",
            "city/touring" | "city" => "City/Touring",
            "triathlon" => "Triathlon",
            _ => {
                return Err(CanyonError::new(
                    "INVALID_WORLD",
                    format!(
                        "Unknown world '{}'. Use mountain, road, e-bike, gravel, city/touring, or triathlon.",
                        options.world.as_deref().unwrap_or(world)
                    ),
                ));
            }
        };
        filters.push(("pc_welt", value.into()));
    }
    Ok(filters)
}

/// Match URLSearchParams.set, including its handling of duplicate parameters.
fn set_query(url: &mut Url, entries: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>) {
    let mut query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    for (key, value) in entries {
        let mut found = false;
        query.retain_mut(|(existing, current)| {
            if existing != key.as_ref() {
                return true;
            }
            if found {
                return false;
            }
            found = true;
            value.as_ref().clone_into(current);
            true
        });
        if !found {
            query.push((key.as_ref().into(), value.as_ref().into()));
        }
    }
    url.query_pairs_mut().clear().extend_pairs(query);
}

fn paging(url: &mut Url, sort: Sort) {
    set_query(url, [("srule", sort.rule()), ("start", "0"), ("sz", "200")]);
}

fn listing_url(locale: &str, query: Option<&str>) -> Url {
    let route = if query.is_some() {
        "shop"
    } else {
        "lp/bicycle"
    };
    let mut url = Url::parse(&format!("{ORIGIN}/{locale}/{route}/"))
        .expect("validated locale and fixed origin");
    if let Some(query) = query {
        set_query(&mut url, [("q", query), ("searchType", "bikes")]);
    }
    url
}

fn filter_pairs(name: &str, value: &str, index: usize) -> [(String, String); 2] {
    [
        (format!("prefn{index}"), name.into()),
        (format!("prefv{index}"), value.into()),
    ]
}

/// Build a listing URL using the canonical values for the requested filters.
/// Live search reads Canyon's published filter links for localized values.
pub fn search_url(options: &SearchOptions) -> Result<Url> {
    let locale = normalize_locale(options.locale.as_deref().unwrap_or(DEFAULT_LOCALE))?;
    let mut url = listing_url(&locale, trimmed(options.query.as_deref()));
    let filters = refinements(options)?;
    set_query(
        &mut url,
        filters
            .iter()
            .enumerate()
            .flat_map(|(index, (name, value))| filter_pairs(name, value, index + 1)),
    );
    paging(&mut url, options.sort);
    Ok(url)
}

fn max_age(refresh: bool, duration: Duration) -> Duration {
    if refresh { Duration::ZERO } else { duration }
}

fn is_bike(product: &BikeSummary) -> bool {
    let category = product
        .category
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    category.contains("bike")
        || (category == "outlet" && product.outlet == Some(true) && {
            let subcategory = product
                .subcategory
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            [
                "bike",
                "road",
                "gravel",
                "mountain",
                "e-bike",
                "city",
                "touring",
                "triathlon",
                "hybrid",
                "race",
                "endurance",
            ]
            .iter()
            .any(|term| subcategory.contains(term))
        })
}

impl CanyonClient {
    pub fn from_env() -> Result<Self> {
        Ok(Self::new(CanyonHttpClient::from_env()?))
    }

    pub fn new(http: CanyonHttpClient) -> Self {
        Self { http }
    }

    pub async fn search(&self, options: SearchOptions) -> Result<SearchResult> {
        let locale = normalize_locale(options.locale.as_deref().unwrap_or(DEFAULT_LOCALE))?;
        let filters = refinements(&options)?;
        let query = trimmed(options.query.as_deref());
        let mut stale = false;
        let mut url = if query.is_some() {
            listing_url(&locale, query)
        } else {
            let home = self
                .http
                .get(&format!("{ORIGIN}/{locale}/"), DAILY_MAX_AGE)
                .await?;
            stale = home.stale;
            parser::parse_all_bikes_url(&home.body, &home.url).ok_or_else(|| {
                CanyonError::new(
                    "CATALOG_NOT_FOUND",
                    format!("Could not find Canyon's all-bikes page for locale {locale}."),
                )
            })?
        };
        paging(&mut url, options.sort);
        let expiry = max_age(options.refresh, SEARCH_MAX_AGE);
        let base = self.http.get(url.as_str(), expiry).await?;
        stale |= base.stale;
        let response = if filters.is_empty() {
            base
        } else {
            let links = parser::parse_search_filter_links(&base.body, &base.url);
            let mut url = base.url.clone();
            let mut pairs = Vec::with_capacity(filters.len() * 2);
            for (index, (name, value)) in filters.iter().enumerate() {
                let Some(link) = links.iter().find(|link| {
                    link.name == *name
                        && link.canonical_value.to_lowercase() == value.to_lowercase()
                }) else {
                    return Ok(SearchResult {
                        products: Vec::new(),
                        total: 0,
                        locale,
                        url: base.url.into(),
                        fetched_at: base.fetched_at,
                        stale,
                    });
                };
                pairs.extend(filter_pairs(name, &link.actual_value, index + 1));
            }
            set_query(&mut url, pairs);
            self.http.get(url.as_str(), expiry).await?
        };
        let (mut products, mut total) =
            parser::parse_search_page(&response.body, &response.url, options.in_stock);
        if !options.include_gear {
            products.retain(is_bike);
            total = total.min(products.len());
        }
        products.truncate(options.limit.unwrap_or(25).clamp(1, 200));
        Ok(SearchResult {
            products,
            total,
            locale,
            url: response.url.into(),
            fetched_at: response.fetched_at,
            stale: stale || response.stale,
        })
    }

    pub async fn get_product(
        &self,
        identifier: &str,
        locale: &str,
        refresh: bool,
    ) -> Result<BikeDetail> {
        let locale = normalize_locale(locale)?;
        let looks_like_url = identifier
            .get(..7)
            .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
            || identifier
                .get(..8)
                .is_some_and(|s| s.eq_ignore_ascii_case("https://"));
        let url = if looks_like_url {
            assert_allowed_canyon_url(identifier)?.into()
        } else {
            let result = self
                .search(SearchOptions {
                    query: Some(identifier.into()),
                    locale: Some(locale),
                    limit: Some(200),
                    refresh,
                    ..Default::default()
                })
                .await?;
            let candidate = result
                .products
                .iter()
                .find(|product| {
                    product.model_id == identifier || product.sku.as_deref() == Some(identifier)
                })
                .or_else(|| (result.products.len() == 1).then(|| &result.products[0]));
            candidate.map(|product| product.url.clone()).ok_or_else(|| CanyonError::new(
                "PRODUCT_NOT_FOUND",
                format!("Could not resolve '{identifier}' to one Canyon product. Search by name first or pass a full product URL.")
            ))?
        };
        let page = self
            .http
            .get(&url, max_age(refresh, PRODUCT_MAX_AGE))
            .await?;
        parser::parse_product_page(&page.body, &page.url, Some(&page.fetched_at), page.stale)
    }

    pub async fn compare(
        &self,
        identifiers: &[String],
        locale: &str,
        refresh: bool,
    ) -> Result<Vec<BikeDetail>> {
        if !(2..=5).contains(&identifiers.len()) {
            return Err(CanyonError::new(
                "INVALID_COUNT",
                "Compare requires between 2 and 5 product identifiers.",
            ));
        }
        let mut details = Vec::with_capacity(identifiers.len());
        for identifier in identifiers {
            details.push(self.get_product(identifier, locale, refresh).await?);
        }
        Ok(details)
    }

    pub async fn list_locales(&self, refresh: bool) -> Result<Vec<Locale>> {
        let page = self
            .http
            .get(
                concat!("https://www.canyon.com", "/sitemap_index.xml"),
                max_age(refresh, DAILY_MAX_AGE),
            )
            .await?;
        parser::parse_sitemap_index(&page.body)
    }

    pub async fn list_product_urls(&self, locale: &str, refresh: bool) -> Result<Vec<String>> {
        static PRODUCT_PATH: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"/[0-9]+\.html(?:\?|$)").unwrap());
        let locale = normalize_locale(locale)?;
        let (language, country) = locale.split_at(2);
        let url = format!(
            "{ORIGIN}/sitemap-{language}_{}.xml",
            country[1..].to_ascii_uppercase()
        );
        let page = self.http.get(&url, max_age(refresh, DAILY_MAX_AGE)).await?;
        Ok(parser::parse_url_set(&page.body)?
            .into_iter()
            .filter(|url| PRODUCT_PATH.is_match(url))
            .collect())
    }

    pub fn cache_stats(&self) -> Result<CacheStats> {
        self.http.cache_stats()
    }
}
