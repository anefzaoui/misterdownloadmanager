//! Proxy auto-config (Options → Proxy / Socks → "Use automatic configuration script"): the
//! script's `FindProxyForURL(url, host)` decides the proxy for each address. It runs in a small
//! JavaScript engine (Boa), with the helper functions PAC scripts expect written here (our own,
//! from the PAC definition) and DNS answered by the system. Answers are kept per script and host.

use std::collections::HashMap;
use std::net::{IpAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex};

use boa_engine::{Context, JsResult, JsString, JsValue, NativeFunction, Source, js_string};
use url::Url;

/// Where a request goes, in the script's order of preference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Direct,
    /// A proxy address: `http://host:port`, `https://…` or `socks5://…`.
    Proxy(String),
}

/// The routes for `url` under `script`: its answer's entries that MDM can use, in order (the
/// first is taken). Err if the script doesn't run.
pub fn routes(script: &Arc<str>, url: &Url) -> Result<Vec<Route>, String> {
    use std::hash::{Hash, Hasher};
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    // Kept per script (its text's hash) and host.
    let mut h = std::collections::hash_map::DefaultHasher::new();
    script.hash(&mut h);
    let key = (h.finish(), host.clone());
    if let Some(r) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(&key)) {
        return Ok(r.clone());
    }
    let answer = run(script, url.as_str(), &host)?;
    let r = parse(&answer);
    CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(key, r.clone());
    Ok(r)
}

type Answers = HashMap<(u64, String), Vec<Route>>;
static CACHE: Mutex<Option<Answers>> = Mutex::new(None);

/// `FindProxyForURL`'s answer: `PROXY a:8080; SOCKS b:1080; DIRECT` → routes, unknown kinds
/// skipped. An empty answer means direct.
pub fn parse(answer: &str) -> Vec<Route> {
    let mut out: Vec<Route> = answer
        .split(';')
        .filter_map(|entry| {
            let mut it = entry.split_whitespace();
            let kind = it.next()?.to_ascii_uppercase();
            let at = it.next();
            match (kind.as_str(), at) {
                ("DIRECT", _) => Some(Route::Direct),
                ("PROXY" | "HTTP", Some(a)) => Some(Route::Proxy(format!("http://{a}"))),
                ("HTTPS", Some(a)) => Some(Route::Proxy(format!("https://{a}"))),
                ("SOCKS" | "SOCKS5", Some(a)) => Some(Route::Proxy(format!("socks5://{a}"))),
                ("SOCKS4", Some(a)) => Some(Route::Proxy(format!("socks4://{a}"))),
                _ => None,
            }
        })
        .collect();
    if out.is_empty() {
        out.push(Route::Direct);
    }
    out
}

/// Run the script and call `FindProxyForURL(url, host)`.
fn run(script: &str, url: &str, host: &str) -> Result<String, String> {
    let err = |e: boa_engine::JsError| e.to_string();
    let mut ctx = Context::default();
    ctx.register_global_callable(
        js_string!("__mdmResolve"),
        1,
        NativeFunction::from_fn_ptr(resolve),
    )
    .map_err(err)?;
    ctx.register_global_callable(
        js_string!("__mdmMyIp"),
        0,
        NativeFunction::from_fn_ptr(my_ip),
    )
    .map_err(err)?;
    ctx.eval(Source::from_bytes(HELPERS)).map_err(err)?;
    ctx.eval(Source::from_bytes(script)).map_err(err)?;
    let find = ctx
        .global_object()
        .get(js_string!("FindProxyForURL"), &mut ctx)
        .map_err(err)?;
    let find = find
        .as_callable()
        .ok_or_else(|| "the script has no FindProxyForURL".to_owned())?;
    let answer = find
        .call(
            &JsValue::undefined(),
            &[
                JsValue::from(JsString::from(url)),
                JsValue::from(JsString::from(host)),
            ],
            &mut ctx,
        )
        .map_err(err)?;
    Ok(answer
        .to_string(&mut ctx)
        .map_err(err)?
        .to_std_string_escaped())
}

/// `__mdmResolve(host)`: the host's first IPv4 address, or "".
fn resolve(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let host = args
        .first()
        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
        .transpose()?
        .unwrap_or_default();
    let ip = (host.as_str(), 0)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.find(|s| s.is_ipv4()))
        .map(|s| s.ip().to_string())
        .unwrap_or_default();
    Ok(JsValue::from(JsString::from(ip.as_str())))
}

