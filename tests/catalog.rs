use canyon_mcp::{
    cache::{CacheEntry, HttpCache},
    client::{CanyonClient, search_url},
    http::{CanyonHttpClient, assert_allowed_canyon_url},
    model::{Availability, SearchOptions, Sort},
    parser::{
        parse_all_bikes_url, parse_product_page, parse_search_filter_links, parse_search_page,
        parse_sitemap_index, parse_url_set,
    },
    timestamp,
};
use serde_json::json;
use std::time::Duration;
use url::Url;

const SEARCH: &str = include_str!("fixtures/search.html");
const PRODUCT: &str = include_str!("fixtures/product.html");
const PRODUCT_URL: &str = "https://www.canyon.com/en-de/gravel-bikes/example/4513.html";
const LISTING_URL: &str = "https://www.canyon.com/en-de/lp/bicycle/";
const SITEMAP: &str = r#"<?xml version="1.0"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
<sitemap><loc>https://www.canyon.com/sitemap-en_DE.xml</loc></sitemap>
<sitemap><loc>https://www.canyon.com/sitemap-de_DE.xml</loc></sitemap>
<sitemap><loc>https://www.canyon.com/sitemap-other.xml</loc></sitemap>
</sitemapindex>"#;
const FILTERS: &str = r#"
<a data-eventlabel="All Bikes" href="/de-de/lp/fahrrad/">Alle Fahrräder</a>
<button class="js-productFilterButton" data-url="/de-de/lp/fahrrad/?prefn1=isInStock&amp;prefv1=Auf%20Lager" data-gtm-click='[{"event":"product_filter","filter_name":"isInStock","filter_value":"In-stock"}]'></button>
<button class="js-productFilterButton" data-url="/de-de/lp/fahrrad/?prefn1=pc_welt&amp;prefv1=Rennrad" data-gtm-click='[{"event":"product_filter","filter_name":"pc_welt","filter_value":"Road"}]'></button>
"#;

#[test]
fn parses_product_group_and_variants() {
    let fetched = "2026-09-22T00:00:00.000Z";
    let product = parse_product_page(
        PRODUCT,
        &Url::parse(PRODUCT_URL).unwrap(),
        Some(fetched),
        true,
    )
    .unwrap();
    assert_eq!(product.model_id, "4513");
    assert_eq!(product.attributes["Weight"], "8.39 kg");
    assert_eq!(product.variants.len(), 1);
    let variant = &product.variants[0];
    assert_eq!(variant.price.as_ref().unwrap().amount, 3999.0);
    assert_eq!(variant.price.as_ref().unwrap().currency, "EUR");
    assert_eq!(variant.availability, Availability::InStock);
    assert_eq!(variant.attributes["Stack"], "575 mm");
    assert_eq!(product.fetched_at, fetched);
    assert!(product.stale);
}

#[test]
fn handles_standalone_json_ld_and_merges_repeated_properties() {
    let html = r#"
<script type="application/ld+json">invalid</script>
<script type="application/ld+json">{"@graph":[{"@type":["Product"],"sku":"12","name":"Bike","url":"/model/12.html#top",
"offers":[{"price":"999.95","priceCurrency":"EUR","availability":"https://schema.org/BackOrder"}],
"additionalProperty":[{"name":"Drive","value":"1x"},{"name":"Drive","value":[12,true,{}]}]}]}</script>"#;
    let product = parse_product_page(html, &Url::parse(PRODUCT_URL).unwrap(), None, false).unwrap();
    assert_eq!(product.url, "https://www.canyon.com/model/12.html");
    assert_eq!(product.variants[0].availability, Availability::Backorder);
    assert_eq!(product.attributes["Drive"], json!(["1x", 12, true]));
    let error = parse_product_page("unrelated", &Url::parse(PRODUCT_URL).unwrap(), None, false)
        .unwrap_err();
    assert_eq!(error.code, "PARSE_ERROR");
}

#[test]
fn reads_tiles_and_falls_back_to_visible_localized_price() {
    let url = Url::parse(LISTING_URL).unwrap();
    let (products, total) = parse_search_page(SEARCH, &url, true);
    assert_eq!(total, 1);
    let product = &products[0];
    assert_eq!(product.model_id, "4513");
    assert_eq!(product.sku.as_deref(), Some("50000001"));
    assert_eq!(product.availability, Availability::InStock);
    assert_eq!(product.price.as_ref().unwrap().amount, 3999.0);
    assert_eq!(product.price.as_ref().unwrap().currency, "EUR");
    let (products, _) = parse_search_page(
        &SEARCH.replace("3.999 &euro;", "3.999,50 &euro;"),
        &url,
        false,
    );
    assert_eq!(products[0].price.as_ref().unwrap().amount, 3999.5);
}

