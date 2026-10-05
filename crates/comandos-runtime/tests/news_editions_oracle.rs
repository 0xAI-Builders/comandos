//! News editions and radar against the isolated Python reference. No live network.
#[path = "support/python.rs"]
mod python;
use comandos_runtime::{news_editions as editions, news_radar as radar};
use serde_json::{Value, json};

#[test]
fn normalized_urls_match_python() {
    let urls = json!([
        "https://www.Example.com/a//b/?utm_source=x&b=2&a=1#x",
        "http://x.test:80/",
        "https://x.test:443/",
        "https://x.test:8443/",
        "ftp://x.test",
        "https://u:p@x.test",
        "",
        "../a",
        "https://x.test/?q=a+b&q=c&empty=",
        "https://x.test/?REF=x&ref_src=y",
        "https://old.reddit.com/r/ai/",
        "HTTPS://WWW.X.TEST/a",
        "https://x.test////",
        "https://x.test/?s=2&t=3",
        "https://x.test/?UTM_FOO=a",
        "https://x.test/?q=%C3%B1",
        "https://x.test/a%20b",
        "https://x.test/?z=2&z=1",
        "https://x.test/#top",
        "https://x.test/?fbclid=1&ok=2"
    ]);
    let root = std::env::temp_dir().join(format!("news-t4-url-{}", std::process::id()));
    let input = urls.to_string();
    let output = python::run_python("import sys,json; sys.path.insert(0,sys.argv[1]+'/lib'); import news_editions as e, news_radar as r; print(json.dumps([[e.normalize_url(u),r.canonical(u)] for u in json.loads(sys.argv[2])]))", &[input.as_ref()], &root).unwrap();
    let expected: Value = serde_json::from_str(&output).unwrap();
    let actual: Vec<Value> = urls
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            json!([
                editions::normalize_url(u.as_str().unwrap()),
                radar::canonical(u.as_str().unwrap())
            ])
        })
        .collect();
    assert_eq!(json!(actual), expected);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn slots_claim_and_build_one_edition() {
    let conn = comandos_store::state::connect(std::path::Path::new(":memory:")).unwrap();
    comandos_store::state::migrate(&conn, comandos_store::state::MIGRATIONS, 0.0).unwrap();
    let policy = editions::default_policy();
    let slots = editions::slot_times("2026-10-05", &policy).unwrap();
    assert_eq!(slots.len(), 3);
    let now = slots[0].1 + 1;
    let fetch = |_: &serde_json::Map<String, Value>, _: usize| {
        Ok(
            json!({"items":[{"url":"https://example.test/new", "title":"New model", "kind":"modelo", "source":"Lab", "fetchStatus":"ok", "text":"We released a model."}],"failures":[]}),
        )
    };
    let summarize = |_: &Value| {
        Ok(
            json!({"costUsd":0.01,"model":"fake:m1","story":{"title":"Nuevo modelo","summary":"Una noticia.","body":"Detalles.","sourceIds":["s1"]}}),
        )
    };
    let result =
        editions::run_due(&conn, now, &policy, &fetch, &summarize, &|| now, None, None).unwrap();
    assert_eq!(result["edition"]["status"], "published");
    assert_eq!(result["edition"]["storyCount"], 1);
    assert!(
        editions::claim_due_job(&conn, now, &policy)
            .unwrap()
            .is_none()
    );
}

#[test]
fn radar_blocks_keep_article_and_omit_script() {
    let (blocks, meta) = radar::page_blocks(
        "<html lang='es'><meta property='og:title' content='Anuncio'><article><h1>Anuncio</h1><p>Hola <code>modelo</code>.</p><script>evil()</script><p>Texto dos.</p></article></html>",
        "https://example.test/a",
    );
    assert_eq!(meta["lang"], "es");
    assert_eq!(
        blocks,
        json!([{"type":"p","text":"Hola `modelo`."},{"type":"p","text":"Texto dos."}])
            .as_array()
            .unwrap()
            .to_vec()
    );
}

