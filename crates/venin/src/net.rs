//! The one HTTP client of the binary. Before each request it checks the URL's scheme, host and port against the
//! origins the configuration names, and it never follows a redirect.

use reqwest::Url;
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde_json::{Value, json};
use std::time::Duration;

/// Public RPCs refuse scripted user agents, so every request presents a browser's.
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/142.0.0.0 Safari/537.36";
const RPC_TIMEOUT: Duration = Duration::from_secs(20);

pub type Origin = (String, String, u16);

pub fn origin(url: &str) -> Result<Origin, String> {
    let u = Url::parse(url).map_err(|e| format!("{url}: {e}"))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(format!("{url}: only http and https are allowed"));
    }
    let host = u.host_str().ok_or_else(|| format!("{url}: no host"))?;
    let port = u
        .port_or_known_default()
        .ok_or_else(|| format!("{url}: no port"))?;
    Ok((u.scheme().to_string(), host.to_ascii_lowercase(), port))
}

pub struct Reply {
    pub status: u16,
    pub body: String,
    pub retry_after_s: Option<u64>,
    /// The response headers, names in lower case.
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// RFC 3986 percent-encoding: every byte outside A-Z, a-z, 0-9 and `-._~`, and outside `/` unless `slash` is set,
/// becomes %XX.
pub fn pct(s: &str, slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) || (slash && b == b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Clone)]
pub struct Net {
    client: Client,
    allowed: Vec<Origin>,
}

impl Net {
    pub fn new(urls: &[String]) -> Result<Self, String> {
        let client = Client::builder()
            .redirect(Policy::none())
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            .no_proxy()
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let allowed = urls.iter().map(|u| origin(u)).collect::<Result<_, _>>()?;
        Ok(Net { client, allowed })
    }

    fn check(&self, url: &str) -> Result<(), String> {
        let o = origin(url)?;
        if self.allowed.contains(&o) {
            Ok(())
        } else {
            Err(format!("{url}: not an origin the configuration allows"))
        }
    }

    fn send(
        &self,
        request: reqwest::blocking::RequestBuilder,
        timeout: Duration,
    ) -> Result<Reply, String> {
        let response = request
            .timeout(timeout)
            .send()
            .map_err(|e| e.without_url().to_string())?;
        let status = response.status().as_u16();
        let retry_after_s = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok());
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
            .collect();
        let body = response.text().map_err(|e| e.without_url().to_string())?;
        Ok(Reply {
            status,
            body,
            retry_after_s,
            headers,
        })
    }

    /// A request with any method, the given headers and a raw body, such as an object store's PUT or HEAD.
    pub fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Vec<u8>,
        timeout: Duration,
    ) -> Result<Reply, String> {
        self.check(url)?;
        let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
        let mut request = self.client.request(method, url).body(body);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        self.send(request, timeout)
    }

    pub fn get(&self, url: &str, timeout: Duration) -> Result<Reply, String> {
        self.check(url)?;
        self.send(self.client.get(url), timeout)
    }

    /// A GET with `Authorization: Bearer <token>`.
    pub fn get_bearer(&self, url: &str, token: &str, timeout: Duration) -> Result<Reply, String> {
        self.check(url)?;
        self.send(self.client.get(url).bearer_auth(token), timeout)
    }

    pub fn post(&self, url: &str, body: Vec<u8>, timeout: Duration) -> Result<Reply, String> {
        self.post_with(url, body, &[], timeout)
    }

    /// A JSON POST with extra headers.
    pub fn post_with(
        &self,
        url: &str,
        body: Vec<u8>,
        headers: &[(&str, String)],
        timeout: Duration,
    ) -> Result<Reply, String> {
        self.check(url)?;
        let mut request = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        self.send(request, timeout)
    }

    /// One JSON-RPC call; an error object or a non-200 status is an error.
    pub fn rpc(&self, url: &str, method: &str, params: Value) -> Result<Value, String> {
        self.rpc_answer(url, method, params)?
            .map_err(|e| format!("{method}: {e}"))
    }

    /// One JSON-RPC call with the node's own refusal kept apart: `Ok(Err(error))` when it answered with an error
    /// object, `Err` when no answer came (the transport, a non-200 status, a body that is not JSON).
    pub fn rpc_answer(
        &self,
        url: &str,
        method: &str,
        params: Value,
    ) -> Result<Result<Value, Value>, String> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let reply = self.post(url, body.to_string().into_bytes(), RPC_TIMEOUT)?;
        if reply.status != 200 {
            return Err(format!("{method}: HTTP {}", reply.status));
        }
        let mut v: Value =
            serde_json::from_str(&reply.body).map_err(|e| format!("{method}: {e}"))?;
        if let Some(e) = v.get_mut("error") {
            return Ok(Err(e.take()));
        }
        Ok(Ok(v
            .get_mut("result")
            .map(Value::take)
            .unwrap_or(Value::Null)))
    }
}