#[test]
fn extracts_localized_filters_and_sitemap_urls() {
    let url = Url::parse("https://www.canyon.com/de-de/").unwrap();
    assert_eq!(
        parse_all_bikes_url(FILTERS, &url).unwrap().as_str(),
        "https://www.canyon.com/de-de/lp/fahrrad/"
    );
    assert_eq!(
        parse_all_bikes_url(r#"<a href="/de-de/lp/fahrrad/">Fahrräder</a>"#, &url)
            .unwrap()
            .as_str(),
        "https://www.canyon.com/de-de/lp/fahrrad/"
    );
    let filters = parse_search_filter_links(FILTERS, &url);
    assert_eq!(filters[0].name, "isInStock");
    assert_eq!(filters[0].canonical_value, "In-stock");
    assert_eq!(filters[0].actual_value, "Auf Lager");
    assert_eq!(filters[1].actual_value, "Rennrad");
    let locales = parse_sitemap_index(SITEMAP).unwrap();
    assert_eq!(
        locales
            .iter()
            .map(|l| l.locale.as_str())
            .collect::<Vec<_>>(),
        ["en-de", "de-de"]
    );
    assert_eq!(
        parse_url_set("<urlset><url><loc>https://example.org/a?a=1&amp;b=2</loc></url></urlset>")
            .unwrap(),
        ["https://example.org/a?a=1&b=2"]
    );
    assert!(parse_sitemap_index("<div>").is_err());
}

#[test]
fn builds_search_routes_and_guards_allowed_urls() {
    let options = SearchOptions {
        query: Some(" grail ".into()),
        locale: Some(" EN_de ".into()),
        in_stock: true,
        size: Some("m".into()),
        world: Some("gravel".into()),
        sort: Sort::PriceAsc,
        ..Default::default()
    };
    let url = search_url(&options).unwrap();
    assert_eq!(
        url.as_str(),
        "https://www.canyon.com/en-de/shop/?q=grail&searchType=bikes&prefn1=isInStock&prefv1=In-stock&prefn2=pc_rahmengroesse&prefv2=M&prefn3=pc_welt&prefv3=Gravel&srule=sort_price_ascending&start=0&sz=200"
    );
    assert_eq!(
        search_url(&SearchOptions::default()).unwrap().path(),
        "/en-de/lp/bicycle/"
    );
    for url in [
        "https://example.org/",
        "http://www.canyon.com/",
        "https://www.canyon.com:8080/",
        "https://name@www.canyon.com/",
    ] {
        assert_eq!(
            assert_allowed_canyon_url(url).unwrap_err().code,
            "URL_NOT_ALLOWED"
        );
    }
    assert_eq!(
        assert_allowed_canyon_url("https://www.canyon.com/on/demandware.store/private")
            .unwrap_err()
            .code,
        "ROBOTS_DISALLOWED"
    );
    assert!(
        assert_allowed_canyon_url("https://www.canyon.com/en-de/#top")
            .unwrap()
            .fragment()
            .is_none()
    );
}

fn insert(cache: &HttpCache, url: &str, body: &str) {
    cache
        .put(
            url,
            &CacheEntry {
                body: body.into(),
                etag: None,
                last_modified: None,
                fetched_at: timestamp(),
            },
        )
        .unwrap();
}

#[tokio::test]
async fn catalog_client_reads_legacy_cache_through_its_normal_entry_points() {
    let cache = HttpCache::open(":memory:").unwrap();
    insert(&cache, "https://www.canyon.com/de-de/", FILTERS);
    let base =
        "https://www.canyon.com/de-de/lp/fahrrad/?srule=sort_master_availability&start=0&sz=200";
    insert(&cache, base, FILTERS);
    let filtered =
        format!("{base}&prefn1=isInStock&prefv1=Auf+Lager&prefn2=pc_welt&prefv2=Rennrad");
    let with_gear = SEARCH.replace(
        "</body>",
        &format!("{}{}", SEARCH.replace("Gravel Bikes", "Gear"), "</body>"),
    );
    insert(&cache, &filtered, &with_gear);
    insert(&cache, PRODUCT_URL, PRODUCT);
    for query in ["4513", "50000001", "Grail"] {
        insert(
            &cache,
            &format!(
                "https://www.canyon.com/en-de/shop/?q={query}&searchType=bikes&srule=sort_master_availability&start=0&sz=200"
            ),
            SEARCH,
        );
    }
    insert(&cache, "https://www.canyon.com/sitemap_index.xml", SITEMAP);
    insert(
        &cache,
        "https://www.canyon.com/sitemap-en_DE.xml",
        &format!(
            "<urlset><url><loc>{PRODUCT_URL}</loc></url><url><loc>{LISTING_URL}</loc></url></urlset>"
        ),
    );
    let client = CanyonClient::new(
        CanyonHttpClient::new(cache, Duration::ZERO, Duration::from_millis(50), "test").unwrap(),
    );
    let result = client
        .search(SearchOptions {
            locale: Some("de-de".into()),
            world: Some("road".into()),
            in_stock: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(result.url, filtered);
    assert_eq!(result.products.len(), 1);
    assert_eq!(result.products[0].availability, Availability::InStock);
    let result = client
        .search(SearchOptions {
            locale: Some("de-de".into()),
            world: Some("road".into()),
            in_stock: true,
            include_gear: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(result.products.len(), 2);
    let missing = client
        .search(SearchOptions {
            locale: Some("de-de".into()),
            size: Some("7XL".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(missing.products.is_empty());
    assert_eq!(missing.url, base);

    for identifier in [PRODUCT_URL, "4513", "50000001", "Grail"] {
        assert_eq!(
            client
                .get_product(identifier, "en-de", false)
                .await
                .unwrap()
                .model_id,
            "4513"
        );
    }
    assert_eq!(
        client
            .compare(&["4513".into(), "50000001".into()], "en-de", false)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        client
            .compare(&["4513".into()], "en-de", false)
            .await
            .unwrap_err()
            .code,
        "INVALID_COUNT"
    );
    assert_eq!(client.list_locales(false).await.unwrap().len(), 2);
    assert_eq!(
        client.list_product_urls("en-de", false).await.unwrap(),
        [PRODUCT_URL]
    );
}
