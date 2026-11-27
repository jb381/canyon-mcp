use crate::{
    CanyonError, Result,
    model::{Availability, BikeDetail, BikeSummary, BikeVariant, Locale, Money, Properties},
    timestamp,
};
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;
use std::{borrow::Cow, sync::LazyLock};
use url::Url;

macro_rules! selector {
    ($query:literal) => {{
        static SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse($query).unwrap());
        &SELECTOR
    }};
}

fn text(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn string(value: &Value) -> Option<&str> {
    text(value.as_str()?)
}

fn first(value: &Value) -> &Value {
    if value.is_array() { &value[0] } else { value }
}

fn image(value: &Value) -> Option<String> {
    string(first(value)).map(str::to_owned)
}

fn absolute_url(value: &str, base: &Url) -> Option<Url> {
    let mut url = base.join(value).ok()?;
    url.set_fragment(None);
    Some(url)
}

pub fn normalize_availability(value: &Value) -> Availability {
    let normalized: String = value
        .as_str()
        .unwrap_or_default()
        .bytes()
        .filter(u8::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase() as char)
        .collect();
    for (pattern, availability) in [
        ("instock", Availability::InStock),
        ("outofstock", Availability::OutOfStock),
        ("preorder", Availability::Preorder),
        ("backorder", Availability::Backorder),
        ("limited", Availability::Limited),
    ] {
        if normalized.contains(pattern) {
            return availability;
        }
    }
    Availability::Unknown
}

fn numeric(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|value| value.is_finite())
}

fn offer_price(offer: &Value) -> Option<Money> {
    let currency = string(&offer["priceCurrency"])?.to_owned();
    let raw = offer.get("price")?;
    let amount = match raw {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(*b),
        Value::String(s) if s.trim().is_empty() => 0.0,
        _ => numeric(raw)?,
    };
    Some(Money { amount, currency })
}

fn primitive(value: &Value) -> bool {
    value.is_string() || value.is_number() || value.is_boolean()
}

pub(crate) fn property_text(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(value) => Cow::Borrowed(value),
        Value::Number(number) if number.is_f64() => {
            Cow::Owned(number.as_f64().unwrap().to_string())
        }
        _ => Cow::Owned(value.to_string()),
    }
}

fn parse_properties(values: &Value) -> Properties {
    let mut properties = Properties::new();
    if let Some(values) = values.as_array() {
        for property in values {
            let Some(name) = string(&property["name"]) else {
                continue;
            };
            let raw = &property["value"];
            let parsed = match raw {
                Value::Array(values) => {
                    Value::Array(values.iter().filter(|v| primitive(v)).cloned().collect())
                }
                _ if primitive(raw) => match string(&property["unitText"]) {
                    Some(unit) => Value::String(format!("{} {unit}", property_text(raw))),
                    None => raw.clone(),
                },
                _ => continue,
            };
            use std::collections::btree_map::Entry;
            match properties.entry(name.into()) {
                Entry::Vacant(slot) => {
                    slot.insert(parsed);
                }
                Entry::Occupied(mut slot) => {
                    let previous = std::mem::take(slot.get_mut());
                    let mut values = match previous {
                        Value::Array(values) => values,
                        value => vec![value],
                    };
                    match parsed {
                        Value::Array(more) => values.extend(more),
                        value => values.push(value),
                    }
                    slot.insert(Value::Array(values));
                }
            }
        }
    }
    properties
}

fn find_typed<'a>(value: &'a Value, wanted: &str) -> Option<&'a Value> {
    match value {
        Value::Array(items) => items.iter().find_map(|item| find_typed(item, wanted)),
        Value::Object(fields) => {
            let types = &value["@type"];
            if types == wanted
                || types
                    .as_array()
                    .is_some_and(|types| types.iter().any(|ty| ty == wanted))
            {
                Some(value)
            } else {
                fields.values().find_map(|child| find_typed(child, wanted))
            }
        }
        _ => None,
    }
}

fn parse_variant(value: &Value, base: &Url) -> Option<BikeVariant> {
    let offer = first(&value["offers"]);
    Some(BikeVariant {
        sku: string(&value["sku"])?.into(),
        name: string(&value["name"])?.into(),
        url: absolute_url(string(&value["url"])?, base)?.into(),
        availability: normalize_availability(&offer["availability"]),
        attributes: parse_properties(&value["additionalProperty"]),
        color: string(&value["color"]).map(str::to_owned),
        size: string(&value["size"]).map(str::to_owned),
        image: image(&value["image"]),
        price: offer_price(offer),
    })
}

