//! Sending a `.http` file's request through `ureq`, on a thread of its own. Redirects are followed here (not by
//! `ureq`) so each hop can be shown and its cookies kept, and the cookies a file's responses set are kept in a jar the
//! caller hands in and gets back.

use crate::http_file::{CookieJar, Resolved, split_url};
use std::time::{Duration, Instant};

/// The most redirects followed.
const MOST_HOPS: usize = 10;
/// The most of a body that is read.
const MOST_BODY: u64 = 64 * 1024 * 1024;
/// How long a request may take altogether.
const TIMEOUT: Duration = Duration::from_secs(60);

/// One redirect that was followed.
#[derive(Clone, Debug, PartialEq)]
pub struct Hop {
    pub status: u16,
    pub from: String,
    pub to: String,
}

/// What came back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub reason: String,
    /// The address the last response came from, after any redirects.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub millis: u128,
    pub hops: Vec<Hop>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn seconds_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Where a `Location` header points, from the address it came with.
pub fn join_url(base: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_string();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("http", base));
    if let Some(network) = location.strip_prefix("//") {
        return format!("{scheme}://{network}");
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, path) = rest.split_at(host_end);
    if location.starts_with('/') {
        return format!("{scheme}://{host}{location}");
    }
    let path = path.split(['?', '#']).next().unwrap_or("");
    let folder = path.rfind('/').map_or("/", |at| &path[..=at]);
    format!("{scheme}://{host}{folder}{location}")
}

