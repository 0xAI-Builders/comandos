use super::{clip, re};
use std::{
    collections::BTreeMap,
    io::Read,
    net::{IpAddr, ToSocketAddrs},
    sync::Arc,
    time::Duration,
};
use url::Url;
pub const MAX_URL_BYTES: usize = 16 * 1024;
pub const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Safari/537.36 ComandOS-radar/2.0";
#[derive(Clone)]
pub struct Request {
    pub accept: String,
    pub max_bytes: usize,
    pub timeout: Duration,
    pub headers: BTreeMap<String, String>,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".into(),
            max_bytes: 2_500_000,
            timeout: Duration::from_secs(15),
            headers: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: String,
    pub location: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Fetched {
    pub body: Vec<u8>,
    pub content_type: String,
    pub final_url: String,
}
/// Transport must honor the size/deadline in Request and never follow redirects.
/// PublicClient is the production transport; test transports may serve loopback.
pub trait Transport: Send + Sync {
    fn get(&self, url: &str, request: &Request) -> Result<Response, String>;
}
pub type Resolver = Arc<dyn Fn(&str) -> bool + Send + Sync>;
#[derive(Clone)]
pub struct Network {
    pub transport: Arc<dyn Transport>,
    pub resolver: Resolver,
}
impl Network {
    pub fn production() -> Self {
        Self {
            transport: Arc::new(PublicClient),
            resolver: Arc::new(public_host),
        }
    }
    pub fn fetch(&self, url: &str, request: &Request) -> Result<Fetched, String> {
        let mut current = url.to_owned();
        for _ in 0..8 {
            if current.len() > MAX_URL_BYTES {
                return Err("URL demasiado larga".into());
            }
            let parsed = Url::parse(&current).map_err(|_| "URL no válida")?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err("URL no válida".into());
            }
            if !(self.resolver)(parsed.host_str().ok_or("URL no válida")?) {
                return Err("host no público".into());
            }
            let response = self.transport.get(&current, request)?;
            if matches!(response.status, 301 | 302 | 303 | 307 | 308)
                && response.location.as_ref().is_some_and(|s| !s.is_empty())
            {
                current = parsed
                    .join(
                        response
                            .location
                            .as_deref()
                            .ok_or("redirección no válida")?,
                    )
                    .map_err(|_| "redirección no válida")?
                    .to_string();
                continue;
            }
            if response.status >= 300 {
                return Err(format!("HTTP {}", response.status));
            }
            if response.body.len() > request.max_bytes {
                return Err("demasiado grande".into());
            }
            return Ok(Fetched {
                body: response.body,
                content_type: response.content_type,
                final_url: current,
            });
        }
        Err("demasiadas redirecciones".into())
    }
    pub fn text(&self, url: &str, accept: &str) -> Result<String, String> {
        let f = self.fetch(
            url,
            &Request {
                accept: accept.into(),
                ..Request::default()
            },
        )?;
        Ok(decode(&f.body, &f.content_type))
    }
    pub fn json(&self, url: &str) -> Result<serde_json::Value, String> {
        serde_json::from_str(&self.text(url, "application/json")?).map_err(|e| e.to_string())
    }
}
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            !a.is_private()
                && !a.is_loopback()
                && !a.is_link_local()
                && !a.is_multicast()
                && !a.is_unspecified()
                && !a.is_broadcast()
                && !a.is_documentation()
                && o[0] != 0
                && o[0] < 240
                && !(o[0] == 192 && o[1] == 0 && o[2] == 0 && !matches!(o[3], 9 | 10))
                && !(o[0] == 198 && (o[1] == 18 || o[1] == 19))
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            let exception = s[0] == 0x2001
                && (s[1] == 3
                    || (s[1] == 4 && s[2] == 0x112)
                    || (s[1] & 0xfff0 == 0x20)
                    || (s[1] & 0xfff0 == 0x30)
                    || (s[1] == 1 && s[2..7].iter().all(|v| *v == 0) && matches!(s[7], 1 | 2)));
            // Same ipaddress private/reserved tables as the reference. IPv4
            // mapped IPv6 is reserved even when its embedded address is public.
            ((s[0] & 0xe000 == 0x2000) || (s[0] & 0xffc0 == 0xfec0))
                && !(s[0] == 0x2001 && s[1] < 0x200 && !exception)
                && !(s[0] == 0x2001 && s[1] == 0xdb8)
                && s[0] != 0x2002
        }
    }
}
pub fn public_host(host: &str) -> bool {
    (host.trim_matches(['[', ']']), 0)
        .to_socket_addrs()
        .ok()
        .is_some_and(|mut a| {
            let v: Vec<_> = a.by_ref().collect();
            !v.is_empty() && v.iter().all(|v| public_ip(v.ip()))
        })
}
/// Re-resolves and pins the validated addresses: no DNS rebinding between check/connect.
pub struct PublicClient;
impl Transport for PublicClient {
    fn get(&self, url: &str, request: &Request) -> Result<Response, String> {
        if url.len() > MAX_URL_BYTES {
            return Err("URL demasiado larga".into());
        }
        let parsed = Url::parse(url).map_err(|e| e.to_string())?;
        let host = parsed
            .host_str()
            .ok_or("URL no válida")?
            .trim_matches(['[', ']']);
        let addresses: Vec<_> = (host, parsed.port_or_known_default().ok_or("URL no válida")?)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .collect();
        if addresses.is_empty() || addresses.iter().any(|a| !public_ip(a.ip())) {
            return Err("host no público".into());
        }
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(request.timeout)
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|e| e.to_string())?;
        let mut req = client
            .get(url)
            .header("User-Agent", UA)
            .header("Accept", &request.accept)
            .header("Accept-Language", "en,es;q=0.8");
        for (k, v) in &request.headers {
            req = req.header(k, v);
        }
        let response = req.send().map_err(|e| clip(&e.to_string(), 120))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut body = Vec::new();
        response
            .take(request.max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut body)
            .map_err(|e| e.to_string())?;
        Ok(Response {
            status,
            body,
            content_type,
            location,
        })
    }
}
pub fn decode(body: &[u8], kind: &str) -> String {
    let pattern = re(r"(?i)charset=([\w-]+)");
    let caps = pattern.captures(kind);
    let enc = caps
        .as_ref()
        .and_then(|c| encoding_rs::Encoding::for_label(c[1].as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    enc.decode(body).0.into_owned()
}
/// Python preserves the path bytes (including dot segments), unlike WHATWG Url.
pub(crate) fn normalized(url: &str, radar: bool) -> Option<String> {
    let raw = url.trim().replace(['\t', '\r', '\n'], "");
    let (scheme, rest) = raw.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    let (hostname, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let (hostname, tail) = bracketed.split_once(']')?;
        hostname.parse::<std::net::Ipv6Addr>().ok()?;
        (hostname, tail.strip_prefix(':'))
    } else {
        let (hostname, port) = authority
            .split_once(':')
            .map_or((authority, None), |(h, p)| (h, Some(p)));
        if hostname.is_empty() || hostname.contains(['[', ']']) {
            return None;
        }
        (hostname, port)
    };
    let mut host = hostname.to_lowercase();
    host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    if radar && host == "old.reddit.com" {
        host = "reddit.com".into()
    }
    if !radar && let Some(port) = port.filter(|p| !p.is_empty()) {
        let port = port.parse::<u16>().ok()?;
        if port != 0 && !((scheme == "https" && port == 443) || (scheme == "http" && port == 80)) {
            host.push_str(&format!(":{port}"));
        }
    }
    let tail = rest[end..].split('#').next().unwrap_or("");
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let mut path = re(r"/{2,}")
        .replace_all(if path.is_empty() { "/" } else { path }, "/")
        .into_owned();
    if path.len() > 1 {
        path = path.trim_end_matches('/').into()
    }
    let pattern = if radar {
        r"(?i)^(utm_.*|fbclid|gclid|mc_cid|mc_eid|ref_src|ref|igshid|s|t)$"
    } else {
        r"(?i)^(utm_.*|fbclid|gclid|mc_cid|mc_eid|ref_src|igshid)$"
    };
    let track = re(pattern);
    let mut pairs: Vec<_> = url::form_urlencoded::parse(query.as_bytes())
        .filter(|(k, _)| !track.is_match(k))
        .collect();
    pairs.sort();
    let q = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", quote_plus(k), quote_plus(v)))
        .collect::<Vec<_>>()
        .join("&");
    Some(format!(
        "{scheme}://{host}{path}{}",
        if q.is_empty() {
            String::new()
        } else {
            format!("?{q}")
        }
    ))
}
fn quote_plus(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}
pub fn canonical(url: &str) -> Option<String> {
    normalized(url, true)
}
