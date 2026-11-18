//! The hourly post on X: the latest hour's caption with its card picture, posted through API v2 as the account whose
//! keys are in the environment and signed with OAuth 1.0a, whose keys do not expire, so an unattended timer cannot lock
//! itself out. It is off until `VENIN_X_POST=on` stands beside the keys; off, a run shows what it would post. On, it
//! posts each new hour at most once and never catches up on hours it missed, so a restart cannot flood the feed: the
//! hour is claimed before the post is sent, and a post X does not confirm is not sent again. A picture X refuses (4xx)
//! leaves the post text only; one lost on the way is tried again before anything is posted.

use crate::config::Config;
use crate::net::{Net, pct};
use crate::runner::log;
use crate::store::Store;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::Sha1;
use std::path::Path;
use std::time::Duration;
use venin_core::card::{CardFile, caption_ok};
use venin_core::sha256_hex;
use venin_core::verify::Head;

/// The switch: posting is on only when this is `on`.
pub const SWITCH_ENV: &str = "VENIN_X_POST";
/// Where the keys come from: the app's consumer key and secret, and the posting account's access token and secret.
pub const CONSUMER_KEY_ENV: &str = "VENIN_X_CONSUMER_KEY";
pub const CONSUMER_SECRET_ENV: &str = "VENIN_X_CONSUMER_SECRET";
pub const TOKEN_ENV: &str = "VENIN_X_TOKEN";
pub const TOKEN_SECRET_ENV: &str = "VENIN_X_TOKEN_SECRET";

/// The last hour handled, in the runner's state folder.
const STATE: &str = "post.json";
const TIMEOUT: Duration = Duration::from_secs(60);
/// X's limit for an account without Premium.
const MAX_CHARS: usize = 280;

pub struct Keys {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub token: String,
    pub token_secret: String,
}

impl Keys {
    fn from_env() -> Result<Self, String> {
        let get = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("{name} is not set"))
        };
        Ok(Keys {
            consumer_key: get(CONSUMER_KEY_ENV)?,
            consumer_secret: get(CONSUMER_SECRET_ENV)?,
            token: get(TOKEN_ENV)?,
            token_secret: get(TOKEN_SECRET_ENV)?,
        })
    }
}

/// The last hour handled, in the lab whose manifest hashes to `lab`. `posted` is None until the post is sent (the
/// hour only shown, or its picture still on the way), empty once it is claimed, and the post's id once X answered.
#[derive(Serialize, Deserialize)]
struct Last {
    #[serde(default)]
    lab: String,
    n: u64,
    posted: Option<String>,
}

/// A request X refused or that got no answer: the HTTP status when X answered, None when the answer never came.
struct Fail {
    status: Option<u16>,
    message: String,
}

pub fn run(config: &Config, store: &Store) -> Result<(), String> {
    let post = config
        .post
        .as_ref()
        .ok_or("the configuration has no [post] section")?;
    let keys = if std::env::var(SWITCH_ENV).is_ok_and(|v| v == "on") {
        Some(Keys::from_env()?)
    } else {
        None
    };
    let net = Net::new(std::slice::from_ref(&post.api))?;
    if let Some(id) = hour(&net, &post.api, keys.as_ref(), store, &config.data_dir)? {
        log(&format!("post: posted {id}"));
    }
    Ok(())
}

