use canyon_mcp::timestamp;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};
use tempfile::TempDir;

const CLI: &str = env!("CARGO_BIN_EXE_canyon");
const MCP: &str = env!("CARGO_BIN_EXE_canyon-mcp");
const PRODUCT_URL: &str = "https://www.canyon.com/en-de/gravel-bikes/example/4513.html";
const SEARCH_URL: &str = "https://www.canyon.com/en-de/shop/?q=grail&searchType=bikes&srule=sort_master_availability&start=0&sz=200";

struct Fixture {
    dir: TempDir,
    unavailable_proxy: String,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let database = Connection::open(dir.path().join("cache.sqlite")).unwrap();
        database.execute_batch(
            "PRAGMA journal_mode = WAL;
            CREATE TABLE http_cache (
                url TEXT PRIMARY KEY, body TEXT NOT NULL, etag TEXT, last_modified TEXT, fetched_at TEXT NOT NULL
            );",
        ).unwrap();
        for (url, body) in [
            (PRODUCT_URL, include_str!("fixtures/product.html")),
            (SEARCH_URL, include_str!("fixtures/search.html")),
            (
                "https://www.canyon.com/sitemap_index.xml",
                "<sitemapindex><sitemap><loc>https://www.canyon.com/sitemap-en_DE.xml</loc></sitemap></sitemapindex>",
            ),
        ] {
            database
                .execute(
                    "INSERT INTO http_cache VALUES (?1, ?2, NULL, NULL, ?3)",
                    params![url, body, timestamp()],
                )
                .unwrap();
        }
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let unavailable_proxy = format!("http://{}", socket.local_addr().unwrap());
        Self {
            dir,
            unavailable_proxy,
        }
    }

    fn command(&self, path: &str) -> Command {
        let mut command = Command::new(path);
        command
            .env("CANYON_CACHE_PATH", self.dir.path().join("cache.sqlite"))
            .env("CANYON_MIN_INTERVAL_MS", "0")
            .env("HTTPS_PROXY", &self.unavailable_proxy)
            .env("https_proxy", &self.unavailable_proxy)
            .env_remove("NO_PROXY")
            .env_remove("no_proxy");
        command
    }

    fn cli(&self, args: &[&str]) -> String {
        let output = self.command(CLI).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn cli_json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.cli(args)).unwrap()
    }
}

#[test]
fn cli_validates_numeric_options_before_opening_the_cache() {
    let fixture = Fixture::new();
    let missing_cache = fixture.dir.path().join("failure.sqlite");
    for args in [
        &["search", "grail", "--limit", "nope"][..],
        &["search", "grail", "--limit", "201"][..],
        &["watch", "4513", "--interval", "0"][..],
    ] {
        let output = fixture
            .command(CLI)
            .args(args)
            .env("CANYON_CACHE_PATH", &missing_cache)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
        assert!(!missing_cache.exists());
    }
}

#[test]
fn cli_searches_compares_exports_and_inspects_the_legacy_cache() {
    let fixture = Fixture::new();
    let result = fixture.cli_json(&["search", "grail", "--json"]);
    assert_eq!(result["products"][0]["modelId"], "4513");
    assert_eq!(
        result["products"][0]["price"]["amount"].as_f64(),
        Some(3999.0)
    );
    assert_eq!(result["stale"], false);
    assert!(fixture.cli(&["search", "grail"]).contains("€3,999.00"));
    assert_eq!(
        fixture.cli_json(&["product", PRODUCT_URL, "--json"])["variants"][0]["size"],
        "M"
    );
    assert_eq!(
        fixture
            .cli_json(&["compare", PRODUCT_URL, PRODUCT_URL, "--json"])
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(fixture.cli(&["locales"]), "en-de\n");
    assert_eq!(fixture.cli_json(&["cache"])["entries"], 3);

    let csv = fixture.cli(&["export", "grail", "--format", "csv"]);
    assert!(csv.contains("4513,50000001,Grail Test,3999,EUR,unknown,Gravel Bikes,"));
    let path = fixture.dir.path().join("shortlist.json");
    assert!(
        fixture
            .cli(&["export", "grail", "--output", path.to_str().unwrap()])
            .is_empty()
    );
    let exported: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(exported, result);
}

#[test]
fn watch_once_filters_size_and_labels_fallback_as_stale() {
    let fixture = Fixture::new();
    let output = fixture.cli(&["watch", PRODUCT_URL, "--once", "--size", "2XL"]);
    assert!(output.starts_with("["));
    assert!(output.contains("Variants: 0; in stock: 0"));
    assert!(output.contains("(stale cache)"));
}

struct StdioSession {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
}

impl StdioSession {
    fn send(&mut self, message: Value) {
        writeln!(self.input, "{message}").unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0", "id":self.id, "method":method,"params":params}));
        let mut line = String::new();
        assert_ne!(self.output.read_line(&mut line).unwrap(), 0);
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], self.id);
        response
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
    }
}

