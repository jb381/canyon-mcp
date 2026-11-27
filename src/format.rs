use crate::{
    model::{Availability, BikeDetail, BikeSummary, Locale, Money},
    parser::property_text,
};
use serde_json::Value;

const DASH: &str = "—";

fn optional(value: Option<&str>) -> String {
    value.unwrap_or(DASH).into()
}

pub fn format_money(money: &Money) -> String {
    let currency = money.currency.to_ascii_uppercase();
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return format!("{} {}", money.amount, money.currency);
    }
    let (prefix, digits) = match currency.as_str() {
        "EUR" => ("€", 2),
        "USD" => ("$", 2),
        "GBP" => ("£", 2),
        "CAD" => ("CA$", 2),
        "AUD" => ("A$", 2),
        "HKD" => ("HK$", 2),
        "NZD" => ("NZ$", 2),
        "TWD" => ("NT$", 2),
        "PHP" => ("₱", 2),
        "CNY" => ("CN¥", 2),
        "INR" => ("₹", 2),
        "MXN" => ("MX$", 2),
        "BRL" => ("R$", 2),
        "ILS" => ("₪", 2),
        "JPY" => ("¥", 0),
        "KRW" => ("₩", 0),
        "VND" => ("₫", 0),
        "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "KMF" | "PYG" | "RWF" | "UGX" | "VUV" | "XAF"
        | "XOF" | "XPF" => (currency.as_str(), 0),
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => (currency.as_str(), 3),
        _ => (currency.as_str(), 2),
    };
    let factor = 10_f64.powi(digits);
    let amount = (money.amount.abs() * factor).round() / factor;
    let amount = format!("{amount:.precision$}", precision = digits as usize);
    let (whole, fraction) = amount.split_once('.').unwrap_or((&amount, ""));
    let mut result = String::new();
    if money.amount.is_sign_negative() {
        result.push('-');
    }
    result.push_str(prefix);
    if prefix == currency {
        result.push('\u{a0}');
    }
    for (index, digit) in whole.chars().enumerate() {
        if index != 0 && (whole.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    if !fraction.is_empty() {
        result.push('.');
        result.push_str(fraction);
    }
    result
}

fn price(money: Option<&Money>) -> String {
    money.map_or_else(|| DASH.into(), format_money)
}

fn printable(value: Option<&Value>) -> String {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .map(property_text)
            .collect::<Vec<_>>()
            .join(" / "),
        Some(value) => property_text(value).into_owned(),
        None => DASH.into(),
    }
}

pub fn format_search_table(products: &[BikeSummary]) -> String {
    if products.is_empty() {
        return "No bikes found.".into();
    }
    render_table(
        &["ID", "Name", "Price", "Stock", "Category", "URL"],
        products
            .iter()
            .map(|product| {
                vec![
                    product.model_id.clone(),
                    product.name.clone(),
                    price(product.price.as_ref()),
                    product.availability.to_string(),
                    optional(product.category.as_deref()),
                    product.url.clone(),
                ]
            })
            .collect(),
    )
}

pub fn format_product(detail: &BikeDetail) -> String {
    let available = detail
        .variants
        .iter()
        .filter(|variant| variant.availability == Availability::InStock)
        .count();
    let mut lines = vec![
        format!("{} ({})", detail.name, detail.model_id),
        detail.description.clone().unwrap_or_default(),
        format!("URL: {}", detail.url),
        format!("Variants: {}; in stock: {available}", detail.variants.len()),
        format!("Weight: {}", printable(detail.attributes.get("Weight"))),
        format!("Material: {}", printable(detail.attributes.get("Material"))),
        format!(
            "Fetched: {}{}",
            detail.fetched_at,
            if detail.stale { " (stale cache)" } else { "" }
        ),
    ];
    lines.retain(|line| !line.is_empty());
    if !detail.variants.is_empty() {
        lines.push(String::new());
        lines.push(render_table(
            &["SKU", "Color", "Size", "Price", "Stock"],
            detail
                .variants
                .iter()
                .map(|variant| {
                    vec![
                        variant.sku.clone(),
                        optional(variant.color.as_deref()),
                        optional(variant.size.as_deref()),
                        price(variant.price.as_ref()),
                        variant.availability.to_string(),
                    ]
                })
                .collect(),
        ));
    }
    lines.join("\n")
}

pub fn format_comparison(details: &[BikeDetail]) -> String {
    // Retain all columns even when the model names match.
    let mut headers = vec!["Attribute"];
    headers.extend(details.iter().map(|detail| detail.name.as_str()));
    let mut price_row = vec!["Price".into()];
    price_row.extend(details.iter().map(|detail| {
        price(
            detail
                .variants
                .iter()
                .find_map(|variant| variant.price.as_ref()),
        )
    }));
    let mut rows = vec![price_row];
    for key in [
        "Weight",
        "Material",
        "World",
        "Style",
        "Year",
        "Rear Derailleur",
        "Wheel",
        "Tyre",
    ] {
        let mut row = vec![key.into()];
        row.extend(
            details
                .iter()
                .map(|detail| printable(detail.attributes.get(key))),
        );
        rows.push(row);
    }
    render_table(&headers, rows)
}

pub fn format_locales(locales: &[Locale]) -> String {
    locales
        .iter()
        .map(|entry| entry.locale.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn to_csv(products: &[BikeSummary]) -> String {
    let mut csv = String::from("modelId,sku,name,price,currency,availability,category,url\n");
    for product in products {
        let amount = product
            .price
            .as_ref()
            .map(|price| price.amount.to_string())
            .unwrap_or_default();
        let currency = product
            .price
            .as_ref()
            .map(|price| price.currency.as_str())
            .unwrap_or_default();
        let availability = product.availability.to_string();
        let fields = [
            &product.model_id,
            product.sku.as_deref().unwrap_or_default(),
            &product.name,
            &amount,
            currency,
            &availability,
            product.category.as_deref().unwrap_or_default(),
            &product.url,
        ];
        for (index, field) in fields.into_iter().enumerate() {
            if index > 0 {
                csv.push(',');
            }
            if field.contains([',', '"', '\n', '\r']) {
                csv.push('"');
                csv.push_str(&field.replace('"', "\"\""));
                csv.push('"');
            } else {
                csv.push_str(field);
            }
        }
        csv.push('\n');
    }
    csv
}

fn render_table(headers: &[&str], rows: Vec<Vec<String>>) -> String {
    let len = |value: &str| value.encode_utf16().count();
    let mut widths: Vec<_> = headers.iter().map(|value| len(value)).collect();
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(len(cell));
        }
    }
    let render = |row: &[&str]| {
        let mut line = String::new();
        for (index, (cell, width)) in row.iter().zip(&widths).enumerate() {
            if index != 0 {
                line.push_str("  ");
            }
            line.push_str(cell);
            line.extend(std::iter::repeat_n(' ', width - len(cell)));
        }
        line.trim_end().to_owned()
    };
    let mut lines = vec![
        render(headers),
        widths
            .iter()
            .map(|&width| "-".repeat(width))
            .collect::<Vec<_>>()
            .join("  "),
    ];
    lines.extend(
        rows.iter()
            .map(|row| render(&row.iter().map(String::as_str).collect::<Vec<_>>())),
    );
    lines.join("\n")
}
