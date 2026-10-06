//! Original Analytics markup with model calculations owned by Rust.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::OnceLock;
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Number(n) => ns(n.as_f64().unwrap_or(0.)),
        _ => v.to_string(),
    }
}
fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.)
}
fn ns(n: f64) -> String {
    if n == 0. { "0".into() } else { n.to_string() }
}
fn fixed(n: f64, places: usize) -> String {
    // Number.toFixed rounds the exact binary value, with ties away from zero.
    // Multiplying in f64 first loses that distinction (1.15 becomes 1.2).
    // The original renderer uses only zero and one fractional digit.
    if !n.is_finite() || n.abs() >= 1e21 {
        return ns(n);
    }
    let bits = n.abs().to_bits();
    let raw_exponent = ((bits >> 52) & 0x7ff) as i32;
    let mantissa =
        u128::from(bits & ((1_u64 << 52) - 1)) | if raw_exponent == 0 { 0 } else { 1_u128 << 52 };
    let exponent = if raw_exponent == 0 {
        -1074
    } else {
        raw_exponent - 1075
    };
    let scale = 10_u128.pow(places as u32);
    let numerator = mantissa * scale;
    let rounded = if exponent >= 0 {
        numerator.checked_shl(exponent as u32).unwrap_or(0)
    } else {
        let shift = (-exponent) as u32;
        if shift >= 128 {
            0
        } else {
            let denominator = 1_u128 << shift;
            (numerator >> shift) + u128::from(numerator & (denominator - 1) >= denominator / 2)
        }
    };
    let sign = if n < 0. { "-" } else { "" };
    if places == 0 {
        format!("{sign}{rounded}")
    } else {
        let whole = rounded / scale;
        let fraction = rounded % scale;
        format!("{sign}{whole}.{fraction:0places$}")
    }
}
fn esc(s: &str) -> String {
    crate::escape::text(s).replace('🍅', "&#127813;")
}
fn arr(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn field(v: &Value, k: &str) -> String {
    text(at(v, k))
}
fn n(v: &Value, k: &str) -> f64 {
    num(at(v, k))
}
fn tpl(id: usize, vals: &[String]) -> String {
    static PARTS: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    let p = PARTS.get_or_init(|| {
        serde_json::from_str(include_str!("analytics_templates.json")).unwrap_or_default()
    });
    let mut out = String::new();
    if let Some(parts) = p.get(id) {
        for (i, s) in parts.iter().enumerate() {
            out.push_str(s);
            if let Some(v) = vals.get(i) {
                out.push_str(v)
            }
        }
    }
    out
}
macro_rules! t {($id:expr $(,$v:expr)* $(,)?)=>{tpl($id,&[$(($v).to_string()),*])};}
fn fmt_h(h: f64) -> String {
    let mut hh = h.floor() as i64;
    let mut mm = ((h - h.floor()) * 60. + 0.5).floor() as i64;
    if mm == 60 {
        hh += 1;
        mm = 0;
    }
    format!("{hh:02}:{mm:02}")
}
fn dur(h: f64) -> String {
    if h == 0. {
        "—".into()
    } else if h < 1. {
        format!("{} min", ns((h * 60. + 0.5).floor()))
    } else {
        format!("{} h", fixed(h, 1))
    }
}
fn fmin(m: f64) -> String {
    if m >= 60. {
        format!("{} h", fixed(m / 60., 1))
    } else {
        format!("{} min", ns(m))
    }
}
fn tok(x: f64) -> String {
    if x >= 1000. {
        format!("{}B", fixed(x / 1000., 1))
    } else {
        format!("{}M", ns((x + 0.5).floor()))
    }
}
fn hh(h: usize) -> String {
    format!("{h:02}:00")
}
fn unique(v: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    for s in v {
        if !out.contains(&s) {
            out.push(s)
        }
    }
    out
}
fn sums(rows: &[Value], key: &str) -> f64 {
    rows.iter().map(|r| n(r, key)).sum()
}
fn completed(f: &Value) -> bool {
    at(f, "status") == "completado"
}
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(b) {
            out.push(*b as char)
        } else {
            out.push_str(&format!("%{b:02X}"))
        }
    }
    out
}
#[derive(Clone)]
struct AxisSeg {
    a: f64,
    b: f64,
    y: f64,
    h: f64,
    on: bool,
}
struct Axis {
    segs: Vec<AxisSeg>,
    height: f64,
}
impl Axis {
    fn y(&self, t: f64) -> f64 {
        let Some(s) = self
            .segs
            .iter()
            .find(|s| t >= s.a && t <= s.b)
            .or_else(|| self.segs.last())
        else {
            return 0.;
        };
        s.y + (t - s.a) * if s.on { 34. } else { 16. / (s.b - s.a) }
    }
    fn labels(&self) -> String {
        self.segs
            .iter()
            .map(|s| {
                if s.on {
                    if s.a as i64 % 2 == 0 {
                        t!(22, ns(s.y), format!("{:02}", s.a as i64))
                    } else {
                        String::new()
                    }
                } else {
                    t!(
                        23,
                        ns(s.y + s.h / 2.),
                        format!("{:02}", s.a as i64),
                        format!("{:02}", s.b as i64)
                    )
                }
            })
            .collect()
    }
    fn bands(&self) -> String {
        self.segs
            .iter()
            .map(|s| {
                if s.on {
                    t!(24, ns(s.y))
                } else {
                    t!(25, ns(s.y), ns(s.h))
                }
            })
            .collect()
    }
}
pub struct Renderer {
    model: Value,
    view: Value,
    accounts: Vec<Value>,
    sessions: Vec<Value>,
    focus: Vec<Value>,
    days: Vec<Value>,
    last: Value,
    uid: usize,
}
impl Renderer {
    pub fn new(model: Value, view: Value) -> Self {
        let accounts = arr(at(&model, "accounts"))
            .into_iter()
            .map(|mut a| {
                {
                    let next = at(&a, "color").clone();
                    *slot(&mut a, &("c")) = next;
                }
                for k in ["cli", "alias"] {
                    {
                        let next = esc(&field(&a, k)).into();
                        *slot(&mut a, &(k)) = next;
                    }
                }
                if at(&a, "model").is_object() {
                    {
                        let next = esc(&field(at(&a, "model"), "n")).into();
                        *slot(slot(&mut a, &("model")), &("n")) = next;
                    }
                }
                a
            })
            .collect();
        let sessions = arr(at(&model, "sessions"))
            .into_iter()
            .map(|mut s| {
                {
                    let next = esc(&field(&s, "proj")).into();
                    *slot(&mut s, &("proj")) = next;
                }
                {
                    let next = json!(n(&s, "en").max(n(&s, "st") + 1. / 60.).min(24.));
                    *slot(&mut s, &("en")) = next;
                }
                s
            })
            .collect();
        let focus = arr(at(&model, "pomodoros"))
            .into_iter()
            .map(|mut f| {
                {
                    let next = esc(&field(&f, "proj")).into();
                    *slot(&mut f, &("proj")) = next;
                }
                {
                    let next = if at(&f, "status") == "completed" {
                        "completado"
                    } else {
                        "cancelado"
                    }
                    .into();
                    *slot(&mut f, &("status")) = next;
                }
                {
                    let next = json!({});
                    *slot(&mut f, &("ag")) = next;
                }
                f
            })
            .collect();
        let mut last = json!({});
        if let Some(m) = at(&model, "lastWeek").as_object() {
            for (k, v) in m {
                {
                    let next = v.clone();
                    *slot(&mut last, &(esc(k))) = next;
                }
            }
        }
        Self {
            days: arr(at(&model, "days")),
            accounts,
            sessions,
            focus,
            last,
            model,
            view,
            uid: 0,
        }
    }
    fn acc(&self, id: &str) -> Value {
        self.accounts
            .iter()
            .find(|a| at(a, "id") == id)
            .cloned()
            .unwrap_or(Value::Null)
    }
    fn acc_name(&self, id: &str) -> String {
        let a = self.acc(id);
        format!("{} {}", field(&a, "cli"), field(&a, "alias"))
    }
    fn past(&self) -> bool {
        n(at(&self.model, "week"), "offset") < 0.
    }
    fn phone(&self) -> bool {
        at(&self.view, "phone").as_bool().unwrap_or(false)
    }
    fn label(&self) -> String {
        field(at(&self.model, "week"), "label")
    }
    fn today(&self) -> Value {
        at(at(&self.model, "week"), "today").clone()
    }
    fn next(&mut self, prefix: &str) -> String {
        let s = format!("{prefix}{}", self.uid);
        self.uid += 1;
        s
    }
    fn defs(id: &str, c: &str) -> String {
        t!(0, id, c, c, id, id, id, id, id)
    }
    fn floor(id: &str, cx: f64, y: f64, rx: f64) -> String {
        t!(8, ns(cx), ns(y), ns(rx), ns(rx * 0.13), id)
    }
    fn glass(id: &str, path: &str, hl: String) -> String {
        t!(7, path, id, path, hl)
    }
    fn logos() -> &'static Value {
        static LOGOS: OnceLock<Value> = OnceLock::new();
        LOGOS.get_or_init(|| {
            serde_json::from_str(include_str!("analytics_logos.json")).unwrap_or(Value::Null)
        })
    }
    fn color_logo(k: &str, size: f64, x: f64, y: f64) -> String {
        let logo = text(&Self::logos()[k]);
        let start = logo.find('>').map(|i| i + 1).unwrap_or(0);
        let inner = logo
            .get(start..)
            .unwrap_or("")
            .strip_suffix("</svg>")
            .unwrap_or("")
            .replacen("fill=\"currentColor\"", "", 1)
            .replacen("<path d=\"M9.27", "<path fill=\"#fff\" d=\"M9.27", 1);
        t!(9, ns(x), ns(y), ns(size), ns(size), inner)
    }
    fn logo_tile(a: &Value, size: f64) -> String {
        let logo = text(&Self::logos()[field(a, "provider")]);
        t!(
            10,
            ns(size),
            ns(size),
            logo.replacen(
                "<svg",
                &format!(
                    "<svg width=\"{}\" height=\"{}\"",
                    ns(size - 8.),
                    ns(size - 8.)
                ),
                1
            )
            .replacen("fill=\"currentColor\"", "fill=\"#fff\"", 1)
        )
    }
    fn liquid(
        id: &str,
        path: &str,
        dims: (f64, f64, f64, f64),
        q: f64,
        c: &str,
        period: f64,
    ) -> String {
        let (w, h, top, bot) = dims;
        let y = bot - (bot - top) * q / 100.;
        let count = ((w + 2. * period) / (period / 2.)).ceil() as usize;
        let wave = t!(1, ns(y))
            + (0..count)
                .map(|i| {
                    t!(
                        2,
                        ns(period / 4.),
                        ns(if i % 2 != 0 { 2.6 } else { -2.6 }),
                        ns(period / 2.)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
                .as_str()
            + &t!(3, ns(h));
        let bubbles = if q > 6. {
            [0.3, 0.55, 0.7, 0.42]
                .iter()
                .zip([1.6, 1.1, 2., 1.3])
                .enumerate()
                .map(|(i, (f, r))| {
                    t!(
                        4,
                        ns(w * f),
                        ns(bot - 6.),
                        ns(r),
                        ns(-(bot - y - 10.)),
                        ns(i as f64 * 1.1),
                        ns(3.6 + i as f64 * 0.7)
                    )
                })
                .collect()
        } else {
            String::new()
        };
        let fill = if q > 0. {
            t!(
                6,
                ns(-period),
                wave,
                c,
                ns(-period / 2.),
                ns(-period),
                wave,
                id,
                ns(y - 0.6),
                ns(w),
                bubbles
            )
        } else {
            String::new()
        };
        t!(5, id, path, id, ns(w), ns(h), fill, ns(w), ns(h), id)
    }
    fn bottle(&mut self, q: f64, c: &str, k: &str, name: &str) -> String {
        let id = self.next("w");
        let path = "M24 56 Q21 44 34 42 L62 42 Q75 44 72 56 L76 68 L76 188 Q76 200 64 200 L32 200 Q20 200 20 188 L20 68 Z";
        let cap = (0..6)
            .map(|i| t!(12, ns(34. + i as f64 * 5.4)))
            .collect::<String>();
        t!(
            11,
            96,
            210,
            96,
            210,
            name,
            ns(q),
            Self::defs(&id, c),
            Self::floor(&id, 48., 203., 36.),
            id,
            cap,
            Self::liquid(&id, path, (96., 210., 72., 196.), q, c, 56.),
            Self::glass(&id, path, t!(13)),
            Self::color_logo(k, 14., 41., 110.),
            name.to_uppercase().replacen("SESIÓN ", "", 1)
        )
    }
    fn limits(a: &Value) -> Vec<Value> {
        let mut l = Vec::new();
        if !at(a, "week").is_null() {
            l.push(json!({"n":"Semana","q":100.-n(a,"week"),"left":at(a, "left")}));
        }
        if at(a, "model").is_object() {
            let m = at(a, "model");
            l.push(json!({"n":at(m, "n"),"q":100.-n(m,"v"),"left":if at(m, "left").is_null(){at(a, "left")}else{at(m, "left")}}));
        }
        if !at(a, "h5").is_null() {
            l.push(json!({"n":"Sesión 5 h","q":100.-n(a,"h5"),"left":at(a, "h5Left")}));
        }
        l
    }
    fn capt(l: &Value) -> String {
        let q = n(l, "q");
        t!(
            14,
            field(l, "n"),
            if q <= 10. {
                "bad"
            } else if q <= 30. {
                "warn"
            } else {
                ""
            },
            ns(q),
            if at(l, "left").is_null() {
                String::new()
            } else {
                format!("reset en {}", field(l, "left"))
            }
        )
    }
    fn shelf(&mut self) -> String {
        let accounts = self.accounts.clone();
        let mut groups = String::new();
        let mut stats = String::new();
        for a in accounts.into_iter().filter(|a| !Self::limits(a).is_empty()) {
            let c = field(&a, "c");
            let mut cols = String::new();
            for l in Self::limits(&a) {
                let q = n(&l, "q");
                cols += &t!(
                    17,
                    self.bottle(
                        q,
                        if q <= 10. {
                            "#FF7580"
                        } else if q <= 30. {
                            "#FFAE1A"
                        } else {
                            &c
                        },
                        &field(&a, "provider"),
                        &field(&l, "n")
                    ),
                    Self::capt(&l)
                );
            }
            groups += &t!(
                16,
                c,
                cols,
                Self::logo_tile(&a, 22.),
                field(&a, "cli"),
                field(&a, "alias")
            );
            stats += &t!(
                18,
                c,
                field(&a, "cli"),
                field(&a, "alias"),
                dur(n(at(&a, "hoy"), "h")),
                field(at(&a, "hoy"), "ses"),
                dur(n(at(&a, "sem"), "h")),
                field(at(&a, "sem"), "ses")
            );
        }
        t!(15, groups, stats)
    }
    fn projects(rows: &[Value]) -> Vec<Value> {
        let mut m = json!({});
        for s in rows {
            let p = field(s, "proj");
            let id = field(s, "acc");
            if !at(&m, p.clone()).is_object() {
                {
                    let next = json!({"p":p,"h":0.,"n":0,"acc":{}});
                    *slot(&mut m, &(p.clone())) = next;
                }
            }
            let v = slot(&mut m, &(p));
            let h = n(s, "en") - n(s, "st");
            {
                let next = json!(n(v, "h") + h);
                *slot(v, &("h")) = next;
            }
            {
                let next = json!(n(v, "n") + 1.);
                *slot(v, &("n")) = next;
            }
            {
                let next = json!(num(at(at(v, "acc"), id.clone())) + h);
                *slot(slot(v, &("acc")), &(id.clone())) = next;
            }
        }
        let mut out = Vec::new();
        if let Some(m) = m.as_object() {
            for v in m.values() {
                let id = at(v, "acc")
                    .as_object()
                    .and_then(|a| a.iter().max_by(|a, b| num(a.1).total_cmp(&num(b.1))))
                    .map(|(k, _)| k.clone())
                    .unwrap_or_default();
                let mut v = v.clone();
                {
                    let next = id.into();
                    *slot(&mut v, &("acc")) = next;
                }
                out.push(v);
            }
        }
        out.sort_by(|a, b| n(b, "h").total_cmp(&n(a, "h")));
        out
    }
    fn week_head(&self) -> String {
        let all = &self.sessions;
        let h = all.iter().map(|s| n(s, "en") - n(s, "st")).sum();
        let ps = Self::projects(all);
        let top = ps.first();
        t!(
            19,
            dur(h),
            unique(all.iter().map(|s| field(s, "d"))).len(),
            all.len(),
            top.map(|v| field(v, "p")).unwrap_or("—".into()),
            top.map(|v| dur(n(v, "h"))).unwrap_or_default(),
            ""
        )
    }
    fn day_rows(&self, d: &Value) -> Vec<Value> {
        self.sessions
            .iter()
            .filter(|s| at(s, "d") == d)
            .cloned()
            .collect()
    }
    fn cal_head(&self, d: &Value, wd: &str, dd: &str) -> String {
        let ss = self.day_rows(d);
        t!(
            21,
            wd,
            dd,
            dur(ss.iter().map(|s| n(s, "en") - n(s, "st")).sum()),
            ss.len(),
            ""
        )
    }
    fn axis(&self) -> Axis {
        let mut active = [false; 24];
        for s in &self.sessions {
            for h in (n(s, "st").floor() as usize)..(n(s, "en").ceil() as usize) {
                if let Some(v) = active.get_mut(h) {
                    *v = true;
                }
            }
        }
        let mut segs = Vec::new();
        let mut h = 0;
        let mut y = 0.;
        while h < 24 {
            let on = active.get(h).copied().unwrap_or(false);
            let mut e = h + 1;
            if !on {
                while e < 24 && !active.get(e).copied().unwrap_or(false) {
                    e += 1;
                }
            }
            if on || e - h < 2 {
                segs.push(AxisSeg {
                    a: h as f64,
                    b: (h + 1) as f64,
                    y,
                    h: 34.,
                    on: true,
                });
                h += 1;
                y += 34.;
            } else {
                segs.push(AxisSeg {
                    a: h as f64,
                    b: e as f64,
                    y,
                    h: 16.,
                    on: false,
                });
                h = e;
                y += 16.;
            }
        }
        Axis { segs, height: y }
    }
    fn merged(rows: Vec<Value>) -> Vec<Value> {
        let mut groups = json!({});
        for mut s in rows {
            {
                let next = 1.into();
                *slot(&mut s, &("n")) = next;
            }
            let k = format!("{}|{}", field(&s, "proj"), field(&s, "acc"));
            if !at(&groups, k.clone()).is_array() {
                {
                    let next = json!([]);
                    *slot(&mut groups, &(k.clone())) = next;
                }
            }
            if let Some(a) = slot(&mut groups, &k).as_array_mut() {
                a.push(s)
            }
        }
        let mut out = Vec::new();
        if let Some(groups) = groups.as_object() {
            for a in groups.values() {
                let mut rows = arr(a);
                rows.sort_by(|a, b| n(a, "st").total_cmp(&n(b, "st")));
                let mut cur: Option<Value> = None;
                for s in rows {
                    if let Some(c) = cur.as_mut().filter(|c| n(&s, "st") <= n(c, "en") + 0.17) {
                        {
                            let next = json!(n(c, "en").max(n(&s, "en")));
                            *slot(c, &("en")) = next;
                        }
                        {
                            let next = json!(n(c, "n") + 1.);
                            *slot(c, &("n")) = next;
                        }
                    } else {
                        if let Some(c) = cur.take() {
                            out.push(c)
                        }
                        cur = Some(s)
                    }
                }
                if let Some(c) = cur {
                    out.push(c)
                }
            }
        }
        out.sort_by(|a, b| n(a, "st").total_cmp(&n(b, "st")));
        out
    }
    fn lanes(rows: Vec<Value>) -> Vec<Value> {
        fn flush(cl: &mut Vec<Value>, out: &mut Vec<Value>) {
            let mut ends: Vec<f64> = Vec::new();
            for s in cl.iter_mut() {
                let idx = ends
                    .iter()
                    .position(|e| *e <= n(s, "st") + 0.01)
                    .unwrap_or(ends.len());
                if idx == ends.len() {
                    ends.push(0.)
                }
                if let Some(e) = ends.get_mut(idx) {
                    *e = n(s, "en");
                }
                {
                    let next = idx.into();
                    *slot(s, &("l")) = next;
                }
            }
            for mut s in cl.drain(..) {
                {
                    let next = ends.len().into();
                    *slot(&mut s, &("nl")) = next;
                }
                out.push(s)
            }
        }
        let mut out = Vec::new();
        let mut cl = Vec::new();
        let mut end = -1_f64;
        for s in rows {
            if !cl.is_empty() && n(&s, "st") >= end {
                flush(&mut cl, &mut out)
            }
            end = end.max(n(&s, "en"));
            cl.push(s)
        }
        flush(&mut cl, &mut out);
        out
    }
    fn tip(&self, s: &Value) -> String {
        let d = self
            .days
            .iter()
            .find(|d| at(d, 0) == at(s, "d"))
            .cloned()
            .unwrap_or(Value::Null);
        t!(
            20,
            format!("{} {}", text(at(&d, 1)), text(at(&d, 2))),
            fmt_h(n(s, "st")),
            fmt_h(n(s, "en")),
            dur(n(s, "en") - n(s, "st")),
            if n(s, "n") > 1. {
                format!(" · {} sesiones unidas", field(s, "n"))
            } else {
                String::new()
            },
            field(s, "proj"),
            self.acc_name(&field(s, "acc"))
        )
    }
    fn calendar(&self, days: &[Value]) -> String {
        let ax = self.axis();
        let mut cols = String::new();
        for d in days {
            let day = at(d, 0);
            let wd = text(at(d, 1));
            let dd = text(at(d, 2));
            let rows = Self::lanes(Self::merged(self.day_rows(day)));
            let mut events = String::new();
            let mut more: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
            for s in rows {
                let lane = n(&s, "l");
                if lane >= 2. {
                    more.entry(n(&s, "st").floor() as usize)
                        .or_default()
                        .push(s);
                    continue;
                }
                let top = ax.y(n(&s, "st"));
                let h = ax.y(n(&s, "en")) - top;
                let inner = if h >= 30. {
                    t!(
                        28,
                        field(&s, "proj"),
                        fmt_h(n(&s, "st")),
                        if n(&s, "n") > 1. {
                            format!(" · ×{}", field(&s, "n"))
                        } else {
                            String::new()
                        }
                    )
                } else if h >= 16. {
                    t!(29, field(&s, "proj"))
                } else {
                    String::new()
                };
                events += &t!(
                    27,
                    field(&self.acc(&field(&s, "acc")), "c"),
                    ns(top),
                    ns(7_f64.max(h - 2.)),
                    ns(lane / 2_f64.min(n(&s, "nl")) * 100.),
                    ns(100. / 2_f64.min(n(&s, "nl"))),
                    self.tip(&s),
                    inner
                );
            }
            let mut buttons = String::new();
            for (h, ss) in more {
                let data: Vec<Value> = ss
                    .iter()
                    .map(|s| {
                        json!([
                            format!("{}–{}", fmt_h(n(s, "st")), fmt_h(n(s, "en"))),
                            at(s, "proj"),
                            self.acc_name(&field(s, "acc")),
                            at(&self.acc(&field(s, "acc")), "c")
                        ])
                    })
                    .collect();
                buttons += &t!(
                    30,
                    ns(ax.y(h as f64)),
                    encode(&serde_json::to_string(&data).unwrap_or_default()),
                    wd,
                    dd,
                    format!("{h:02}"),
                    ss.len()
                );
            }
            let today = *day == self.today();
            cols += &t!(
                26,
                if today { "today" } else { "" },
                self.cal_head(day, &wd, &dd),
                ns(ax.height),
                ax.bands(),
                events,
                buttons,
                if today {
                    t!(31, ns(ax.y(n(at(&self.model, "week"), "now"))))
                } else {
                    String::new()
                }
            );
        }
        self.week_head() + &t!(32, ns(ax.height), ax.labels(), cols)
    }
    fn calendar_cuentas(&self) -> String {
        let chrono: Vec<_> = self.days.iter().rev().cloned().collect();
        if !self.phone() {
            return self.calendar(&chrono);
        }
        let last = chrono.len().saturating_sub(3);
        let current = at(&self.view, "phoneDay")
            .as_u64()
            .map(|v| v as usize)
            .unwrap_or(last)
            .min(last);
        let selected: Vec<_> = chrono.iter().skip(current).take(3).cloned().collect();
        t!(
            78,
            if current == 0 { "disabled" } else { "" },
            selected
                .iter()
                .map(|d| format!("{} {}", text(at(d, 1)), text(at(d, 2))))
                .collect::<Vec<_>>()
                .join(" · "),
            if current >= last { "disabled" } else { "" }
        ) + &self.calendar(&selected)
    }
    fn used(a: &Value) -> f64 {
        if a.get("weekUsed").is_none() {
            n(a, "week")
        } else {
            n(a, "weekUsed")
        }
    }
    fn costs(&self) -> Vec<Value> {
        let mut ha = json!({});
        for s in &self.sessions {
            let id = field(s, "acc");
            {
                let next = json!(num(at(&ha, id.clone())) + n(s, "en") - n(s, "st"));
                *slot(&mut ha, &(id.clone())) = next;
            }
        }
        let mut projects = json!({});
        for s in &self.sessions {
            let p = field(s, "proj");
            let id = field(s, "acc");
            if !at(&projects, p.clone()).is_object() {
                {
                    let next = json!({"p":p,"h":0.,"tok":0.,"acc":{}});
                    *slot(&mut projects, &(p.clone())) = next;
                }
            }
            let v = slot(&mut projects, &(p));
            let h = n(s, "en") - n(s, "st");
            {
                let next = json!(n(v, "h") + h);
                *slot(v, &("h")) = next;
            }
            {
                let next = json!(n(v, "tok") + n(s, "tok"));
                *slot(v, &("tok")) = next;
            }
            if !at(at(v, "acc"), id.clone()).is_object() {
                {
                    let next = json!({"h":0.,"tok":0.,"q":0.});
                    *slot(slot(v, &("acc")), &(id.clone())) = next;
                }
            }
            let a = slot(slot(v, &("acc")), &(id.clone()));
            {
                let next = json!(n(a, "h") + h);
                *slot(a, &("h")) = next;
            }
            {
                let next = json!(n(a, "tok") + n(s, "tok"));
                *slot(a, &("tok")) = next;
            }
            {
                let next =
                    json!(n(a, "q") + h / num(at(&ha, id.clone())) * Self::used(&self.acc(&id)));
                *slot(a, &("q")) = next;
            }
        }
        let mut out = projects
            .as_object()
            .map(|p| p.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        out.sort_by(|a, b| n(b, "tok").total_cmp(&n(a, "tok")));
        out
    }
    fn franjas(&self) -> Vec<Value> {
        [
            ("Madrugada", 0., 6.),
            ("Mañana", 6., 12.),
            ("Tarde", 12., 18.),
            ("Noche", 18., 24.),
        ]
        .iter()
        .map(|(name, a, b)| {
            let mut h = 0.;
            let mut projects = json!({});
            for s in &self.sessions {
                let overlap = 0_f64.max(n(s, "en").min(*b) - n(s, "st").max(*a));
                if overlap > 0. {
                    h += overlap;
                    let p = field(s, "proj");
                    {
                        let next = json!(num(at(&projects, p.clone())) + overlap);
                        *slot(&mut projects, &(p.clone())) = next;
                    }
                }
            }
            let mut top = projects
                .as_object()
                .map(|p| p.iter().map(|(p, h)| json!([p, h])).collect::<Vec<_>>())
                .unwrap_or_default();
            top.sort_by(|a, b| num(at(b, 1)).total_cmp(&num(at(a, 1))));
            top.truncate(3);
            json!({"n":name,"a":a,"b":b,"h":h,"top":top})
        })
        .collect()
    }
    fn p_hour(&self) -> String {
        let fs = self.franjas();
        let max = fs.iter().map(|f| n(f, "h")).fold(1., f64::max);
        t!(
            35,
            fs.iter()
                .map(|f| t!(
                    36,
                    ns(n(f, "h") / max * 100.),
                    dur(n(f, "h")),
                    field(f, "n"),
                    format!("{:02}", n(f, "a") as usize),
                    format!("{:02}", n(f, "b") as usize),
                    {
                        let ps = arr(at(f, "top"))
                            .iter()
                            .map(|v| text(at(v, 0)))
                            .collect::<Vec<_>>()
                            .join(" · ");
                        if ps.is_empty() { "—".into() } else { ps }
                    }
                ))
                .collect::<String>()
        )
    }
    fn vs_week(&self) -> (Vec<Value>, f64, f64) {
        let mut cur = json!({});
        for s in &self.sessions {
            let p = field(s, "proj");
            {
                let next = json!(num(at(&cur, p.clone())) + n(s, "en") - n(s, "st"));
                *slot(&mut cur, &(p.clone())) = next;
            }
        }
        let keys = unique(
            cur.as_object()
                .into_iter()
                .flat_map(|m| m.keys().cloned())
                .chain(
                    self.last
                        .as_object()
                        .into_iter()
                        .flat_map(|m| m.keys().cloned()),
                ),
        );
        let mut rows:Vec<_>=keys.into_iter().map(|p|json!({"p":p,"now":at(&cur, p.clone()),"prev":at(&self.last, p.clone()),"d":num(at(&cur, p.clone()))-num(at(&self.last, p))})).filter(|v|n(v,"now")!=0.||n(v,"prev")!=0.).collect();
        rows.sort_by(|a, b| n(b, "d").abs().total_cmp(&n(a, "d").abs()));
        let now = sums(&rows, "now");
        let prev = sums(&rows, "prev");
        (rows, now, prev)
    }
    fn layered(&mut self, a: &Value, parts: &[Value]) -> String {
        let id = self.next("lb");
        let path = "M30 64 Q26 50 42 47 L68 47 Q84 50 80 64 L86 80 L86 206 Q86 220 72 220 L38 220 Q24 220 24 206 L24 80 Z";
        let mut y = 216.;
        let mut layers = String::new();
        for p in parts {
            let h = 132. * n(p, "q") / 100.;
            layers += &t!(
                37,
                ns(y - h),
                110,
                ns(h),
                field(p, "c"),
                field(p, "o"),
                field(p, "p"),
                fixed(n(p, "q"), 1),
                self.acc_name(&field(a, "id")),
                ns(y - h),
                110
            );
            y -= h;
        }
        t!(
            38,
            110,
            230,
            110,
            230,
            self.acc_name(&field(a, "id")),
            Self::defs(&id, &field(a, "c")),
            Self::floor(&id, 55., 224., 40.),
            id,
            id,
            path,
            id,
            110,
            230,
            layers,
            110,
            230,
            id,
            Self::glass(&id, path, t!(39)),
            ns(y),
            ns(y)
        )
    }
    fn bottles(&mut self) -> String {
        let cd = self.costs();
        let mut groups = String::new();
        for a in self.accounts.clone() {
            let id = field(&a, "id");
            let mut parts: Vec<_> = cd
                .iter()
                .filter(|p| at(at(p, "acc"), id.clone()).is_object())
                .map(|p| json!({"p":at(p, "p"),"q":at(at(at(p, "acc"), id.clone()), "q")}))
                .collect();
            parts.sort_by(|a, b| n(b, "q").total_cmp(&n(a, "q")));
            let rest: f64 = parts.iter().skip(5).map(|p| n(p, "q")).sum();
            parts.truncate(5);
            if rest > 0. {
                parts.push(json!({"p":"otros","q":rest}));
            }
            for (i, p) in parts.iter_mut().enumerate() {
                {
                    let next = at(&a, "c").clone();
                    *slot(p, &("c")) = next;
                }
                {
                    let next = ["1", ".78", ".6", ".46", ".34", ".26"]
                        .get(i)
                        .copied()
                        .unwrap_or("")
                        .into();
                    *slot(p, &("o")) = next;
                }
            }
            let legend = parts
                .iter()
                .map(|p| {
                    t!(
                        63,
                        field(&a, "c"),
                        field(p, "o"),
                        field(p, "p"),
                        fixed(n(p, "q"), 1)
                    )
                })
                .collect::<String>();
            let used = if a.get("weekUsed").unwrap_or(at(&a, "week")).is_null() {
                "—".into()
            } else {
                format!("{}%", ns(100. - Self::used(&a)))
            };
            groups += &t!(
                62,
                self.layered(&a, &parts),
                legend,
                if self.past() {
                    "quedó al reset"
                } else {
                    "queda"
                },
                used,
                Self::logo_tile(&a, 22.),
                field(&a, "cli"),
                field(&a, "alias")
            );
        }
        t!(61, groups)
    }
    fn insights(&self) -> String {
        let cd = self.costs();
        let (v, tn, tp) = self.vs_week();
        let mut fs = self.franjas();
        fs.sort_by(|a, b| n(b, "h").total_cmp(&n(a, "h")));
        let mut waste = arr(at(&self.model, "waste"));
        waste.retain(|w| !arr(at(w, "cyc")).is_empty());
        waste.sort_by(|a, b| num(at(at(b, "cyc"), 0)).total_cmp(&num(at(at(a, "cyc"), 0))));
        let mut out = String::new();
        if let Some(top) = cd.first() {
            let mut acc = at(top, "acc")
                .as_object()
                .map(|m| m.iter().collect::<Vec<_>>())
                .unwrap_or_default();
            acc.sort_by(|a, b| n(b.1, "q").total_cmp(&n(a.1, "q")));
            let title = acc
                .first()
                .map(|(id, a)| t!(43, field(top, "p"), fixed(n(a, "q"), 0), self.acc_name(id)))
                .unwrap_or_default();
            let body = t!(
                44,
                tok(n(top, "tok")),
                dur(n(top, "h")),
                cd.iter()
                    .skip(1)
                    .take(2)
                    .map(|p| field(p, "p"))
                    .collect::<Vec<_>>()
                    .join(" y ")
            );
            let viz = t!(
                45,
                cd.iter()
                    .take(5)
                    .map(|p| t!(
                        46,
                        field(p, "p"),
                        ns(n(p, "tok") / n(top, "tok") * 100.),
                        tok(n(p, "tok"))
                    ))
                    .collect::<String>()
            );
            out += &t!(42, "Lo más caro", title, body, viz);
        }
        if let Some(w) = waste.first() {
            let cyc = arr(at(w, "cyc"));
            let mut rest = String::new();
            if let Some(hi) = waste.iter().skip(1).find(|w| num(at(at(w, "cyc"), 0)) > 5.) {
                rest += &t!(
                    40,
                    self.acc_name(&field(hi, "id")),
                    text(at(at(hi, "cyc"), 0))
                );
            }
            let low: Vec<_> = waste
                .iter()
                .skip(1)
                .filter(|w| num(at(at(w, "cyc"), 0)) <= 5.)
                .collect();
            if !low.is_empty() {
                rest += &t!(
                    41,
                    low.iter()
                        .map(|w| self.acc_name(&field(w, "id")))
                        .collect::<Vec<_>>()
                        .join(" y "),
                    if low.len() > 1 {
                        "se agotan"
                    } else {
                        "se agota"
                    }
                );
            }
            let title = t!(
                47,
                self.acc_name(&field(w, "id")),
                text(at(at(w, "cyc"), 0))
            );
            let body = t!(
                48,
                if cyc.len() > 1 {
                    t!(49, cyc.iter().map(text).collect::<Vec<_>>().join("%, "))
                } else {
                    String::new()
                },
                rest
            );
            let viz = t!(
                50,
                waste
                    .iter()
                    .map(|w| {
                        let c = field(&self.acc(&field(w, "id")), "c");
                        let q = num(at(at(w, "cyc"), 0));
                        t!(
                            51,
                            c,
                            self.acc_name(&field(w, "id")),
                            ns(100. - q),
                            c,
                            ns(q),
                            if q >= 40. { "warnT" } else { "" },
                            ns(q)
                        )
                    })
                    .collect::<String>()
            );
            out += &t!(42, "Cuota que sobra", title, body, viz);
        }
        let title = t!(
            52,
            if tn >= tp { "Trabajaste" } else { "Bajaste" },
            dur((tn - tp).abs()),
            if tn >= tp { "más" } else { "menos" }
        );
        let body = v
            .iter()
            .find(|r| n(r, "d") > 0.)
            .map(|r| t!(53, field(r, "p"), dur(n(r, "prev")), dur(n(r, "now"))))
            .unwrap_or_default();
        let viz = t!(
            54,
            v.iter()
                .take(4)
                .map(
                    |r| t!(55, field(r, "p"), dur(n(r, "prev")), dur(n(r, "now")), {
                        let d = n(r, "d");
                        if d > 0.05 {
                            t!(33, dur(d))
                        } else if d < -0.05 {
                            t!(34, dur(-d))
                        } else {
                            "<b class=\"eq\">=</b>".into()
                        }
                    })
                )
                .collect::<String>()
        );
        out += &t!(42, "Vs la semana pasada", title, body, viz);
        if let Some(p) = cd.iter().find(|p| {
            at(p, "acc")
                .as_object()
                .map(|a| a.len() > 1)
                .unwrap_or(false)
        }) {
            let m = at(p, "acc").as_object();
            let viz = t!(
                57,
                m.into_iter()
                    .flat_map(|m| m.iter())
                    .map(|(id, x)| t!(
                        58,
                        field(&self.acc(id), "c"),
                        field(x, "h"),
                        ns((n(x, "h") / n(p, "h") * 100. + 0.5).floor())
                    ))
                    .collect::<String>()
            );
            out += &t!(
                42,
                "Cuentas mezcladas",
                t!(56, field(p, "p"), m.map(|m| m.len()).unwrap_or(0)),
                "Si quieres separar el gasto por cliente, cada proyecto debería vivir en una sola cuenta.",
                viz
            );
        }
        if let Some(f) = fs.first().filter(|f| n(f, "h") > 0.) {
            out += &t!(
                42,
                "Tu horario",
                t!(59, field(f, "n").to_lowercase()),
                t!(
                    60,
                    dur(n(f, "h")),
                    format!("{:02}", n(f, "a") as usize),
                    format!("{:02}", n(f, "b") as usize),
                    arr(at(f, "top"))
                        .iter()
                        .map(|p| text(at(p, 0)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                self.p_hour()
                    .replacen("<h5>¿A qué hora trabajas?</h5>", "", 1)
                    .replacen("class=\"panel\"", "class=\"bare\"", 1)
            );
        }
        out
    }
    fn focus_projects(fs: &[Value]) -> Vec<Value> {
        let mut m = json!({});
        for f in fs {
            let p = field(f, "proj");
            if !at(&m, p.clone()).is_object() {
                {
                    let next = json!({"p":p,"min":0.,"n":0,"done":0});
                    *slot(&mut m, &(p.clone())) = next;
                }
            }
            let v = slot(&mut m, &(p));
            {
                let next = json!(n(v, "min") + n(f, "act"));
                *slot(v, &("min")) = next;
            }
            {
                let next = json!(n(v, "n") + 1.);
                *slot(v, &("n")) = next;
            }
            if completed(f) {
                {
                    let next = json!(n(v, "done") + 1.);
                    *slot(v, &("done")) = next;
                }
            }
        }
        let mut out = m
            .as_object()
            .map(|m| m.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        out.sort_by(|a, b| n(b, "min").total_cmp(&n(a, "min")));
        out
    }
    fn proj_color(&self, p: &str) -> &str {
        let ps = Self::focus_projects(&self.focus);
        let idx = ps
            .iter()
            .position(|v| at(v, "p") == p)
            .filter(|i| *i < 7)
            .unwrap_or(7);
        [
            "#FF6B5B", "#2fd3c0", "#8B7CFF", "#FFAE1A", "#4CC2FF", "#C5E35A", "#FF9AD5", "#9AA6BF",
        ]
        .get(idx)
        .copied()
        .unwrap_or("#9AA6BF")
    }
    fn tomato(&mut self, ok: bool, size: usize, title: &str) -> String {
        let id = self.next("tm");
        t!(
            66,
            if ok { "" } else { "x" },
            size,
            size,
            title,
            title,
            id,
            if ok { "#ff8a73" } else { "#3a4256" },
            if ok { "#ef3b2c" } else { "#2a3142" },
            if ok { "#a81a10" } else { "#1c2230" },
            id,
            if ok {
                ""
            } else {
                "stroke=\"#ff758066\" stroke-dasharray=\"2 2\""
            },
            if ok { "#3fae5a" } else { "#4a5368" },
            if ok {
                "<ellipse cx=\"11\" cy=\"15\" rx=\"3\" ry=\"2\" fill=\"#fff\" opacity=\".35\" transform=\"rotate(-30 11 15)\"/>"
            } else {
                ""
            }
        )
    }
    fn pnums(&self) -> String {
        let fs = &self.focus;
        let today: Vec<_> = fs
            .iter()
            .filter(|f| *at(f, "d") == self.today())
            .cloned()
            .collect();
        let days = unique(fs.iter().map(|f| field(f, "d"))).len();
        let done = fs.iter().filter(|f| completed(f)).count();
        let k = |label: &str, v: String, e: String| {
            t!(
                67,
                label,
                v,
                if e.is_empty() {
                    String::new()
                } else {
                    t!(68, e)
                }
            )
        };
        t!(
            69,
            k(
                "Hoy",
                format!("{} 🍅", today.iter().filter(|f| completed(f)).count()),
                fmin(sums(&today, "act"))
            ),
            k(
                "Esta semana",
                format!("{done} 🍅"),
                format!("{} de foco", fmin(sums(fs, "act")))
            ),
            k("Cancelados", (fs.len() - done).to_string(), String::new()),
            k(
                "Promedio por día",
                fixed(done as f64 / days.max(1) as f64, 1),
                format!("en {days} días con pomodoros")
            )
        )
    }
    fn p_per_hour(&self) -> String {
        let mut hours = [(0_usize, 0_usize); 24];
        for f in &self.focus {
            if let Some(h) = hours.get_mut(n(f, "st").floor() as usize) {
                if completed(f) { h.0 += 1 } else { h.1 += 1 }
            }
        }
        let max = hours.iter().map(|h| h.0 + h.1).max().unwrap_or(1).max(1) as f64;
        let best = hours
            .iter()
            .enumerate()
            .fold(
                (0, 0),
                |best, (i, h)| if h.0 > best.1 { (i, h.0) } else { best },
            )
            .0;
        t!(
            70,
            hh(best),
            hours
                .iter()
                .enumerate()
                .map(|(i, h)| t!(
                    71,
                    hh(i),
                    hh(i + 1),
                    h.0,
                    if h.1 > 0 {
                        format!(" · {} cancelados", h.1)
                    } else {
                        String::new()
                    },
                    ns(h.1 as f64 / max * 100.),
                    ns(h.0 as f64 / max * 100.),
                    if i % 3 == 0 {
                        format!("{i:02}")
                    } else {
                        String::new()
                    }
                ))
                .collect::<String>()
        )
    }
    fn p_per_proj(&self) -> String {
        let ps = Self::focus_projects(&self.focus);
        let max = ps.first().map(|p| n(p, "n")).unwrap_or(1.);
        let mut rows = ps
            .iter()
            .map(|p| {
                t!(
                    73,
                    self.proj_color(&field(p, "p")),
                    field(p, "p"),
                    ns(n(p, "done") / max * 100.),
                    self.proj_color(&field(p, "p")),
                    field(p, "done"),
                    fmin(n(p, "min"))
                )
            })
            .collect::<String>();
        if rows.is_empty() {
            rows = "<div class=\"dim\">Sin pomodoros.</div>".into();
        }
        t!(72, rows)
    }
    fn focus_day(&self, d: &Value) -> Vec<Value> {
        self.focus
            .iter()
            .filter(|f| at(f, "d") == d)
            .cloned()
            .collect()
    }
    fn projects_chips(&self, fs: &[Value]) -> String {
        let mut m = json!({});
        for f in fs.iter().filter(|f| completed(f)) {
            let p = field(f, "proj");
            {
                let next = json!(num(at(&m, p.clone())) + 1.);
                *slot(&mut m, &(p.clone())) = next;
            }
        }
        m.as_object()
            .into_iter()
            .flat_map(|m| m.iter())
            .map(|(p, n)| t!(77, self.proj_color(p), p, text(n)))
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn time_chips(fs: &[Value]) -> String {
        fs.iter()
            .map(|f| t!(76, if completed(f) { "" } else { "x" }, fmt_h(n(f, "st"))))
            .collect()
    }
    fn tomatoes(&mut self, fs: &[Value]) -> String {
        fs.iter()
            .map(|f| self.tomato(completed(f), 18, &field(f, "proj")))
            .collect()
    }
    fn pomodoro(&mut self) -> String {
        let nums = self.pnums();
        let mut rows = String::new();
        for d in self.days.clone() {
            let fs = self.focus_day(at(&d, 0));
            let ok = fs.iter().filter(|f| completed(f)).count();
            let tomatoes = self.tomatoes(&fs);
            let times = Self::time_chips(&fs);
            let projects = self.projects_chips(&fs);
            let today = *at(&d, 0) == self.today();
            if self.phone() {
                rows += &t!(
                    80,
                    if today { "today" } else { "" },
                    text(at(&d, 1)),
                    text(at(&d, 2)),
                    tomatoes,
                    ok,
                    if fs.is_empty() {
                        "<div class=\"dim\">sin pomodoros</div>".into()
                    } else {
                        t!(81, times, projects, fmin(sums(&fs, "act")))
                    }
                );
            } else {
                rows += &t!(
                    75,
                    if today { "today" } else { "" },
                    text(at(&d, 1)),
                    text(at(&d, 2)),
                    tomatoes,
                    ok,
                    if times.is_empty() {
                        "<span class=\"dim\">—</span>".into()
                    } else {
                        times
                    },
                    if projects.is_empty() {
                        "<span class=\"dim\">—</span>".into()
                    } else {
                        projects
                    },
                    fmin(sums(&fs, "act"))
                );
            }
        }
        if self.phone() {
            nums + &t!(79, rows) + &self.p_per_hour() + &t!(84, self.p_per_proj())
        } else {
            nums + &t!(
                74,
                rows,
                self.focus.iter().filter(|f| completed(f)).count(),
                fmin(sums(&self.focus, "act")),
                self.p_per_hour(),
                self.p_per_proj()
            )
        }
    }
    pub fn html(&mut self, tab: &str) -> String {
        self.uid = 0;
        let body = match tab {
            "cuentas" => {
                let past = if self.past() {
                    "<div class=\"wk-note\" style=\"margin:0 0 8px\">Las botellas muestran tu cuota de ahora; el calendario es de la semana que elegiste.</div>"
                } else {
                    ""
                };
                past.to_string()
                    + &self.shelf()
                    + &t!(
                        85,
                        if self.past() { "Semana" } else { "Esta semana" },
                        self.label()
                    )
                    + &self.calendar_cuentas()
            }
            "comparar" => {
                t!(
                    64,
                    "¿Qué proyecto cuesta más?",
                    "lo gastado de cada cuenta, en capas por proyecto"
                ) + &self.bottles()
                    + &t!(64, "Hallazgos de la semana", self.label())
                    + &t!(65, self.insights())
            }
            _ => self.pomodoro(),
        };
        let tabs = [
            ("cuentas", "Cuentas"),
            ("comparar", "Comparar"),
            ("pomodoro", "Pomodoro"),
        ]
        .iter()
        .map(|(k, label)| t!(87, if *k == tab { "on" } else { "" }, *k == tab, k, label))
        .collect::<String>();
        let offset = n(at(&self.model, "week"), "offset");
        let min = at(&self.view, "minOffset").as_f64().unwrap_or(-1.);
        let out = t!(
            86,
            tabs,
            if offset <= min { "disabled" } else { "" },
            if offset != 0. {
                "Semana"
            } else {
                "Esta semana"
            },
            self.label(),
            if offset >= 0. { "disabled" } else { "" },
            body
        );
        let tin = t!(88, self.tomato(true, 16, ""));
        out.replace('🍅', &tin)
    }
    pub fn phone_days(&self) -> usize {
        self.days.len()
    }
}

pub fn at<K: serde_json::value::Index>(v: &Value, k: K) -> &Value {
    v.get(k).unwrap_or(&Value::Null)
}
fn slot<'a, K: AsRef<str>>(v: &'a mut Value, k: &K) -> &'a mut Value {
    if !v.is_object() {
        *v = json!({});
    }
    match v {
        Value::Object(m) => m.entry(k.as_ref()).or_insert(Value::Null),
        other => other,
    }
}