/// Posts the latest hour when it is new and its card is written, and returns the post's id. Without keys (posting off)
/// it shows, once per hour, the text it would post.
fn hour(
    net: &Net,
    api: &str,
    keys: Option<&Keys>,
    store: &Store,
    data: &Path,
) -> Result<Option<String>, String> {
    let Some(head) = read::<Head>(&data.join("head.json"))? else {
        return Ok(None);
    };
    let n = head.n;
    let lab = std::fs::read(data.join("manifest.json"))
        .map_or_else(|_| String::new(), |m| sha256_hex(&m));
    // Within one lab the head only moves forward, so an hour at or below the last one handled is never posted (a
    // restored head cannot bring an hour back); a new lab starts from nothing.
    let last = store.state_json::<Last>(STATE)?.filter(|l| l.lab == lab);
    if last
        .as_ref()
        .is_some_and(|l| n < l.n || (n == l.n && (l.posted.is_some() || keys.is_none())))
    {
        return Ok(None);
    }
    let picture = data.join(format!("cards/{n}.png"));
    let Some(card) = read::<CardFile>(&data.join(format!("cards/{n}.json")))? else {
        return Ok(None);
    };
    if !picture.exists() {
        return Ok(None);
    }
    let text = text(&card)?;
    let mark = |posted: Option<String>| {
        store.write_state_json(
            STATE,
            &Last {
                lab: lab.clone(),
                n,
                posted,
            },
        )
    };
    // Written before anything is sent, so a state folder that cannot be written stops the job here.
    mark(None)?;
    let Some(keys) = keys else {
        log(&format!(
            "post: posting is off; hour {n} would post: {text:?}"
        ));
        return Ok(None);
    };
    let picture = std::fs::read(&picture).map_err(|e| format!("{}: {e}", picture.display()))?;
    let media = match upload(net, api, keys, &picture) {
        Ok(id) => Some(id),
        Err(f) if f.status.is_some_and(|s| (400..500).contains(&s)) => {
            log(&format!(
                "post: hour {n} goes out without its picture: {}",
                f.message
            ));
            None
        }
        Err(f) => return Err(f.message),
    };
    mark(Some(String::new()))?;
    let id = create(net, api, keys, &text, media.as_deref())
        .map_err(|f| format!("hour {n} stays unposted: {}", f.message))?;
    mark(Some(id.clone()))?;
    Ok(Some(id))
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The post's words as the card prints them: its caption, held to the card's own rules (no link, tag or long hex),
/// and its note line under it (a LATE hour, or more than 10% void), never hidden.
fn text(card: &CardFile) -> Result<String, String> {
    caption_ok(&card.caption)
        .map_err(|e| format!("hour {}: the caption breaks the card rules: {e}", card.n))?;
    let text = match &card.note {
        Some(note) => format!("{}\nNote: {note}", card.caption),
        None => card.caption.clone(),
    };
    if text.chars().count() > MAX_CHARS {
        return Err(format!(
            "hour {}: the post is longer than {MAX_CHARS} characters",
            card.n
        ));
    }
    Ok(text)
}

fn upload(net: &Net, api: &str, keys: &Keys, picture: &[u8]) -> Result<String, Fail> {
    let url = format!("{api}/2/media/upload");
    let body = json!({"media": STANDARD.encode(picture), "media_category": "tweet_image"});
    id_of(send(net, &url, keys, &body)?, &url)
}

fn create(
    net: &Net,
    api: &str,
    keys: &Keys,
    text: &str,
    media: Option<&str>,
) -> Result<String, Fail> {
    let url = format!("{api}/2/tweets");
    let mut body = json!({"text": text});
    if let Some(id) = media {
        body["media"] = json!({"media_ids": [id]});
    }
    id_of(send(net, &url, keys, &body)?, &url)
}

fn id_of(reply: Value, url: &str) -> Result<String, Fail> {
    reply["data"]["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Fail {
            status: Some(200),
            message: format!("{url}: no data.id in the reply"),
        })
}

/// POSTs JSON signed for the keys' account; any answer but 2xx is a failure carrying X's reason.
fn send(net: &Net, url: &str, keys: &Keys, body: &Value) -> Result<Value, Fail> {
    let nonce = venin_core::hex(&crate::random::<16>());
    let header = authorization(url, keys, &nonce, crate::now());
    let reply = net
        .post_with(
            url,
            body.to_string().into_bytes(),
            &[("authorization", header)],
            TIMEOUT,
        )
        .map_err(|e| Fail {
            status: None,
            message: format!("{url}: {e}"),
        })?;
    if !(200..300).contains(&reply.status) {
        return Err(Fail {
            status: Some(reply.status),
            message: format!(
                "{url}: HTTP {}: {}",
                reply.status,
                reply.body.chars().take(300).collect::<String>()
            ),
        });
    }
    serde_json::from_str(&reply.body).map_err(|e| Fail {
        status: Some(reply.status),
        message: format!("{url}: {e}"),
    })
}

/// The OAuth 1.0a header of a POST to `url` with a JSON body: only the oauth parameters are signed, as such a body is.
fn authorization(url: &str, keys: &Keys, nonce: &str, timestamp: i64) -> String {
    let timestamp = timestamp.to_string();
    let mut oauth = vec![
        ("oauth_consumer_key", keys.consumer_key.as_str()),
        ("oauth_nonce", nonce),
        ("oauth_signature_method", "HMAC-SHA1"),
        ("oauth_timestamp", timestamp.as_str()),
        ("oauth_token", keys.token.as_str()),
        ("oauth_version", "1.0"),
    ];
    let signature = signature(
        "POST",
        url,
        &oauth,
        &keys.consumer_secret,
        &keys.token_secret,
    );
    oauth.push(("oauth_signature", signature.as_str()));
    let fields: Vec<String> = oauth
        .iter()
        .map(|(k, v)| format!("{k}=\"{}\"", pct(v, false)))
        .collect();
    format!("OAuth {}", fields.join(", "))
}