/// Sends a request, following redirects unless it says not to. The jar is updated with the cookies the responses set.
pub fn send(request: &Resolved, jar: &mut CookieJar) -> Result<Reply, String> {
    let config = ureq::Agent::config_builder().max_redirects(0).http_status_as_error(false).timeout_global(Some(TIMEOUT)).build();
    let agent = ureq::Agent::new_with_config(config);
    let began = Instant::now();
    let (mut method, mut url, mut body) = (request.method.clone(), request.url.clone(), request.body.clone());
    let mut headers = request.headers.clone();
    let mut hops = Vec::new();
    loop {
        let mut built = ureq::http::Request::builder().method(ureq::http::Method::from_bytes(method.as_bytes()).map_err(|_| crate::i18n::tf!("{method} isn't a method that can be sent."))?).uri(&url);
        let mut cookie_sent = false;
        for (name, value) in &headers {
            if name.eq_ignore_ascii_case("cookie") && !request.no_cookie_jar {
                // The request's own cookies, and the jar's after them.
                let more = jar.header_for(&url, seconds_now()).map(|jarred| format!("; {jarred}")).unwrap_or_default();
                built = built.header(name.as_str(), format!("{value}{more}"));
                cookie_sent = true;
            } else {
                built = built.header(name.as_str(), value.as_str());
            }
        }
        if !cookie_sent
            && !request.no_cookie_jar
            && let Some(jarred) = jar.header_for(&url, seconds_now())
        {
            built = built.header("Cookie", jarred);
        }
        if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("user-agent")) {
            built = built.header("User-Agent", "Limoniter");
        }
        let sent = built.body(body.clone()).map_err(|e| crate::i18n::tf!("The request can't be made: {e}"))?;
        let response = agent.run(sent).map_err(|e| crate::i18n::tf!("{url} couldn't be reached: {e}"))?;
        let status = response.status();
        let mut reply_headers = Vec::new();
        for (name, value) in response.headers() {
            reply_headers.push((name.as_str().to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned()));
        }
        if !request.no_cookie_jar {
            for (name, value) in &reply_headers {
                if name.eq_ignore_ascii_case("set-cookie") {
                    jar.store(&url, value, seconds_now());
                }
            }
        }
        let location = reply_headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("location")).map(|(_, v)| v.clone());
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
            && !request.no_redirect
            && let Some(location) = location
        {
            if hops.len() >= MOST_HOPS {
                return Err(crate::i18n::tf!("It was sent on more than {MOST_HOPS} times: the redirects may be going round in a circle."));
            }
            let next = join_url(&url, &location);
            hops.push(Hop { status: status.as_u16(), from: url.clone(), to: next.clone() });
            // 301, 302 and 303 turn a POST into a GET with no body, as browsers do; 307 and 308 keep it all.
            if matches!(status.as_u16(), 301..=303) && method != "GET" && method != "HEAD" {
                method = "GET".to_string();
                body.clear();
                headers.retain(|(n, _)| !n.to_ascii_lowercase().starts_with("content-"));
            }
            // A password isn't passed on to another site.
            if split_url(&next).0 != split_url(&url).0 {
                headers.retain(|(n, _)| !n.eq_ignore_ascii_case("authorization"));
            }
            url = next;
            continue;
        }
        let bytes = response.into_body().with_config().limit(MOST_BODY).read_to_vec().map_err(|e| crate::i18n::tf!("The answer couldn't be read: {e}"))?;
        return Ok(Reply { status: status.as_u16(), reason: status.canonical_reason().unwrap_or("").to_string(), url, headers: reply_headers, body: bytes, millis: began.elapsed().as_millis(), hops });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A little server on this computer: it answers each request with what `answer` makes of the request's text (the
    /// reply's full text), and keeps what it was asked.
    fn serve(answer: impl Fn(&str) -> String + Send + 'static) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut data = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buffer).unwrap_or(0);
                    data.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&data).into_owned();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end].to_ascii_lowercase().split("content-length:").nth(1).and_then(|l| l.lines().next()).and_then(|l| l.trim().parse::<usize>().ok()).unwrap_or(0);
                        if data.len() >= end + 4 + length {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&data).into_owned();
                let reply = answer(&text);
                kept.lock().unwrap().push(text);
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (address, seen)
    }

    fn ok(headers: &str, body: &str) -> String {
        format!("HTTP/1.1 200 OK\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    fn get(url: &str) -> Resolved {
        Resolved { method: "GET".into(), url: url.into(), ..Resolved::default() }
    }

    #[test]
    fn a_request_goes_with_its_headers_and_body_and_the_answer_is_read() {
        let (address, seen) = serve(|_| ok("Content-Type: application/json\r\nX-Thing: yes\r\n", "{\"ok\": true}"));
        let request = Resolved { method: "POST".into(), url: format!("{address}/things?a=1"), headers: vec![("Content-Type".into(), "text/plain".into()), ("X-Mine".into(), "1".into())], body: b"hello".to_vec(), ..Resolved::default() };
        let reply = send(&request, &mut CookieJar::default()).unwrap();
        assert_eq!((reply.status, reply.reason.as_str()), (200, "OK"));
        assert_eq!(reply.header("x-thing"), Some("yes"));
        assert_eq!(reply.body, b"{\"ok\": true}");
        assert!(reply.hops.is_empty());
        let asked = seen.lock().unwrap()[0].clone();
        assert!(asked.starts_with("POST /things?a=1 HTTP/1.1"), "{asked}");
        assert!(asked.to_ascii_lowercase().contains("x-mine: 1"));
        assert!(asked.ends_with("hello"));
        // Any method may be sent, and an error status is an answer, not a failure.
        let (address, _) = serve(|_| "HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope".to_string());
        let reply = send(&Resolved { method: "DELETE".into(), url: format!("{address}/x"), ..Resolved::default() }, &mut CookieJar::default()).unwrap();
        assert_eq!((reply.status, reply.body.as_slice()), (404, &b"nope"[..]));
    }

    #[test]
    fn redirects_are_followed_and_the_hops_shown() {
        let (address, seen) = serve(|asked| {
            if asked.starts_with("POST /start") {
                "HTTP/1.1 302 Found\r\nLocation: /middle\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else if asked.starts_with("GET /middle") {
                "HTTP/1.1 301 Moved\r\nLocation: end\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else {
                ok("", "arrived")
            }
        });
        let request = Resolved { method: "POST".into(), url: format!("{address}/start"), body: b"data".to_vec(), headers: vec![("Content-Type".into(), "text/plain".into())], ..Resolved::default() };
        let reply = send(&request, &mut CookieJar::default()).unwrap();
        assert_eq!(reply.body, b"arrived");
        assert_eq!(reply.hops, [Hop { status: 302, from: format!("{address}/start"), to: format!("{address}/middle") }, Hop { status: 301, from: format!("{address}/middle"), to: format!("{address}/end") }]);
        assert!(reply.url.ends_with("/end"));
        assert!(seen.lock().unwrap()[1].starts_with("GET /middle"), "a 302 turns the POST into a GET");
        // Not followed when it says so.
        let reply = send(&Resolved { no_redirect: true, ..request }, &mut CookieJar::default()).unwrap();
        assert_eq!((reply.status, reply.hops.len()), (302, 0));
        assert_eq!(reply.header("location"), Some("/middle"));
    }

    #[test]
    fn a_redirect_that_goes_in_a_circle_stops() {
        let (address, _) = serve(|_| "HTTP/1.1 302 Found\r\nLocation: /again\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string());
        let problem = send(&get(&format!("{address}/again")), &mut CookieJar::default()).unwrap_err();
        assert!(problem.contains("circle"), "{problem}");
    }

    #[test]
    fn cookies_set_by_a_response_are_sent_with_the_next_request() {
        let (address, seen) = serve(|asked| if asked.starts_with("GET /login") { ok("Set-Cookie: sid=abc123; Path=/\r\nSet-Cookie: lang=en; Path=/\r\n", "in") } else { ok("", "page") });
        let mut jar = CookieJar::default();
        send(&get(&format!("{address}/login")), &mut jar).unwrap();
        assert_eq!(jar.len(), 2);
        send(&get(&format!("{address}/page")), &mut jar).unwrap();
        let second = seen.lock().unwrap()[1].to_ascii_lowercase();
        assert!(second.contains("cookie: sid=abc123; lang=en"), "{second}");
        // A request that opts out neither sends nor keeps them.
        send(&Resolved { no_cookie_jar: true, ..get(&format!("{address}/page")) }, &mut jar).unwrap();
        assert!(!seen.lock().unwrap()[2].to_ascii_lowercase().contains("cookie:"));
        // And clearing them stops them going.
        jar.clear();
        send(&get(&format!("{address}/page")), &mut jar).unwrap();
        assert!(!seen.lock().unwrap()[3].to_ascii_lowercase().contains("cookie:"));
    }

    #[test]
    fn a_cookie_set_during_a_redirect_goes_on_the_next_hop() {
        let (address, seen) = serve(|asked| {
            if asked.starts_with("GET /a") {
                "HTTP/1.1 302 Found\r\nSet-Cookie: hop=1; Path=/\r\nLocation: /b\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else {
                ok("", "b")
            }
        });
        send(&get(&format!("{address}/a")), &mut CookieJar::default()).unwrap();
        assert!(seen.lock().unwrap()[1].to_ascii_lowercase().contains("cookie: hop=1"));
    }

    #[test]
    fn a_server_that_isnt_there_is_a_problem_in_words() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let problem = send(&get(&format!("http://{address}/")), &mut CookieJar::default()).unwrap_err();
        assert!(problem.contains(&address.to_string()), "{problem}");
        assert!(send(&Resolved { method: "BAD METHOD".into(), url: "http://127.0.0.1/".into(), ..Resolved::default() }, &mut CookieJar::default()).is_err());
    }

    #[test]
    fn addresses_are_joined_the_way_browsers_do() {
        assert_eq!(join_url("http://h/a/b?q=1", "c"), "http://h/a/c");
        assert_eq!(join_url("http://h/a/b", "/c"), "http://h/c");
        assert_eq!(join_url("https://h/a", "//other/x"), "https://other/x");
        assert_eq!(join_url("http://h", "x"), "http://h/x");
        assert_eq!(join_url("http://h/a", "https://z/y"), "https://z/y");
    }
}
