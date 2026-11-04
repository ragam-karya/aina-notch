// Against the real Composio account, read-only: a session, its meta tools and which apps are linked.
// The app keeps no keys (Badi Cloud holds them), so the key comes from the environment:
// $env:COMPOSIO_API_KEY = "ak_…"; cargo test -p badi-apps --test live -- --ignored --nocapture

use badi_apps::composio::{self, Opts};

#[test]
#[ignore]
fn opens_a_session_and_reads_status() {
    let store = badi_store::Store::open(badi_store::data_dir());
    let key = std::env::var("COMPOSIO_API_KEY").expect("set COMPOSIO_API_KEY");
    println!("key kind: {}", &key[..3]);
    let o = Opts { api_key: key, user_id: store.get("composioUserId").as_str().unwrap_or("badi_test").into(), timezone: String::new(), auth_configs: store.get("composioAuthConfigs") };
    let tools = composio::with_session(&o, |s| Ok(s.tools()?.iter().map(|t| t["name"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>())).unwrap();
    println!("tools: {tools:?}");
    assert!(tools.iter().any(|t| t == "COMPOSIO_MULTI_EXECUTE_TOOL"));
    let mut linked: Vec<String> = composio::status(&o).unwrap().into_iter().filter(|(_, on)| *on).map(|(s, _)| s).collect();
    linked.sort();
    println!("linked: {linked:?}");
}