/// The OAuth 1.0a HMAC-SHA1 signature over a request's method, URL (without its query) and every parameter to sign.
fn signature(
    method: &str,
    url: &str,
    params: &[(&str, &str)],
    consumer_secret: &str,
    token_secret: &str,
) -> String {
    let mut pairs: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (pct(k, false), pct(v, false)))
        .collect();
    pairs.sort();
    let joined = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let base = format!("{method}&{}&{}", pct(url, false), pct(&joined, false));
    let key = format!(
        "{}&{}",
        pct(consumer_secret, false),
        pct(token_secret, false)
    );
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(key.as_bytes())
        .expect("HMAC takes a key of any length");
    mac.update(base.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::tests::{Seen, serve_head};
    use std::sync::{Arc, Mutex};

    /// The worked example of X's (then Twitter's) "Creating a signature" guide.
    #[test]
    fn the_signature_matches_the_published_example() {
        let params = [
            (
                "status",
                "Hello Ladies + Gentlemen, a signed OAuth request!",
            ),
            ("include_entities", "true"),
            ("oauth_consumer_key", "xvz1evFS4wEEPTGEFPHBog"),
            ("oauth_nonce", "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg"),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", "1318622958"),
            (
                "oauth_token",
                "370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb",
            ),
            ("oauth_version", "1.0"),
        ];
        assert_eq!(
            signature(
                "POST",
                "https://api.twitter.com/1.1/statuses/update.json",
                &params,
                "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw",
                "LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE"
            ),
            "hCtSmYh+iHYCEqBWrE7C7hYmtUk="
        );
    }

    fn keys() -> Keys {
        Keys {
            consumer_key: "ck".into(),
            consumer_secret: "cs".into(),
            token: "tk".into(),
            token_secret: "ts".into(),
        }
    }

    /// X's two endpoints. Each answers with the statuses queued for it, in order, then with success: a media id, or a
    /// post's id.
    fn x(media: &[u16], posts: &[u16]) -> (String, Net, Seen) {
        let queues = Arc::new(Mutex::new((media.to_vec(), posts.to_vec())));
        let (url, seen) = serve_head(move |_, path, _, _| {
            let mut q = queues.lock().expect("lock");
            let next = |list: &mut Vec<u16>| (!list.is_empty()).then(|| list.remove(0));
            match path {
                "/2/media/upload" => match next(&mut q.0) {
                    Some(status) => (status, Vec::new(), r#"{"title":"no"}"#.into()),
                    None => (200, Vec::new(), r#"{"data":{"id":"1001"}}"#.into()),
                },
                "/2/tweets" => match next(&mut q.1) {
                    Some(status) => (status, Vec::new(), r#"{"title":"no"}"#.into()),
                    None => (
                        201,
                        Vec::new(),
                        r#"{"data":{"id":"2002","text":"t"}}"#.into(),
                    ),
                },
                _ => (404, Vec::new(), String::new()),
            }
        });
        let net = Net::new(std::slice::from_ref(&url)).expect("client");
        (url, net, seen)
    }

    fn lab(name: &str) -> (std::path::PathBuf, Store) {
        let dir = crate::anchor::tests::temp(name);
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("cards")).expect("folder");
        (data.clone(), Store::new(data, dir.join("state")))
    }

    fn hour_written(data: &Path, n: u64, card: bool) {
        let head = json!({"n": n, "sha": "0".repeat(64), "written_s": 0});
        std::fs::write(data.join("head.json"), head.to_string()).expect("write");
        if card {
            let card = json!({
                "n": n, "story": "quiet", "caption": format!("No agent fell in hour {n}."), "note": null, "focal": null
            });
            std::fs::write(data.join(format!("cards/{n}.json")), card.to_string()).expect("write");
            std::fs::write(data.join(format!("cards/{n}.png")), "png").expect("write");
        }
    }

    fn paths(seen: &Seen) -> Vec<String> {
        seen.lock()
            .expect("lock")
            .iter()
            .map(|(_, path, _, _)| path.clone())
            .collect()
    }

    #[test]
    fn each_new_hour_is_posted_once_with_its_picture_and_missed_hours_are_skipped() {
        let (data, store) = lab("post-hours");
        let (url, net, seen) = x(&[], &[]);
        hour_written(&data, 3, false);
        assert_eq!(
            hour(&net, &url, Some(&keys()), &store, &data).expect("waits"),
            None,
            "no card yet"
        );
        hour_written(&data, 3, true);
        assert_eq!(
            hour(&net, &url, Some(&keys()), &store, &data)
                .expect("posts")
                .as_deref(),
            Some("2002")
        );
        assert_eq!(
            hour(&net, &url, Some(&keys()), &store, &data).expect("once"),
            None
        );
        let log = seen.lock().expect("lock").clone();
        assert_eq!(log.len(), 2);
        let media: Value = serde_json::from_str(&log[0].2).expect("json");
        assert_eq!(
            media,
            json!({"media": STANDARD.encode("png"), "media_category": "tweet_image"})
        );
        let post: Value = serde_json::from_str(&log[1].2).expect("json");
        assert_eq!(
            post,
            json!({"text": "No agent fell in hour 3.", "media": {"media_ids": ["1001"]}})
        );
        for (_, _, _, head) in &log {
            assert!(
                head.contains("authorization: oauth oauth_consumer_key=\"ck\", oauth_nonce=\""),
                "{head}"
            );
            assert!(head.contains("oauth_signature=\""), "{head}");
        }
        hour_written(&data, 4, true);
        hour_written(&data, 6, true);
        assert!(
            hour(&net, &url, Some(&keys()), &store, &data)
                .expect("posts")
                .is_some()
        );
        let log = seen.lock().expect("lock").clone();
        assert_eq!(log.len(), 4, "hour 4 is skipped, hour 6 posted");
        assert!(log[3].2.contains("hour 6"));
        hour_written(&data, 4, true);
        assert_eq!(
            hour(&net, &url, Some(&keys()), &store, &data).expect("not back"),
            None,
            "a head moved back brings no hour back"
        );
        std::fs::write(data.join("manifest.json"), "{\"genesis\":2}").expect("write");
        hour_written(&data, 0, true);
        assert!(
            hour(&net, &url, Some(&keys()), &store, &data)
                .expect("a new lab")
                .is_some(),
            "a new lab's hour 0 is posted"
        );
    }

    #[test]
    fn off_it_only_shows_the_hour_and_on_it_posts_without_a_refused_picture() {
        let (data, store) = lab("post-off");
        let (url, net, seen) = x(&[403], &[]);
        hour_written(&data, 2, true);
        assert_eq!(hour(&net, &url, None, &store, &data).expect("shown"), None);
        assert_eq!(
            hour(&net, &url, None, &store, &data).expect("shown once"),
            None
        );
        assert!(seen.lock().expect("lock").is_empty(), "off sends nothing");
        assert_eq!(
            hour(&net, &url, Some(&keys()), &store, &data)
                .expect("posts")
                .as_deref(),
            Some("2002")
        );
        let log = seen.lock().expect("lock").clone();
        assert_eq!(log.len(), 2);
        assert_eq!(
            log[1].2,
            json!({"text": "No agent fell in hour 2."}).to_string()
        );
    }

    #[test]
    fn a_lost_picture_is_tried_again_and_a_post_x_does_not_confirm_is_never_sent_again() {
        let (data, store) = lab("post-fail");
        let (url, net, seen) = x(&[503], &[500]);
        hour_written(&data, 1, true);
        assert!(
            hour(&net, &url, Some(&keys()), &store, &data).is_err(),
            "a picture lost to a 503 is tried again"
        );
        assert_eq!(paths(&seen), ["/2/media/upload"], "no post without it");
        let e = hour(&net, &url, Some(&keys()), &store, &data).expect_err("not confirmed");
        assert!(e.contains("stays unposted"), "{e}");
        assert_eq!(
            hour(&net, &url, Some(&keys()), &store, &data).expect("not again"),
            None,
            "a 5xx may follow a post X made, so the hour is not tried again"
        );
        assert_eq!(
            paths(&seen),
            ["/2/media/upload", "/2/media/upload", "/2/tweets"]
        );
    }

    #[test]
    fn the_text_is_the_card_s_own_and_keeps_its_rules() {
        let card = |caption: &str, note: Option<&str>| CardFile {
            n: 7,
            story: venin_core::card::Story::Quiet,
            caption: caption.into(),
            note: note.map(str::to_string),
            focal: None,
        };
        assert_eq!(
            text(&card(
                "Hour 7 was quiet.",
                Some("anchored LATE; off the scoreboard.")
            ))
            .expect("text"),
            "Hour 7 was quiet.\nNote: anchored LATE; off the scoreboard."
        );
        for bad in [
            "See https://veninlab.test",
            "Ask @veninlab",
            "Hour 7 #ai",
            "lower case start",
        ] {
            assert!(text(&card(bad, None)).is_err(), "{bad}");
        }
        let long = format!("A{}", "x".repeat(238));
        assert!(text(&card(&long, None)).is_ok());
        assert!(
            text(&card(&long, Some(&"y".repeat(40)))).is_err(),
            "over 280 with its note"
        );
    }
}