fn oracle(tag: &str, script: &str, input: &Value) -> Value {
    let root = std::env::temp_dir().join(format!("news-t4-{tag}-{}", std::process::id()));
    let data = input.to_string();
    let code = format!(
        "import sys,json,datetime; sys.path.insert(0,sys.argv[1]+'/lib'); import news_editions as e, news_radar as r, app_state; data=json.loads(sys.argv[2]);\n{script}"
    );
    let got = python::run_python(&code, &[data.as_ref()], &root).unwrap();
    let parsed = serde_json::from_str(&got).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    parsed
}
fn memory() -> rusqlite::Connection {
    let c = comandos_store::state::connect(std::path::Path::new(":memory:")).unwrap();
    comandos_store::state::migrate(&c, comandos_store::state::MIGRATIONS, 0.0).unwrap();
    c
}
#[test]
fn prepare_group_opportunities_and_notes_match_python() {
    let input = json!([null,{}, {"url":"ftp://no","title":"Invalid"},{"url":"https://a.test/new?utm_a=x","title":"New","kind":"model","text":"read","fetchStatus":"ok","announcementKey":"launch"},{"url":"https://a.test/new","title":"Duplicate"},{"url":"https://a.test/other","title":"Other","kind":"ia","announcementKey":"launch","meta":{"timezone":"UTC"}},{"url":"https://prize.test/old","title":"Expired","kind":"bounty","meta":{"deadline":1000000000}},{"url":"https://prize.test/new","title":"Win","kind":"hackathon","meta":{"prize":"100 USD","deadline":"2030-01-01","timezone":"UTC"}}]);
    let now = 1791212400000;
    let expected = oracle(
        "prepare",
        "notes=[]; src=e._prepare(data,e.default_policy(),1791212400000,notes); groups=e._groups(src); print(json.dumps([src,notes,groups,[e._opportunity({},g) for g in groups]]))",
        &input,
    );
    let mut notes = vec![];
    let src = editions::prepare(
        input.as_array().unwrap(),
        &editions::default_policy(),
        now,
        &mut notes,
    );
    let groups = editions::groups(&src);
    let opportunities: Vec<_> = groups
        .iter()
        .map(|g| editions::opportunity(&json!({}), g))
        .collect();
    assert_eq!(json!([src, notes, groups, opportunities]), expected);
}
#[test]
fn run_due_outputs_and_database_rows_match_python() {
    let p = editions::default_policy();
    let now = editions::slot_times("2026-10-05", &p).unwrap()[0].1 + 123;
    let items = json!([{"url":"https://a.test/new","title":"New model","source":"Lab","kind":"model","fetchStatus":"ok","text":"An announcement","publishedAt":now-10000,"capture":{"title":"New","blocks":[{"type":"p","text":"Announcement"}],"finalUrl":"https://a.test/new"}},{"url":"https://b.test/post","title":"Community","fetchStatus":"failed","fetchError":"timeout"}]);
    for (mode, response) in [
        (
            "published",
            json!({"model":"fake:m1","costUsd":0.02,"story":{"title":"Nuevo","summary":"Hola","body":"Detalles","sourceIds":["s1"]}}),
        ),
        (
            "nocite",
            json!({"model":"m","story":{"title":"Inventado","sourceIds":["s01"]}}),
        ),
        (
            "overspend",
            json!({"model":"m","costUsd":0.4,"story":{"title":"Costoso","sourceIds":["s1"]}}),
        ),
    ] {
        let input = json!({"now":now,"items":items,"response":response});
        let expected = oracle(
            mode,
            r#"
c=app_state.connect(':memory:'); app_state.migrate(c)
f=lambda p,n: {'items':data['items'],'failures':[]}
s=lambda req: data['response']
result=e.run_due(c,data['now'],e.default_policy(),fetch=f,summarize=s,clock=lambda:data['now'])
tables={t:[list(row) for row in c.execute('SELECT * FROM '+t+' ORDER BY 1')] for t in ['news_editions','news_jobs','news_sources','news_stories','news_story_sources','news_captures']}
for rows in tables.values():
 for row in rows:
  for i,v in enumerate(row):
   if isinstance(v,str) and v[:1] in '[{':
    try: row[i]=json.loads(v)
    except ValueError: pass
print(json.dumps([result,tables,e.recent_story_urls(c,0),e.notice_text(result['edition'])]))
"#,
            &input,
        );
        let conn = memory();
        let items = items.clone();
        let fetch = move |_: &editions::Policy, _| Ok(json!({"items":items,"failures":[]}));
        let summarize = move |_: &Value| Ok(response.clone());
        let result =
            editions::run_due(&conn, now, &p, &fetch, &summarize, &|| now, None, None).unwrap();
        let mut tables = serde_json::Map::new();
        for t in [
            "news_editions",
            "news_jobs",
            "news_sources",
            "news_stories",
            "news_story_sources",
            "news_captures",
        ] {
            let mut st = conn
                .prepare(&format!("SELECT * FROM {t} ORDER BY 1"))
                .unwrap();
            let count = st.column_count();
            let rows: Vec<Value> = st
                .query_map([], |r| {
                    let vals: Vec<Value> = (0..count)
                        .map(|i| match r.get_ref(i).unwrap() {
                            rusqlite::types::ValueRef::Null => Value::Null,
                            rusqlite::types::ValueRef::Integer(n) => n.into(),
                            rusqlite::types::ValueRef::Real(n) => json!(n),
                            rusqlite::types::ValueRef::Text(b) => {
                                let text = std::str::from_utf8(b).unwrap();
                                if text.starts_with(['{', '[']) {
                                    serde_json::from_str(text).unwrap_or_else(|_| text.into())
                                } else {
                                    text.into()
                                }
                            }
                            _ => panic!("unexpected blob"),
                        })
                        .collect();
                    Ok(json!(vals))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect();
            tables.insert(t.into(), json!(rows));
        }
        let notice = editions::notice_text(&result["edition"]);
        assert_eq!(
            json!([
                result,
                tables,
                editions::recent_story_urls(&conn, 0).unwrap(),
                notice
            ]),
            expected,
            "{mode}"
        );
    }
}
#[test]
fn reconcile_requeue_and_slot_claim_match_python() {
    let p = editions::default_policy();
    let now = editions::slot_times("2026-10-05", &p).unwrap()[0].1 + 1;
    let expected = oracle(
        "leases",
        r#"
c=app_state.connect(':memory:');app_state.migrate(c);p=e.default_policy();day=datetime.date(2026,10,5)
slots=e.slot_times(day,p);scheduled=e.schedule_editions(c,day,p);job=e.claim_due_job(c,data['now'],p);busy=e.claim_due_job(c,data['now'],p);e.requeue_orphans(c,data['now']+1,p);again=e.claim_due_job(c,data['now']+2,p);e.reconcile(c,data['now']+4000000,p);e.requeue_orphans(c,data['now']+4000000,p)
print(json.dumps([slots,scheduled,job,busy,again,e.list_editions(c)]))
"#,
        &json!({"now":now}),
    );
    let c = memory();
    let slots = editions::slot_times("2026-10-05", &p).unwrap();
    let scheduled = editions::schedule_editions(&c, "2026-10-05", &p, None).unwrap();
    let job = editions::claim_due_job(&c, now, &p).unwrap();
    let busy = editions::claim_due_job(&c, now, &p).unwrap();
    editions::requeue_orphans(&c, now + 1, &p).unwrap();
    let again = editions::claim_due_job(&c, now + 2, &p).unwrap();
    editions::reconcile(&c, now + 4000000, &p).unwrap();
    editions::requeue_orphans(&c, now + 4000000, &p).unwrap();
    let list = comandos_store::news::list_editions(&c, 30, None).unwrap();
    assert_eq!(json!([slots, scheduled, job, busy, again, list]), expected);
}
#[test]
fn radar_rank_cluster_score_and_heat_match_python() {
    let now = 1791212400;
    let items = json!([
{"family":"oficial","origin":"Anthropic","title":"Introducing Claude 4.8","url":"https://anthropic.com/news/claude-4-8","publishedAt":now-3600,"official":true,"lab":"Anthropic","discussion":null,"signals":{},"summary":"","heat":0.0,"release":false},
{"family":"hn","origin":"Hacker News","title":"Claude 4.8 model available","url":"https://anthropic.com/news/claude-4-8?utm_source=hn","publishedAt":now-2000,"official":false,"lab":null,"discussion":"https://news.ycombinator.com/item?id=1","signals":{"points":120,"comments":44},"summary":"","heat":18.0,"release":false},
{"family":"reddit","origin":"r/LocalLLaMA","title":"My personal website","url":"https://person.test/","publishedAt":now-1000,"official":false,"lab":null,"discussion":"https://reddit.com/1","signals":{"hotRank":1,"subreddit":"LocalLLaMA"},"summary":"","heat":12.0,"release":false},
{"family":"github","origin":"GitHub","title":"New agent runtime 3.0","url":"https://github.com/a/b","publishedAt":null,"official":false,"lab":null,"discussion":null,"signals":{"starsToday":12456,"trendingRank":1},"summary":"","heat":50.0,"release":false}]);
    let expected = oracle(
        "ranking",
        "groups=r.cluster(data['items']); print(json.dumps([groups,[r.score(g,data['now']) for g in groups],r.rank(data['items'],data['now']),[r.heat_label(i) for i in data['items']]]))",
        &json!({"items":items,"now":now}),
    );
    let items = items.as_array().unwrap();
    let groups = radar::cluster(items);
    let scores: Vec<_> = groups.iter().map(|g| radar::score(g, now)).collect();
    let heats: Vec<_> = items.iter().map(radar::heat_label).collect();
    assert_eq!(
        json!([groups, scores, radar::rank(items, now, 6, &[], 10.0), heats]),
        expected
    );
}
#[test]
fn feed_and_page_blocks_match_python() {
    let xml = r#"<rss><channel><item><title>New &amp; good</title><link>https://feed.test/post</link><pubDate>Mon, 05 Oct 2026 10:00:00 GMT</pubDate><description><![CDATA[<p>Hello &amp; hi</p>]]></description></item></channel></rss>"#;
    let pages = json!([
        "<main><h2>Hello</h2><p>A &amp; B.</p><pre>\nlet x = 1;\n</pre><nav><p>Hidden</p></nav><ul><li>Bullet</li></ul><img src='/hero.png' alt='Figure'></main>",
        "<article><p>Subscribe</p><p>Share</p><p>Text one</p><p>Text one</p><p>Text two</p></article>",
        "<meta property='og:title' content='Title'><p>Before</p><article><h1>Title</h1><p>Inside <code>x</code> now</p></article>"
    ]);
    let expected = oracle(
        "parse",
        "print(json.dumps([r.parse_feed(data['xml']),[r.page_blocks(p,'https://feed.test/post') for p in data['pages']]]))",
        &json!({"xml":xml,"pages":pages}),
    );
    let blocks: Vec<_> = pages
        .as_array()
        .unwrap()
        .iter()
        .map(|p| radar::page_blocks(p.as_str().unwrap(), "https://feed.test/post"))
        .collect();
    assert_eq!(json!([radar::parse_feed(xml).unwrap(), blocks]), expected);
}
use std::sync::{Arc, Mutex};
struct Fixtures(Value);
impl radar::Transport for Fixtures {
    fn get(&self, url: &str, _: &radar::Request) -> Result<radar::Response, String> {
        let rows = self.0.as_array().unwrap();
        let r = rows
            .iter()
            .find(|r| url.contains(r["match"].as_str().unwrap()))
            .ok_or_else(|| format!("no fixture {url}"))?;
        let body = if let Some(s) = r["body"].as_str() {
            s.as_bytes().to_vec()
        } else {
            r["body"].to_string().into_bytes()
        };
        Ok(radar::Response {
            status: r["status"].as_u64().unwrap_or(200) as u16,
            body,
            content_type: r["kind"].as_str().unwrap_or("application/json").into(),
            location: r["location"].as_str().map(str::to_owned),
        })
    }
}
fn network(fixtures: Value) -> radar::Network {
    radar::Network {
        transport: Arc::new(Fixtures(fixtures)),
        resolver: Arc::new(|_| true),
    }
}
#[test]
fn all_radar_collectors_match_python_without_network() {
    let now = 1791212400;
    let rss = r#"<rss><channel><item><title>New AI model 3.0</title><link>https://source.test/model</link><pubDate>Mon, 05 Oct 2026 10:00:00 GMT</pubDate><description><![CDATA[<p>New model read here <a href="https://source.test/model">link</a></p>]]></description></item></channel></rss>"#;
    let atom = r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><title>v2.1.0</title><link href="https://github.com/repo/release"/><updated>2026-10-05T10:00:00Z</updated><content>Release</content></entry><entry><title>v3.0-beta</title><link href="https://github.com/repo/beta"/></entry></feed>"#;
    let fixtures = json!([
{"match":"releases.atom","body":atom,"kind":"application/atom+xml"},
{"match":"anthropic.com/news","body":"<script>\"publishedOn\":\"2026-10-05T10:00:00Z\",\"slug\":{\"_type\":\"slug\",\"current\":\"new-claude\"},\"summary\":\"Read more\",\"title\":\"New Claude model\"</script>","kind":"text/html"},
{"match":"/api/models?author=","body":[{"id":"Lab/New-3","createdAt":"2026-10-05T10:00:00Z","likes":24,"downloads":300}]},
{"match":"hn.algolia.com","body":{"hits":[{"objectID":"1","title":"Introducing AI 3.0","url":"https://openai.com/news/model","created_at_i":now-1000,"points":120,"num_comments":42},{"objectID":"2","title":"Gardening today","points":60}]}},
{"match":"reddit.com/","body":rss,"kind":"application/atom+xml"},
{"match":"github.com/trending","body":"<article class=\"Box-row\"><h2><a href=\"/org/ai-model\">AI</a></h2><p class=\"col-9\">New AI model</p><span>1,234 stars today</span></article>","kind":"text/html"},
{"match":"/api/models?sort=","body":[{"id":"Lab/trending","createdAt":"2026-10-05T10:00:00Z","likes":50,"downloads":42},{"id":"old","createdAt":"2020-01-01T00:00:00Z"}]},
{"match":"/api/daily_papers","body":[{"paper":{"id":"2601.123","title":"AI paper","upvotes":20,"summary":"<p>A study</p>"},"publishedAt":"2026-10-05T10:00:00Z"}]},
{"match":"lobste.rs","body":[{"title":"AI lobsters","url":"https://source.test/lobsters","comments_url":"https://lobste.rs/s/a","created_at":"2026-10-05T10:00:00Z","score":15,"comment_count":4}]},
{"match":"api.x.com","body":{"includes":{"users":[{"id":"7","username":"OpenAI","name":"OpenAI"}]},"data":[{"id":"1","author_id":"7","text":"New model","created_at":"2026-10-05T10:00:00Z","public_metrics":{"like_count":1000,"retweet_count":200},"entities":{"urls":[{"expanded_url":"https://openai.com/news/new"}]}}]}},
{"match":"https://","body":rss,"kind":"application/rss+xml"}]);
    let expected = oracle(
        "collectors",
        r#"
def fetch(url,**kw):
 f=next(x for x in data['fixtures'] if x['match'] in url)
 body=f['body'].encode() if isinstance(f['body'],str) else json.dumps(f['body']).encode()
 return True,body,f.get('kind','application/json'),url,None
out=[]
for fn in [r.collect_official,r.collect_hn,r.collect_reddit,r.collect_github_trending,r.collect_hf,r.collect_lobsters,r.collect_press,r.collect_x]:
 errors=[];kw={'sleep':lambda _:None,'clock':lambda:0} if fn==r.collect_reddit else {'env':{'X_BEARER_TOKEN':'fake'}} if fn==r.collect_x else {}
 items=fn(data['now'],fetch,errors,**kw);out.append([items,errors])
print(json.dumps(out))
"#,
        &json!({"now":now,"fixtures":fixtures}),
    );
    let net = network(fixtures);
    let mut result = vec![];
    for (index, collector) in radar::COLLECTORS.iter().enumerate() {
        let mut errors = vec![];
        let items = if index == 2 {
            radar::collect_reddit_with(now, &net, &mut errors, &|_| {}, &|| 0.0)
        } else {
            collector(now, &net, &mut errors)
        };
        result.push(json!([items, errors]));
    }
    let mut errors = vec![];
    let x = radar::collect_x(now, &net, &mut errors, Some("fake"));
    result.push(json!([x, errors]));
    assert_eq!(json!(result), expected);
}
#[test]
fn article_redirect_public_check_and_truncation() {
    let calls = Arc::new(Mutex::new(vec![]));
    let seen = Arc::clone(&calls);
    let net = radar::Network {
        transport: Arc::new(Fixtures(
            json!([{"match":"public.test","status":302,"location":"http://127.0.0.1/private","body":""}]),
        )),
        resolver: Arc::new(move |host| {
            seen.lock().unwrap().push(host.to_owned());
            host == "public.test"
        }),
    };
    let read = editions::read_article("https://public.test/a", &net);
    assert_eq!(read, (false, String::new(), Some("host no público".into())));
    assert_eq!(*calls.lock().unwrap(), vec!["public.test", "127.0.0.1"]);
    let net =
        network(json!([{"match":"large.test","body":"x".repeat(400001),"kind":"text/plain"}]));
    let (ok, text, error) = editions::read_article("https://large.test", &net);
    assert!(ok);
    assert_eq!(text.len(), 6000);
    assert_eq!(error, None);
    assert_eq!(
        net.fetch(
            "https://large.test",
            &radar::Request {
                max_bytes: 400000,
                ..Default::default()
            }
        )
        .unwrap_err(),
        "demasiado grande"
    );
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "169.254.169.254",
        "192.168.1.1",
        "::1",
        "fc00::1",
        "::ffff:127.0.0.1",
    ] {
        assert!(!radar::public_ip(ip.parse().unwrap()), "{ip}")
    }
}
#[test]
fn read_article_against_local_http_server() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut input = [0; 4096];
        let _ = stream.read(&mut input).unwrap();
        let body = "<html><header>Ignore</header><p>Hello &amp; hi.</p><script>evil()</script><p>Readable text.</p></html>";
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    struct Local;
    impl radar::Transport for Local {
        fn get(&self, url: &str, r: &radar::Request) -> Result<radar::Response, String> {
            assert_eq!(url::Url::parse(url).unwrap().host_str(), Some("127.0.0.1"));
            let mut response = reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(r.timeout)
                .build()
                .unwrap()
                .get(url)
                .send()
                .unwrap();
            let kind = response.headers()["content-type"]
                .to_str()
                .unwrap()
                .to_owned();
            let mut body = vec![];
            response.read_to_end(&mut body).unwrap();
            Ok(radar::Response {
                status: 200,
                content_type: kind,
                location: None,
                body,
            })
        }
    }
    let net = radar::Network {
        transport: Arc::new(Local),
        resolver: Arc::new(|h| h == "127.0.0.1"),
    };
    let result = editions::read_article(&format!("http://{address}/article"), &net);
    thread.join().unwrap();
    assert_eq!(result, (true, "Hello & hi.\nReadable text.".into(), None));
}
#[test]
fn public_addresses_match_python_rejection() {
    let addresses = json!([
        "8.8.8.8",
        "100.64.0.1",
        "192.0.0.9",
        "192.0.0.10",
        "192.0.0.8",
        "192.0.2.3",
        "198.18.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "::ffff:8.8.8.8",
        "2001::1",
        "2001:10::1",
        "2001:20::1",
        "2001:db8::1",
        "2002::1",
        "2606:4700:4700::1111",
        "fe80::1",
        "fec0::1"
    ]);
    let expected = oracle(
        "ip",
        "import ipaddress; print(json.dumps([not any([a.is_private,a.is_loopback,a.is_link_local,a.is_multicast,a.is_reserved,a.is_unspecified]) for a in map(ipaddress.ip_address,data)]))",
        &addresses,
    );
    let actual: Vec<_> = addresses
        .as_array()
        .unwrap()
        .iter()
        .map(|s| radar::public_ip(s.as_str().unwrap().parse().unwrap()))
        .collect();
    assert_eq!(json!(actual), expected);
}
#[test]
fn http_summarizer_request_and_usage_match_python() {
    struct Post(Mutex<Vec<Value>>);
    impl editions::Post for Post {
        fn post(
            &self,
            u: &str,
            h: &std::collections::BTreeMap<String, String>,
            b: &Value,
        ) -> editions::Result<(u16, Value)> {
            self.0.lock().unwrap().push(json!([u, h, b]));
            Ok((
                200,
                json!({"choices":[{"message":{"content":"{\"title\":\"Nuevo\",\"sourceIds\":[\"s1\"]}"}}],"usage":{"prompt_tokens":1000,"completion_tokens":200}}),
            ))
        }
    }
    let config = json!({"summarizer":{"kind":"openai-chat","baseUrl":"https://provider.test/v1/","model":"m1","apiKeyEnv":"FAKE_KEY","inputUsdPerMTok":1.0,"outputUsdPerMTok":2.0}});
    let request = json!({"editionId":"e","maxCostUsd":0.1,"language":"es","group":{"key":"g","category":"ia","sources":[{"id":"s1","url":"https://a.test","title":"Title","origin":"Lab","text":"Source","publishedAt":null,"discoveredAt":1,"meta":{}}]}});
    let expected = oracle(
        "http-summary",
        r#"
calls=[]
def post(u,h,b):
 calls.append([u,h,b]);return 200,{'choices':[{'message':{'content':'{"title":"Nuevo","sourceIds":["s1"]}'}}],'usage':{'prompt_tokens':1000,'completion_tokens':200}}
summary=e.make_summarizer(data['config'],env={'FAKE_KEY':'fake'},post=post)
print(json.dumps([summary(data['request']),calls]))
"#,
        &json!({"config":config,"request":request}),
    );
    let post = Arc::new(Post(Mutex::new(vec![])));
    let summarize = editions::make_summarizer(
        &config,
        &|_| Some("fake".into()),
        post.clone(),
        Arc::new(|_, _| panic!("ACP not expected")),
    )
    .unwrap();
    let result = summarize(&request).unwrap();
    assert_eq!(json!([result, *post.0.lock().unwrap()]), expected);
    let mut low = request.clone();
    low["maxCostUsd"] = 0.000001.into();
    assert!(matches!(
        summarize(&low),
        Err(editions::SummaryError::Budget(_))
    ));
    assert_eq!(post.0.lock().unwrap().len(), 1);
}
#[test]
fn chain_demotes_failed_agent_and_always_closes_session() {
    use comandos_runtime::acp_client::{AcpError, AgentSession, Event};
    struct Fake {
        good: bool,
        closed: Arc<Mutex<usize>>,
    }
    impl AgentSession for Fake {
        fn prompt(
            &mut self,
            _: &str,
            event: &mut dyn FnMut(&Event),
            timeout: std::time::Duration,
        ) -> Result<Value, AcpError> {
            assert_eq!(timeout, std::time::Duration::from_secs(12));
            if self.good {
                event(&Event::Text(
                    "{\"title\":\"Nuevo\",\"sourceIds\":[\"s1\"]}".into(),
                ));
                Ok(Value::Null)
            } else {
                Err(AcpError("offline".into()))
            }
        }
        fn close(&mut self) {
            *self.closed.lock().unwrap() += 1;
        }
    }
    let opened = Arc::new(Mutex::new(vec![]));
    let closed = Arc::new(Mutex::new(0));
    let opener = {
        let opened = opened.clone();
        let closed = closed.clone();
        Arc::new(move |agent: &str, _: &str| {
            opened.lock().unwrap().push(agent.to_owned());
            Ok(Box::new(Fake {
                good: agent == "good",
                closed: closed.clone(),
            }) as Box<dyn AgentSession>)
        })
    };
    let config = json!({"summarizer":{"kind":"chain","steps":[{"agent":"bad","model":"m","timeoutSeconds":12},{"agent":"good","model":"m","timeoutSeconds":12}]}});
    let request = json!({"editionId":"e","maxCostUsd":0.1,"language":"es","group":{"key":"g","category":"ia","sources":[]}});
    let summarize =
        editions::make_summarizer(&config, &|_| None, Arc::new(editions::HttpPost), opener)
            .unwrap();
    let first = summarize(&request).unwrap();
    let second = summarize(&request).unwrap();
    assert_eq!(first["model"], "good:m");
    assert_eq!(first["notes"].as_array().unwrap().len(), 1);
    assert_eq!(second["notes"], json!([]));
    assert_eq!(*opened.lock().unwrap(), vec!["bad", "good", "good"]);
    assert_eq!(*closed.lock().unwrap(), 3);
}
#[test]
fn scheduler_try_lock_prevents_overlapping_publications() {
    let scheduler = Arc::new(editions::EditionScheduler::default());
    let config = json!({"enabled":true,"summarizer":{"kind":"acp","agent":"fake","model":"test"}});
    let now = editions::slot_times("2026-10-05", &editions::default_policy()).unwrap()[0].1;
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let thread = {
        let scheduler = scheduler.clone();
        let config = config.clone();
        let barrier = barrier.clone();
        let release = release.clone();
        std::thread::spawn(move || {
            let fetch = move |_: &editions::Policy, _| {
                barrier.wait();
                release.wait();
                Ok(json!({"items":[],"failures":[]}))
            };
            scheduler
                .tick(
                    &|| Ok(memory()),
                    config.as_object(),
                    &|_| false,
                    now,
                    &fetch,
                    &|_| panic!("no sources"),
                    None,
                    None,
                )
                .unwrap()
        })
    };
    barrier.wait();
    let result = scheduler
        .tick(
            &|| panic!("must not open"),
            config.as_object(),
            &|_| false,
            now,
            &|_, _| panic!("must not fetch"),
            &|_| panic!("must not summarize"),
            None,
            None,
        )
        .unwrap();
    assert_eq!(result, json!({"built":null,"busy":true}));
    release.wait();
    assert_eq!(thread.join().unwrap()["edition"]["status"], "empty");
}
#[test]
fn capture_images_fallback_and_discussion_match_python() {
    let media = std::env::temp_dir().join(format!("news-t4-capture-media-{}", std::process::id()));
    let image = "x".repeat(1600);
    let fixtures = json!([{"match":"image.png","body":image,"kind":"image/png"},{"match":"article","body":"<meta property='og:title' content='Story'><meta property='og:image' content='/image.png'><article><p>Short article.</p></article>","kind":"text/html"},{"match":"hn.algolia.com","body":{"text":"<p>Original</p>","children":[{"author":"one","text":"<p>A comment</p>"},{"author":null,"text":"Second"}]}},{"match":"failed","status":500,"body":"no"}]);
    let item = json!({"family":"hn","title":"News","url":"https://a.test/article","discussion":"https://news.ycombinator.com/item?id=1","origin":"Hacker News","publishedAt":123,"signals":{"hnId":"1"}});
    let fallback = json!([{"type":"p","text":"Fallback source text."}]);
    let expected = oracle(
        "captures",
        r#"
import tempfile,os
def fetch(url,**kw):
 f=next(x for x in data['fixtures'] if x['match'] in url)
 if f.get('status',200)>=400:return False,b'', '',url,'HTTP '+str(f['status'])
 return True,(f['body'].encode() if isinstance(f['body'],str) else json.dumps(f['body']).encode()),f.get('kind','application/json'),url,None
with tempfile.TemporaryDirectory(dir=os.environ['HOME']) as media:
 captured=r.capture_page('https://a.test/article',media,fetch=fetch,fallback_blocks=data['fallback'])
 failed=r.capture_page('https://a.test/failed',media,fetch=fetch,fallback_blocks=data['fallback'])
 disc=r.discussion_capture(data['item'],fetch=fetch)
 print(json.dumps([captured,failed,disc,sorted(os.listdir(media)),r.blocks_text(captured[1]['blocks'])]))
"#,
        &json!({"fixtures":fixtures,"item":item,"fallback":fallback}),
    );
    let net = network(fixtures);
    let captured = radar::capture_page(
        "https://a.test/article",
        &media,
        &net,
        6,
        fallback.as_array().unwrap(),
    );
    let failed = radar::capture_page(
        "https://a.test/failed",
        &media,
        &net,
        6,
        fallback.as_array().unwrap(),
    );
    let disc = radar::discussion_capture(&item, &net);
    let mut names: Vec<_> = std::fs::read_dir(&media)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        json!([
            captured,
            failed,
            disc,
            names,
            radar::blocks_text(captured.1["blocks"].as_array().unwrap(), 14000)
        ]),
        expected
    );
    std::fs::remove_dir_all(media).unwrap();
}
#[test]
fn radar_fetcher_selection_and_provenance_match_python() {
    for empty in [false, true] {
        let mut item = json!({"family":"oficial","origin":"Lab","title":"Introducing AI model 4.0","url":"https://www.a.test:8443/article?ref=page2&s=source&t=2&utm_source=test","publishedAt":1791212000,"official":true,"lab":"Lab","discussion":null,"signals":{},"summary":"Fallback article text.","heat":0.0,"release":true});
        let mut item2 = json!({"family":"hn","origin":"Hacker News","title":"Introducing AI model 4.0","url":"https://www.a.test:8443/article?ref=page2&s=source&t=2&utm_source=test","publishedAt":1791212001,"official":false,"lab":null,"discussion":"https://news.ycombinator.com/item?id=1","signals":{"hnId":"1","points":100,"comments":10},"summary":"","heat":20.0,"release":false});
        if empty {
            item["publishedAt"] = json!(0);
            item["summary"] = json!(" ");
            item2["publishedAt"] = json!("");
        }
        let mut current = item2.clone();
        current["publishedAt"] = json!(1791212002);
        let items = json!([item, item2, current]);
        let mut fixtures = json!([{"match":"article","body":"<article><p>A short source.</p></article>","kind":"text/html"},{"match":"hn.algolia.com","body":{"text":"Original","children":[{"author":"a","text":"comment"}]}}]);
        if empty {
            fixtures[1]["body"] = json!({"text":"<p></p>","children":[]});
            fixtures[0]["body"] = json!(
                "<meta property='og:title' content='Empty'><meta property='article:published_time' content='2026-10-05T10:00:00Z'>"
            );
        }
        let expected = oracle(
            "radar-fetcher",
            r#"
import types,tempfile,os
def fetch(url,**kw):
 f=next(x for x in data['fixtures'] if x['match'] in url);return True,(f['body'].encode() if isinstance(f['body'],str) else json.dumps(f['body']).encode()),f.get('kind','application/json'),url,None
with tempfile.TemporaryDirectory(dir=os.environ['HOME']) as media:
 radar=types.SimpleNamespace(collect=lambda ts:{'items':data['items'],'failures':[]}, rank=r.rank, canonical=r.canonical,_primary=r._primary,blocks_text=r.blocks_text,heat_label=r.heat_label,capture_page=lambda u,m,**kw:r.capture_page(u,m,fetch=fetch,**kw),discussion_capture=lambda i:r.discussion_capture(i,fetch=fetch))
 f=e.make_radar_fetcher(radar,media=media,recent_urls=lambda:[],now=lambda:1791212400)
 print(json.dumps(f(e.default_policy(),25)))
"#,
            &json!({"items":items,"fixtures":fixtures}),
        );
        let media =
            std::env::temp_dir().join(format!("news-t4-radar-fetcher-{}", std::process::id()));
        let f = editions::make_radar_fetcher(
            Arc::new(move |_| Ok(json!({"items":items,"failures":[]}))),
            network(fixtures),
            media,
            Arc::new(|| 1791212400),
            Arc::new(|| Ok(vec![])),
            5,
        );
        let actual = f(&editions::default_policy(), 25).unwrap();
        if empty {
            assert!(
                expected["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|i| i["fetchError"] == "sin texto"),
                "{expected}"
            );
        }
        assert_eq!(actual, expected, "empty={empty}");
    }
}
#[test]
fn reddit_retry_and_budget_match_reference() {
    let calls = Arc::new(Mutex::new(0usize));
    struct Limited(Arc<Mutex<usize>>);
    impl radar::Transport for Limited {
        fn get(&self, _: &str, _: &radar::Request) -> Result<radar::Response, String> {
            *self.0.lock().unwrap() += 1;
            Ok(radar::Response {
                status: 429,
                body: vec![],
                content_type: String::new(),
                location: None,
            })
        }
    }
    let sleeps = Mutex::new(vec![]);
    let net = radar::Network {
        transport: Arc::new(Limited(calls.clone())),
        resolver: Arc::new(|_| true),
    };
    let mut errors = vec![];
    let items = radar::collect_reddit_with(
        0,
        &net,
        &mut errors,
        &|d| sleeps.lock().unwrap().push(d.as_secs_f64()),
        &|| 0.0,
    );
    let expected = oracle(
        "reddit-budget",
        r#"
calls=[];sleeps=[];errors=[]
def fetch(url,**kw):
 calls.append(url);return False,b'','',url,'HTTP 429'
items=r.collect_reddit(0,fetch,errors,sleep=sleeps.append,clock=lambda:0)
print(json.dumps([items,errors,len(calls),[float(s) for s in sleeps]]))
"#,
        &Value::Null,
    );
    assert_eq!(
        json!([
            items,
            errors,
            *calls.lock().unwrap(),
            *sleeps.lock().unwrap()
        ]),
        expected
    );
}
#[test]
fn normalize_query_punctuation_unicode_and_ports() {
    let urls = json!([
        "https://www.ñ.test/a?q=~*&empty=&bad=%FF",
        "https://host.test:0/a",
        "https://host.test:bad/a",
        "https://host.test:99999/a",
        "https://[2001:4860:4860::8888]:443/a",
        "https://host.test/a/../b",
        "http://host.test/?q=x%20y&x=%2F&x=+"
    ]);
    let expected = oracle(
        "url-punctuation",
        "print(json.dumps([[e.normalize_url(u),r.canonical(u)] for u in data]))",
        &urls,
    );
    let result: Vec<_> = urls
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            json!([
                editions::normalize_url(u.as_str().unwrap()),
                radar::canonical(u.as_str().unwrap())
            ])
        })
        .collect();
    assert_eq!(json!(result), expected);
}
#[test]
fn page_streaming_preserves_title_dedup_and_raw_script_text() {
    let pages = json!([
        "<meta property='og:title' content='Same'><article><h1>Same</h1><h1>Same</h1><p>Same</p><p>Different</p></article>",
        "<article><h1>Same</h1><p>Same</p><p>Different</p></article>",
        "<article><script>const fake='<script><p>not HTML</p>';</script><p>After script.</p></article>",
        format!(
            "<meta property='og:title' content='Title'><article><h1>Title</h1>{}</article>",
            (0..200)
                .map(|n| format!("<p>Paragraph {n}</p>"))
                .collect::<String>()
        )
    ]);
    let expected = oracle(
        "streaming",
        "print(json.dumps([r.page_blocks(p,'https://a.test/article') for p in data]))",
        &pages,
    );
    let actual: Vec<_> = pages
        .as_array()
        .unwrap()
        .iter()
        .map(|p| radar::page_blocks(p.as_str().unwrap(), "https://a.test/article"))
        .collect();
    assert_eq!(json!(actual), expected);
}
#[test]
fn watch_fetcher_round_robin_matches_python() {
    let items = json!([{"url":"https://a.test/1?utm_x=1","title":"A1","source":"A","kind":"modelo","at":10,"publishedAt":100},{"url":"https://a.test/2","title":"A2","source":"A","kind":"ia","at":9,"publishedAt":90},{"url":"https://b.test/1","title":"B1","source":"B","kind":"ia","at":8,"publishedAt":80},{"url":"https://a.test/1","title":"Dup","source":"A","kind":"ia","at":7}]);
    let expected = oracle(
        "watch-fetcher",
        r#"
import types
watch=types.SimpleNamespace(collect=lambda ts:{'items':data,'failures':[{'source':'missing','error':'offline'}]})
f=e.make_fetcher(watch,reader=lambda u:(True,'body',None),now=lambda:1791212400)
print(json.dumps(f(e.default_policy(),3)))
"#,
        &items,
    );
    let collect = Arc::new(move |_| {
        Ok(json!({"items":items,"failures":[{"source":"missing","error":"offline"}]}))
    });
    let fetch = editions::make_fetcher(
        collect,
        network(json!([{"match":"https://","body":"body","kind":"text/plain"}])),
        Arc::new(|| 1791212400),
    );
    assert_eq!(fetch(&editions::default_policy(), 3).unwrap(), expected);
}
#[test]
fn time_and_budget_stop_before_another_summary() {
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    let p = editions::default_policy();
    let now = editions::slot_times("2026-10-05", &p).unwrap()[0].1;
    let fetch = |_: &editions::Policy, _| {
        Ok(
            json!({"items":[{"url":"https://a.test/a","title":"A","fetchStatus":"ok","text":"A"},{"url":"https://b.test/b","title":"B","fetchStatus":"ok","text":"B"}],"failures":[]}),
        )
    };
    for deadline in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let summarize = move |_: &Value| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"costUsd":"0.245","model":"fake","story":{"title":"A","sourceIds":["s1"]}}))
        };
        let clock = AtomicI64::new(now);
        let get_clock = || {
            if deadline {
                clock.fetch_add(1_500_000, Ordering::SeqCst)
            } else {
                now
            }
        };
        let result = editions::run_due(
            &memory(),
            now,
            &p,
            &fetch,
            &summarize,
            &get_clock,
            None,
            None,
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(!deadline));
        if deadline {
            assert_eq!(result["edition"]["status"], "failed");
            assert!(
                result["edition"]["notes"][0]
                    .as_str()
                    .unwrap()
                    .contains("límite de tiempo")
            );
        } else {
            assert_eq!(result["edition"]["status"], "partial");
            assert_eq!(result["edition"]["costUsd"], 0.245);
            assert!(
                result["edition"]["notes"][0]
                    .as_str()
                    .unwrap()
                    .contains("presupuesto")
            );
        }
    }
}
#[test]
fn redirect_without_location_is_an_http_error() {
    let net = network(
        json!([{"match":"missing-location.test","status":302,"body":"","kind":"text/html"},{"match":"not-modified.test","status":304,"body":"","kind":"text/html"}]),
    );
    for (url, code) in [
        ("https://missing-location.test", 302),
        ("https://not-modified.test", 304),
    ] {
        assert_eq!(
            net.fetch(url, &radar::Request::default()).unwrap_err(),
            format!("HTTP {code}")
        );
        assert_eq!(
            editions::read_article(url, &net),
            (false, String::new(), Some(format!("HTTP {code}")))
        );
    }
}
#[test]
fn slots_use_python_fold_zero_for_dst_gaps_and_ambiguities() {
    let cases = json!([{"day":"2026-03-08","timezone":"America/New_York","slots":["02:30"]},{"day":"2026-11-01","timezone":"America/New_York","slots":["01:30"]},{"day":"2011-12-30","timezone":"Pacific/Apia","slots":["09:00"]}]);
    let expected = oracle(
        "dst",
        "print(json.dumps([e.slot_times(datetime.date.fromisoformat(c['day']),c) for c in data]))",
        &cases,
    );
    let actual: Vec<_> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|c| editions::slot_times(c["day"].as_str().unwrap(), c.as_object().unwrap()).unwrap())
        .collect();
    assert_eq!(json!(actual), expected);
}
