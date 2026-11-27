use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    InStock,
    OutOfStock,
    Preorder,
    Backorder,
    Limited,
    Unknown,
}

impl fmt::Display for Availability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InStock => "in_stock",
            Self::OutOfStock => "out_of_stock",
            Self::Preorder => "preorder",
            Self::Backorder => "backorder",
            Self::Limited => "limited",
            Self::Unknown => "unknown",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Money {
    pub amount: f64,
    pub currency: String,
}

pub type Properties = BTreeMap<String, Value>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BikeSummary {
    pub model_id: String,
    pub name: String,
    pub url: String,
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcategory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Money>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outlet: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct BikeVariant {
    pub sku: String,
    pub name: String,
    pub url: String,
    pub availability: Availability,
    pub attributes: Properties,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Money>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BikeDetail {
    pub model_id: String,
    pub name: String,
    pub url: String,
    pub attributes: Properties,
    pub variants: Vec<BikeVariant>,
    pub fetched_at: String,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, ValueEnum, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Sort {
    #[default]
    Availability,
    PriceAsc,
    PriceDesc,
    Newest,
    Bestseller,
}

impl Sort {
    pub(crate) fn rule(self) -> &'static str {
        match self {
            Self::Availability => "sort_master_availability",
            Self::PriceAsc => "sort_price_ascending",
            Self::PriceDesc => "sort_price_descending",
            Self::Newest => "sort_last_added",
            Self::Bestseller => "bestsellers",
        }
    }
}

#[derive(Debug, Default)]
pub struct SearchOptions {
    pub query: Option<String>,
    pub locale: Option<String>,
    pub world: Option<String>,
    pub size: Option<String>,
    pub in_stock: bool,
    pub sort: Sort,
    pub limit: Option<usize>,
    pub refresh: bool,
    pub include_gear: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub products: Vec<BikeSummary>,
    pub total: usize,
    pub locale: String,
    pub url: String,
    pub fetched_at: String,
    pub stale: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Locale {
    pub locale: String,
    pub url: String,
}