pub fn parse_product_page(
    html: &str,
    page_url: &Url,
    fetched_at: Option<&str>,
    stale: bool,
) -> Result<BikeDetail> {
    let document = Html::parse_document(html);
    let scripts: Vec<Value> = document
        .select(selector!(r#"script[type="application/ld+json"]"#))
        .filter_map(|script| serde_json::from_str(&script.text().collect::<String>()).ok())
        .collect();
    let group = scripts
        .iter()
        .find_map(|script| find_typed(script, "ProductGroup"));
    let product = group
        .or_else(|| {
            scripts
                .iter()
                .find_map(|script| find_typed(script, "Product"))
        })
        .ok_or_else(|| {
            CanyonError::new(
                "PARSE_ERROR",
                format!("No Product or ProductGroup JSON-LD found at {page_url}"),
            )
        })?;
    let missing = || {
        CanyonError::new(
            "PARSE_ERROR",
            format!("Product data at {page_url} is missing an ID or name"),
        )
    };
    let model_id = string(&product["productGroupID"])
        .or_else(|| string(&product["sku"]))
        .ok_or_else(missing)?;
    let name = string(&product["name"]).ok_or_else(missing)?;
    let canonical = string(&product["url"]).and_then(|value| absolute_url(value, page_url));
    let canonical = canonical.as_ref().unwrap_or(page_url);
    let mut variants: Vec<_> = product["hasVariant"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|variant| parse_variant(variant, canonical))
        .collect();
    if group.is_none()
        && variants.is_empty()
        && let Some(variant) = parse_variant(product, canonical)
    {
        variants.push(variant);
    }
    Ok(BikeDetail {
        model_id: model_id.into(),
        name: name.into(),
        url: canonical.as_str().into(),
        attributes: parse_properties(&product["additionalProperty"]),
        variants,
        fetched_at: fetched_at.map_or_else(timestamp, str::to_owned),
        stale,
        description: string(&product["description"]).map(str::to_owned),
        brand: string(&product["brand"]["name"]).map(str::to_owned),
        image: image(&product["image"]),
    })
}

fn tile_image(raw: &str) -> Option<String> {
    let data: Value = serde_json::from_str(raw).ok()?;
    let urls = &data[0]["urls"];
    ["xxl", "xl", "zoom"]
        .into_iter()
        .find_map(|size| string(&urls[size]).map(str::to_owned))
}

fn parse_localized_amount(raw: &str) -> Option<f64> {
    let mut value: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | ','))
        .collect();
    match (value.rfind('.'), value.rfind(',')) {
        (Some(dot), Some(comma)) if dot > comma => value.retain(|c| c != ','),
        (Some(_), Some(_)) => value = value.replace('.', "").replacen(',', ".", 1),
        (Some(index), None) | (None, Some(index)) => {
            let separator = value.as_bytes()[index] as char;
            if (1..=2).contains(&(value.len() - index - 1)) && value.find(separator) == Some(index)
            {
                value.replace_range(index..=index, ".");
            } else {
                value.retain(|c| c != separator);
            }
        }
        (None, None) => {}
    }
    value
        .parse::<f64>()
        .ok()
        .filter(|amount| amount.is_finite() && *amount > 0.0)
}