/// A `0x` quantity as a number.
pub fn quantity(v: &Value) -> Result<u128, String> {
    let s = v.as_str().ok_or_else(|| format!("{v} is not a quantity"))?;
    let digits = s
        .strip_prefix("0x")
        .ok_or_else(|| format!("{s} is not a quantity"))?;
    u128::from_str_radix(digits, 16).map_err(|e| format!("{s}: {e}"))
}

pub fn quantity_u64(v: &Value) -> Result<u64, String> {
    u64::try_from(quantity(v)?).map_err(|e| e.to_string())
}

pub fn hex_quantity(n: u128) -> String {
    format!("0x{n:x}")
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A request the mock server saw: method, path, body, and its header lines in lower case.
    pub type Seen = Arc<Mutex<Vec<(String, String, String, String)>>>;

    /// A one-thread HTTP server on a free local port. `answer` gets method, path and body and returns status,
    /// extra headers and body. It serves until the test process ends.
    pub fn serve<F>(answer: F) -> (String, Seen)
    where
        F: Fn(&str, &str, &str) -> (u16, Vec<(String, String)>, String) + Send + 'static,
    {
        serve_head(move |method, path, _, body| answer(method, path, body))
    }

    /// `serve`, with the request's header lines (in lower case) passed to `answer` before the body.
    pub fn serve_head<F>(answer: F) -> (String, Seen)
    where
        F: Fn(&str, &str, &str, &str) -> (u16, Vec<(String, String)>, String) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let seen: Seen = Arc::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().expect("clones"));
                let mut first = String::new();
                if reader.read_line(&mut first).is_err() {
                    continue;
                }
                let mut length = 0;
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    head.push_str(&lower);
                }
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    continue;
                }
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let body = String::from_utf8_lossy(&body).to_string();
                let (status, headers, reply) = answer(&method, &path, &head, &body);
                log.lock()
                    .expect("lock")
                    .push((method, path, body.clone(), head));
                let mut out = format!(
                    "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n",
                    reply.len()
                );
                for (k, v) in headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                out.push_str(&reply);
                let _ = stream.write_all(out.as_bytes());
            }
        });
        (url, seen)
    }

    #[test]
    fn origins_outside_the_config_are_refused() {
        let (url, seen) = serve(|_, _, _| (200, Vec::new(), "ok".into()));
        let net = Net::new(&[format!("{url}/v1")]).expect("client");
        assert_eq!(
            net.get(&format!("{url}/other"), RPC_TIMEOUT)
                .expect("same origin")
                .body,
            "ok"
        );
        assert!(net.get("http://127.0.0.1:1/", RPC_TIMEOUT).is_err());
        assert!(
            net.get(&url.replace("127.0.0.1", "localhost"), RPC_TIMEOUT)
                .is_err()
        );
        assert!(net.get("file:///etc/passwd", RPC_TIMEOUT).is_err());
        assert_eq!(seen.lock().expect("lock").len(), 1);
    }

    #[test]
    fn redirects_are_not_followed() {
        let (url, _) = serve(|_, _, _| {
            (
                302,
                vec![("location".into(), "http://127.0.0.1:1/".into())],
                String::new(),
            )
        });
        let net = Net::new(std::slice::from_ref(&url)).expect("client");
        assert_eq!(net.get(&url, RPC_TIMEOUT).expect("replies").status, 302);
    }

    #[test]
    fn rpc_returns_the_result_and_surfaces_errors() {
        let (url, seen) = serve(|_, _, body| {
            let v: Value = serde_json::from_str(body).expect("json");
            let reply = if v["method"] == "ok" {
                r#"{"jsonrpc":"2.0","id":1,"result":"0x10"}"#
            } else {
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"no"}}"#
            };
            (200, Vec::new(), reply.into())
        });
        let net = Net::new(std::slice::from_ref(&url)).expect("client");
        let r = net.rpc(&url, "ok", json!([])).expect("result");
        assert_eq!(quantity_u64(&r).expect("number"), 16);
        assert!(net.rpc(&url, "bad", json!([])).is_err());
        let (_, _, body, _) = seen.lock().expect("lock")[0].clone();
        assert!(body.contains("\"jsonrpc\":\"2.0\""));
        assert!(quantity(&json!("16")).is_err());
        assert_eq!(hex_quantity(255), "0xff");
    }
}