/// `__mdmMyIp()`: this computer's address on its network (the one a connection out would use;
/// nothing is sent), else 127.0.0.1.
fn my_ip(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let ip = UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|_| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]));
    Ok(JsValue::from(JsString::from(ip.to_string().as_str())))
}

/// The functions PAC scripts may call (as browsers provide them).
const HELPERS: &str = r#"
function isPlainHostName(host) { return String(host).indexOf('.') < 0; }
function dnsDomainIs(host, domain) {
  host = String(host).toLowerCase(); domain = String(domain).toLowerCase();
  return host.length >= domain.length && host.substring(host.length - domain.length) === domain;
}
function localHostOrDomainIs(host, hostdom) {
  host = String(host).toLowerCase(); hostdom = String(hostdom).toLowerCase();
  return host === hostdom || (host.indexOf('.') < 0 && hostdom.indexOf(host + '.') === 0);
}
function dnsResolve(host) { var a = __mdmResolve(String(host)); return a === '' ? null : a; }
function isResolvable(host) { return dnsResolve(host) !== null; }
function myIpAddress() { return __mdmMyIp(); }
function dnsDomainLevels(host) { return String(host).split('.').length - 1; }
function __mdmAddr(s) {
  var p = String(s).split('.');
  if (p.length !== 4) return null;
  var n = 0;
  for (var i = 0; i < 4; i++) {
    if (!/^\d+$/.test(p[i])) return null;
    var x = parseInt(p[i], 10);
    if (x > 255) return null;
    n = n * 256 + x;
  }
  return n;
}
function convert_addr(ipchars) { var n = __mdmAddr(ipchars); return n === null ? 0 : n; }
function isInNet(host, pattern, mask) {
  var ip = __mdmAddr(host) !== null ? host : dnsResolve(host);
  if (ip === null) return false;
  var a = __mdmAddr(ip), p = __mdmAddr(pattern), m = __mdmAddr(mask);
  if (a === null || p === null || m === null) return false;
  return ((a & m) >>> 0) === ((p & m) >>> 0);
}
function shExpMatch(str, exp) {
  var re = String(exp).replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.');
  return new RegExp('^' + re + '$').test(String(str));
}
var __mdmDays = ['SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT'];
var __mdmMonths = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];
function __mdmGmt(args) { return args.length > 0 && String(args[args.length - 1]).toUpperCase() === 'GMT'; }
function __mdmIn(v, lo, hi) { return lo <= hi ? (v >= lo && v <= hi) : (v >= lo || v <= hi); }
function weekdayRange() {
  var a = Array.prototype.slice.call(arguments), gmt = __mdmGmt(a);
  if (gmt) a.pop();
  var now = new Date(), d = gmt ? now.getUTCDay() : now.getDay();
  var d1 = __mdmDays.indexOf(String(a[0]).toUpperCase());
  var d2 = a.length > 1 ? __mdmDays.indexOf(String(a[1]).toUpperCase()) : d1;
  if (d1 < 0 || d2 < 0) return false;
  return __mdmIn(d, d1, d2);
}
function timeRange() {
  var a = Array.prototype.slice.call(arguments), gmt = __mdmGmt(a);
  if (gmt) a.pop();
  a = a.map(function (x) { return parseInt(x, 10); });
  var now = new Date();
  var h = gmt ? now.getUTCHours() : now.getHours();
  var m = gmt ? now.getUTCMinutes() : now.getMinutes();
  var s = gmt ? now.getUTCSeconds() : now.getSeconds();
  var t = h * 3600 + m * 60 + s;
  if (a.length === 1) return h === a[0];
  if (a.length === 2) return __mdmIn(h, a[0], a[1]);
  if (a.length === 4) return __mdmIn(t, a[0] * 3600 + a[1] * 60, a[2] * 3600 + a[3] * 60 + 59);
  if (a.length === 6) return __mdmIn(t, a[0] * 3600 + a[1] * 60 + a[2], a[3] * 3600 + a[4] * 60 + a[5]);
  return false;
}
function dateRange() {
  var a = Array.prototype.slice.call(arguments), gmt = __mdmGmt(a);
  if (gmt) a.pop();
  var now = new Date();
  var cur = { d: gmt ? now.getUTCDate() : now.getDate(), m: gmt ? now.getUTCMonth() : now.getMonth(),
              y: gmt ? now.getUTCFullYear() : now.getFullYear() };
  // Each argument: a month name, a year (over 31) or a day.
  var parts = a.map(function (x) {
    var i = __mdmMonths.indexOf(String(x).toUpperCase());
    if (i >= 0) return { k: 'm', v: i };
    var n = parseInt(x, 10);
    return n > 31 ? { k: 'y', v: n } : { k: 'd', v: n };
  });
  if (parts.length === 1) return cur[parts[0].k] === parts[0].v;
  // The kinds given decide what is compared; each end is today with those parts set.
  var kinds = {};
  parts.forEach(function (p) { kinds[p.k] = true; });
  var key = function (x) { return (kinds.y ? x.y : 0) * 10000 + (kinds.m ? x.m : 0) * 100 + (kinds.d ? x.d : 0); };
  var end = function (ps) {
    var x = { y: cur.y, m: cur.m, d: cur.d };
    ps.forEach(function (p) { x[p.k] = p.v; });
    return x;
  };
  var half = parts.length / 2;
  return __mdmIn(key(cur), key(end(parts.slice(0, half))), key(end(parts.slice(half))));
}
function alert() {}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn route(script: &str, url: &str) -> Vec<Route> {
        routes(&Arc::from(script), &Url::parse(url).unwrap()).unwrap()
    }

    #[test]
    fn answers_become_routes() {
        assert_eq!(
            parse("PROXY proxy.corp:8080; SOCKS s.corp:1080; DIRECT"),
            vec![
                Route::Proxy("http://proxy.corp:8080".into()),
                Route::Proxy("socks5://s.corp:1080".into()),
                Route::Direct
            ]
        );
        assert_eq!(parse(""), vec![Route::Direct]);
        assert_eq!(parse("WEIRD x; DIRECT"), vec![Route::Direct]);
    }

    #[test]
    fn a_typical_script_is_run() {
        let script = r#"
            function FindProxyForURL(url, host) {
                if (isPlainHostName(host) || dnsDomainIs(host, ".intranet.test")) return "DIRECT";
                if (shExpMatch(url, "*/downloads/*.iso")) return "PROXY iso.proxy:3128";
                if (isInNet("10.1.2.3", "10.0.0.0", "255.0.0.0") && dnsDomainLevels(host) > 1)
                    return "PROXY proxy.corp:8080; DIRECT";
                return "DIRECT";
            }"#;
        assert_eq!(route(script, "http://intranet/a"), vec![Route::Direct]);
        assert_eq!(
            route(script, "https://wiki.intranet.test/x"),
            vec![Route::Direct]
        );
        assert_eq!(
            route(script, "https://mirror.example.org/downloads/os.iso"),
            vec![Route::Proxy("http://iso.proxy:3128".into())]
        );
        assert_eq!(
            route(script, "https://cdn.example.org/a.zip")[0],
            Route::Proxy("http://proxy.corp:8080".into())
        );
    }

    #[test]
    fn helpers_behave() {
        let check = |expr: &str| {
            let script = format!(
                "function FindProxyForURL(u, h) {{ return ({expr}) ? 'DIRECT' : 'PROXY no:1'; }}"
            );
            route(&script, "http://x.test/") == vec![Route::Direct]
        };
        assert!(check("localHostOrDomainIs('www', 'www.example.org')"));
        assert!(!check(
            "localHostOrDomainIs('www.other.org', 'www.example.org')"
        ));
        assert!(check(
            "isInNet('192.168.1.20', '192.168.0.0', '255.255.0.0')"
        ));
        assert!(!check("isInNet('10.0.0.1', '192.168.0.0', '255.255.0.0')"));
        assert!(check("convert_addr('104.16.41.2') === 1745889538"));
        assert!(check(
            "shExpMatch('a.b.c', 'a.?.c') && !shExpMatch('abc', 'a?')"
        ));
        assert!(check(
            "isResolvable('localhost') && dnsResolve('localhost') === '127.0.0.1'"
        ));
        assert!(check(
            "typeof myIpAddress() === 'string' && myIpAddress().split('.').length === 4"
        ));
        assert!(check("weekdayRange('SUN', 'SAT') && timeRange(0, 23)"));
        assert!(check("dateRange('JAN', 'DEC')"));
    }

    #[test]
    fn a_broken_script_is_an_error() {
        assert!(
            routes(
                &Arc::from("this is not javascript ("),
                &Url::parse("http://x/").unwrap()
            )
            .is_err()
        );
        assert!(routes(&Arc::from("var x = 1;"), &Url::parse("http://x/").unwrap()).is_err());
    }
}