pub fn parse_search_page(
    html: &str,
    page_url: &Url,
    assume_in_stock: bool,
) -> (Vec<BikeSummary>, usize) {
    let document = Html::parse_document(html);
    let products: Vec<_> = document
        .select(selector!(r#"[data-product-tile-context="grid"][data-pid]"#))
        .filter_map(|tile| {
            let data: Value = serde_json::from_str(tile.attr("data-gtm-impression")?).ok()?;
            let item = &first(&data)["ecommerce"]["items"][0];
            let link = tile
                .select(selector!("a.js-productTileLink[href]"))
                .next()?;
            let href = text(link.attr("href")?)?;
            let name = string(&item["item_name"]).or_else(|| text(link.attr("title")?))?;
            let currency = string(&item["currency"]);
            let price = currency.and_then(|currency| {
                let amount = numeric(&item["gross_price"])
                    .filter(|value| *value > 0.0)
                    .or_else(|| {
                        let element = tile.select(selector!(".productTile__priceSale")).next()?;
                        parse_localized_amount(&element.text().collect::<String>())
                    })?;
                Some(Money {
                    amount,
                    currency: currency.into(),
                })
            });
            let image = tile
                .select(selector!(".js-colorSwatchSelected"))
                .next()
                .and_then(|swatch| tile_image(swatch.attr("data-tile-images")?))
                .or_else(|| {
                    tile_image(
                        tile.select(selector!("[data-tile-images]"))
                            .next()?
                            .attr("data-tile-images")?,
                    )
                });
            let highlights: String = tile
                .select(selector!(".productTileDefault__info--highlights"))
                .flat_map(|element| element.text())
                .collect();
            Some(BikeSummary {
                model_id: string(&item["item_id"])?.into(),
                name: name.into(),
                url: absolute_url(href, page_url)?.into(),
                availability: if assume_in_stock {
                    Availability::InStock
                } else {
                    normalize_availability(&item["item_availability"])
                },
                sku: tile.attr("data-pid").map(str::to_owned),
                category: string(&item["item_category"]).map(str::to_owned),
                subcategory: string(&item["item_category2"]).map(str::to_owned),
                color: string(&item["item_color"])
                    .filter(|_| item["item_color"] != "not defined")
                    .map(str::to_owned),
                highlight: text(&highlights).map(str::to_owned),
                image,
                price,
                outlet: (item["item_outlet_flag"] == "true").then_some(true),
            })
        })
        .collect();

    static COUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9.,\s]+").unwrap());
    static BODY_COUNT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b[0-9.,\s]+\s+(?:items|Artikel|articles)\b").unwrap());
    fn count(raw: &str, pattern: &Regex) -> Option<usize> {
        pattern
            .find(raw.trim())?
            .as_str()
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    }
    let total = document
        .select(selector!(".js-productCount"))
        .next()
        .and_then(|node| count(&node.text().collect::<String>(), &COUNT))
        .or_else(|| {
            let body = document.select(selector!("body")).next()?;
            count(&body.text().collect::<String>(), &BODY_COUNT)
        })
        .unwrap_or(products.len());
    (products, total)
}

#[derive(Debug, PartialEq, Eq)]
pub struct SearchFilterLink {
    pub name: String,
    pub canonical_value: String,
    pub actual_value: String,
}

pub fn parse_search_filter_links(html: &str, page_url: &Url) -> Vec<SearchFilterLink> {
    Html::parse_document(html)
        .select(selector!(
            "button.js-productFilterButton[data-url][data-gtm-click]"
        ))
        .filter_map(|button| {
            let event: Value = serde_json::from_str(button.attr("data-gtm-click")?).ok()?;
            let name = string(&event[0]["filter_name"])?;
            let canonical_value = string(&event[0]["filter_value"])?;
            let url = absolute_url(button.attr("data-url")?, page_url)?;
            let pairs: Vec<_> = url.query_pairs().collect();
            let get = |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, value)| value.as_ref())
            };
            let index = (1..=5).find(|index| get(&format!("prefn{index}")) == Some(name))?;
            let actual_value = get(&format!("prefv{index}")).filter(|value| !value.is_empty())?;
            Some(SearchFilterLink {
                name: name.into(),
                canonical_value: canonical_value.into(),
                actual_value: actual_value.into(),
            })
        })
        .collect()
}

pub fn parse_all_bikes_url(html: &str, page_url: &Url) -> Option<Url> {
    static LANDING_PATH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"/lp/[^/]+/?(?:\?|$)").unwrap());
    static BIKE_TEXT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)bike|fahrr|velo").unwrap());
    let document = Html::parse_document(html);
    document
        .select(selector!(r#"a[data-eventlabel="All Bikes"][href]"#))
        .next()
        .and_then(|link| absolute_url(text(link.attr("href")?)?, page_url))
        .or_else(|| {
            document.select(selector!("a[href]")).find_map(|link| {
                let href = link.attr("href")?;
                (LANDING_PATH.is_match(href)
                    && (BIKE_TEXT.is_match(href)
                        || BIKE_TEXT.is_match(&link.text().collect::<String>())))
                .then(|| absolute_url(href, page_url))
                .flatten()
            })
        })
}

fn sitemap_locs(xml: &str, parent: &str) -> Result<Vec<String>> {
    let document = roxmltree::Document::parse(xml).map_err(|error| {
        CanyonError::new(
            "PARSE_ERROR",
            format!("Invalid Canyon sitemap XML: {error}"),
        )
    })?;
    Ok(document
        .descendants()
        .filter(|node| {
            node.has_tag_name("loc") && node.parent().is_some_and(|node| node.has_tag_name(parent))
        })
        .filter_map(|node| text(node.text()?).map(str::to_owned))
        .collect())
}

pub fn parse_sitemap_index(xml: &str) -> Result<Vec<Locale>> {
    static SITEMAP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"sitemap-([a-z]{2})_([A-Z]{2})\.xml$").unwrap());
    Ok(sitemap_locs(xml, "sitemap")?
        .into_iter()
        .filter_map(|url| {
            let captures = SITEMAP.captures(&url)?;
            let locale = format!("{}-{}", &captures[1], captures[2].to_ascii_lowercase());
            Some(Locale { locale, url })
        })
        .collect())
}

pub fn parse_url_set(xml: &str) -> Result<Vec<String>> {
    sitemap_locs(xml, "url")
}