impl Drop for StdioSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn stdio_preserves_tools_and_structured_results_for_existing_protocol_versions() {
    let fixture = Fixture::new();
    for version in ["2025-03-26", "2025-06-18", "2025-11-25"] {
        let mut child = fixture
            .command(MCP)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut session = StdioSession {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            id: 0,
        };
        let init = session.request("initialize", json!({
            "protocolVersion":version, "clientInfo":{"name":"test","version":"1"}, "capabilities":{}
        }));
        assert_eq!(init["result"]["protocolVersion"], version);
        assert_eq!(init["result"]["serverInfo"]["name"], "canyon-catalog");
        session.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        let list = session.request("tools/list", json!({}));
        let tools = list["result"]["tools"].as_array().unwrap();
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            names,
            ["compare_bikes", "get_bike", "list_locales", "search_bikes"].into()
        );
        let search = tools
            .iter()
            .find(|tool| tool["name"] == "search_bikes")
            .unwrap();
        let fields = &search["inputSchema"]["properties"];
        assert_eq!(fields["limit"]["default"], 25);
        assert_eq!(fields["limit"]["maximum"], 200);
        assert_eq!(fields["sort"]["default"], "availability");
        assert_eq!(fields["locale"]["default"], "en-de");

        let reply = session.call(
            "search_bikes",
            json!({"query":"grail","locale":"EN_DE","limit":3.0}),
        );
        assert_eq!(
            reply["result"]["structuredContent"]["data"]["products"][0]["modelId"], "4513",
            "{reply}"
        );
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Fetched:")
        );
        let reply = session.call("get_bike", json!({"identifier":PRODUCT_URL}));
        assert_eq!(
            reply["result"]["structuredContent"]["data"]["modelId"],
            "4513"
        );
        let reply = session.call(
            "compare_bikes",
            json!({"identifiers":[PRODUCT_URL,PRODUCT_URL]}),
        );
        assert_eq!(
            reply["result"]["structuredContent"]["data"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let reply = session.call("list_locales", json!({}));
        assert_eq!(
            reply["result"]["structuredContent"]["data"][0]["locale"],
            "en-de"
        );
        let reply = session.call("get_bike", json!({"identifier":"https://example.org/"}));
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(
            reply["result"]["structuredContent"]["error"]["code"],
            "URL_NOT_ALLOWED"
        );

        for (name, values) in [
            ("get_bike", json!({"identifier": ""})),
            ("compare_bikes", json!({"identifiers": ["4513"]})),
        ] {
            let reply = session.call(name, values);
            assert_eq!(reply["result"]["isError"], true, "{reply}");
            assert!(reply["result"]["structuredContent"].is_null());
        }
        for values in [
            json!({"limit":201}),
            json!({"query":null}),
            json!({"locale":"en"}),
        ] {
            let reply = session.call("search_bikes", values);
            assert_eq!(reply["result"]["isError"], true, "{reply}");
            assert!(reply["result"]["structuredContent"].is_null());
        }
    }
}
