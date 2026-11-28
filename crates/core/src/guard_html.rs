//! The guard's S03 and S04 over HTML. Tags, attributes, comments and raw text are read as a browser's tokenizer reads
//! them, and inline CSS is judged against a list of properties known to leave text visible with the values given.
//! S03 drops the elements hidden by the techniques listed in `strip_hidden`; it is not a browser, and other
//! combinations of visible styles can still hide text. What it cannot judge it drops.

use crate::skin::strip_comments;
use crate::strain::is_hidden;

/// One piece of a text read as HTML, as a byte range.
enum Piece {
    Text(usize, usize),
    /// The text of a raw-text element (`script`, `style`, `textarea`, ...), never read for tags.
    Raw(usize, usize),
    Start {
        name: String,
        attrs: Vec<(String, String)>,
        at: usize,
        end: usize,
    },
    End {
        name: String,
        at: usize,
        end: usize,
    },
    Comment(usize, usize),
    /// `<![CDATA[...]]>`, `<!...>`, `<?...>` and `</` with no letter: markup a page never shows. Only its end is kept.
    Bogus(usize),
}

/// Elements with no content.
const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose content is read as text up to their end tag.
const RAW: [&str; 10] = [
    "script",
    "style",
    "textarea",
    "title",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "noscript",
    "plaintext",
];

/// Elements a page does not show, whatever their style: scripts and templates, fallback content of media and
/// canvases, a title outside the head, the parts of `datalist`, `ruby` and SVG that never render.
const UNSHOWN: [&str; 16] = [
    "script", "template", "noscript", "noembed", "noframes", "iframe", "object", "title",
    "datalist", "rp", "video", "audio", "canvas", "desc", "metadata", "head",
];

/// What the guard's S03 leaves of a text whose style sheet hides anything: a sheet can hide any element, before it or
/// after it, so none of the text can be judged.
pub const WITHHELD_STYLED: &str = "[text withheld by policy: a style sheet in it hides part of it]";

fn starts_ci(b: &[u8], at: usize, s: &str) -> bool {
    b.len() >= at + s.len() && b[at..at + s.len()].eq_ignore_ascii_case(s.as_bytes())
}

fn find_from(t: &str, from: usize, s: &str) -> Option<usize> {
    t.get(from..)?.find(s).map(|i| from + i)
}

/// The tag whose name starts at `from` (just after `<` or `</`): its name, attributes and where it ends. A tag left
/// open runs to the end of the text.
fn tag(t: &str, from: usize) -> (String, Vec<(String, String)>, usize) {
    let b = t.as_bytes();
    let n = b.len();
    let mut p = from;
    while p < n && !b[p].is_ascii_whitespace() && b[p] != b'/' && b[p] != b'>' {
        p += 1;
    }
    let name = t[from..p].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        while p < n && (b[p].is_ascii_whitespace() || b[p] == b'/') {
            p += 1;
        }
        if p >= n {
            return (name, attrs, n);
        }
        if b[p] == b'>' {
            return (name, attrs, p + 1);
        }
        let key_at = p;
        p += 1;
        while p < n && !b[p].is_ascii_whitespace() && !matches!(b[p], b'/' | b'>' | b'=') {
            p += 1;
        }
        let key = t[key_at..p].to_ascii_lowercase();
        let mut q = p;
        while q < n && b[q].is_ascii_whitespace() {
            q += 1;
        }
        let mut value = String::new();
        if q < n && b[q] == b'=' {
            q += 1;
            while q < n && b[q].is_ascii_whitespace() {
                q += 1;
            }
            if q < n && (b[q] == b'"' || b[q] == b'\'') {
                let close = find_from(t, q + 1, if b[q] == b'"' { "\"" } else { "'" }).unwrap_or(n);
                value = t[q + 1..close].to_string();
                q = (close + 1).min(n);
            } else {
                let start = q;
                while q < n && !b[q].is_ascii_whitespace() && b[q] != b'>' {
                    q += 1;
                }
                value = t[start..q].to_string();
            }
            p = q;
        }
        attrs.push((key, value));
    }
}

/// `t` read as HTML.
fn pieces(t: &str) -> Vec<Piece> {
    let b = t.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0;
    let mut text_from = 0;
    while i < n {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let letter = |k: usize| b.get(k).is_some_and(u8::is_ascii_alphabetic);
        let piece = if starts_ci(b, i, "<!--") {
            let body = i + 4;
            let end = if starts_ci(b, body, ">") {
                body + 1
            } else if starts_ci(b, body, "->") {
                body + 2
            } else {
                comment_end(t, body)
            };
            Some(Piece::Comment(i, end))
        } else if starts_ci(b, i, "<![cdata[") {
            Some(Piece::Bogus(
                find_from(t, i + 9, "]]>").map_or(n, |p| p + 3),
            ))
        } else if starts_ci(b, i, "<!")
            || starts_ci(b, i, "<?")
            || (starts_ci(b, i, "</") && !letter(i + 2) && i + 2 < n)
        {
            Some(Piece::Bogus(find_from(t, i + 2, ">").map_or(n, |p| p + 1)))
        } else if starts_ci(b, i, "</") && letter(i + 2) {
            let (name, _, end) = tag(t, i + 2);
            Some(Piece::End { name, at: i, end })
        } else if letter(i + 1) {
            let (name, attrs, end) = tag(t, i + 1);
            Some(Piece::Start {
                name,
                attrs,
                at: i,
                end,
            })
        } else {
            None
        };
        let Some(piece) = piece else {
            i += 1;
            continue;
        };
        if text_from < i {
            out.push(Piece::Text(text_from, i));
        }
        let (end, raw) = match &piece {
            Piece::Start { name, end, .. } if RAW.contains(&name.as_str()) => {
                (*end, Some(name.clone()))
            }
            Piece::Start { end, .. } | Piece::End { end, .. } => (*end, None),
            Piece::Comment(_, end)
            | Piece::Bogus(end)
            | Piece::Text(_, end)
            | Piece::Raw(_, end) => (*end, None),
        };
        out.push(piece);
        i = end;
        if let Some(name) = raw {
            let close = format!("</{name}");
            let mut k = i;
            let stop = if name == "script" {
                script_end(t, i)
            } else {
                loop {
                    match find_ci(t, k, &close) {
                        Some(p)
                            if b.get(p + close.len()).is_none_or(|c| {
                                c.is_ascii_whitespace() || *c == b'/' || *c == b'>'
                            }) =>
                        {
                            break p;
                        }
                        Some(p) => k = p + 1,
                        None => break n,
                    }
                }
            };
            let stop = if name == "plaintext" { n } else { stop };
            if i < stop {
                out.push(Piece::Raw(i, stop));
            }
            i = stop;
        }
        text_from = i;
    }
    if text_from < n {
        out.push(Piece::Text(text_from, n));
    }
    out
}

/// Where a comment whose text starts at `from` ends: just past its `-->` or `--!>`, or at the end of the text. Each
/// `--` is looked at once, so a text of many comments is read in one pass.
fn comment_end(t: &str, from: usize) -> usize {
    let b = t.as_bytes();
    let mut k = from;
    while let Some(p) = find_from(t, k, "--") {
        match (b.get(p + 2), b.get(p + 3)) {
            (Some(b'>'), _) => return p + 3,
            (Some(b'!'), Some(b'>')) => return p + 4,
            _ => k = p + 1,
        }
    }
    b.len()
}

/// Where the text of a script that starts at `from` ends: at the `</script` that closes it. Inside `<!--`, a
/// `<script` opens a nested script whose `</script` does not close the outer one, as a browser's tokenizer reads it.
fn script_end(t: &str, from: usize) -> usize {
    let b = t.as_bytes();
    let bounded = |p: usize, len: usize| {
        b.get(p + len)
            .is_none_or(|c| c.is_ascii_whitespace() || *c == b'/' || *c == b'>')
    };
    let (mut escaped, mut nested) = (false, false);
    let mut k = from;
    while k < b.len() {
        if !escaped && starts_ci(b, k, "<!--") {
            escaped = true;
            k += 4;
        } else if escaped && starts_ci(b, k, "-->") {
            (escaped, nested) = (false, false);
            k += 3;
        } else if starts_ci(b, k, "</script") && bounded(k, 8) {
            if !nested {
                return k;
            }
            nested = false;
            k += 8;
        } else if escaped && !nested && starts_ci(b, k, "<script") && bounded(k, 7) {
            nested = true;
            k += 7;
        } else {
            k += 1;
        }
    }
    b.len()
}

fn find_ci(t: &str, from: usize, s: &str) -> Option<usize> {
    let b = t.as_bytes();
    (from..b.len()).find(|&k| starts_ci(b, k, s))
}

/// How the text of an element looks, as far as its own markup says; what it leaves unset follows its parent.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    opacity: f64,
    size: Option<Size>,
    /// Luminance, 0 (black) to 1 (white), of the text and of the background.
    color: Option<f64>,
    background: Option<f64>,
}

/// A font size: in pixels, as a factor of the parent's, or as a factor of the page's 16 pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Size {
    Px(f64),
    Scale(f64),
    Root(f64),
}

const PLAIN: Look = Look {
    opacity: 1.0,
    size: None,
    color: None,
    background: None,
};

/// The text of an element, as its ancestors leave it: how opaque, how large in pixels, what luminance on what.
#[derive(Clone, Copy, Debug)]
struct Shown {
    opacity: f64,
    px: f64,
    color: f64,
    background: f64,
}

impl Shown {
    /// `self` inside an element that looks like `look`, or none when its text could not be seen: less than half
    /// opaque, under 6 pixels, or within a quarter of the background's luminance.
    fn inside(self, look: Look) -> Option<Shown> {
        let px = match look.size {
            Some(Size::Px(n)) => n,
            Some(Size::Scale(f)) => self.px * f,
            Some(Size::Root(f)) => 16.0 * f,
            None => self.px,
        };
        let next = Shown {
            opacity: self.opacity * look.opacity,
            px,
            color: look.color.unwrap_or(self.color),
            background: look.background.unwrap_or(self.background),
        };
        (next.opacity >= 0.5 && next.px >= 6.0 && (next.color - next.background).abs() >= 0.25)
            .then_some(next)
    }
}

/// How a start tag shows the text it holds, or none when it hides it whatever its parents.
fn shows(name: &str, attrs: &[(String, String)]) -> Option<Look> {
    let attr = |k: &str| attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    let zero = |k: &str| attr(k).is_some_and(|v| length(v).is_some_and(|(n, _)| n <= 0.0));
    if UNSHOWN.contains(&name)
        || (matches!(name, "details" | "dialog") && attr("open").is_none())
        || (name == "svg" && (zero("width") || zero("height")))
        || attr("hidden").is_some()
    {
        return None;
    }
    let mut look = PLAIN;
    for (a, v) in attrs {
        let shade = || decoded(v).as_deref().and_then(color).map(luminance);
        match a.as_str() {
            "style" => {
                let own = read_style(v)?;
                look = Look {
                    opacity: look.opacity * own.opacity,
                    size: own.size.or(look.size),
                    color: own.color.or(look.color),
                    background: own.background.or(look.background),
                };
            }
            "color" if name == "font" => look.color = Some(shade()?),
            "text" if name == "body" => look.color = Some(shade()?),
            "bgcolor" => look.background = Some(shade()?),
            _ => {}
        }
    }
    Some(look)
}

/// The most elements the guard follows open at once; one deeper is dropped.
const DEPTH: usize = 512;

/// S03 as the guard runs it. Hidden characters go first, then the elements hidden by these techniques, each with all
/// it holds:
///
/// - elements that never render: scripts and templates, `noscript`, fallback content of `video`, `audio`, `canvas`
///   and `object`, a `title` outside the head, `datalist`, `rp`, SVG `desc` and `metadata`, a closed `details` or
///   `dialog`, an SVG of zero width or height, and anything with the `hidden` attribute;
/// - inline style the guard cannot judge visible (`visible_style`), a `font` or `body` color it cannot read, and
///   text that its ancestors make less than half opaque, smaller than 6 pixels, or close in luminance to the nearest
///   background set by a style or a `bgcolor`;
/// - markup a page never shows: CDATA, `<!...>`, `<?...>`; and style sheets.
///
/// An element left open hides the rest of the text. A style sheet that hides anything, or one linked from elsewhere,
/// withholds the whole text. It is not a browser: other combinations of visible styles can still hide text.
pub fn strip_hidden(text: &str) -> String {
    let text: String = text.chars().filter(|c| !is_hidden(*c)).collect();
    let ps = pieces(&text);
    let styled = ps.iter().enumerate().any(|(k, p)| match p {
        Piece::Start { name, .. } if name == "style" => match ps.get(k + 1) {
            Some(Piece::Raw(a, b)) => sheet_hides(&text[*a..*b]),
            _ => false,
        },
        Piece::Start { name, attrs, .. } if name == "link" => attrs
            .iter()
            .any(|(a, v)| a == "rel" && v.to_ascii_lowercase().contains("stylesheet")),
        _ => false,
    });
    if styled {
        return WITHHELD_STYLED.to_string();
    }
    let page = Shown {
        opacity: 1.0,
        px: 16.0,
        color: 0.0,
        background: 1.0,
    };
    let mut open: Vec<(String, Shown)> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut k = 0;
    while k < ps.len() {
        match &ps[k] {
            Piece::Text(a, b) | Piece::Raw(a, b) | Piece::Comment(a, b) => {
                out.push_str(&text[*a..*b])
            }
            Piece::End { name, at, end } => {
                if let Some(i) = open.iter().rposition(|(n, _)| n == name) {
                    open.truncate(i);
                }
                out.push_str(&text[*at..*end]);
            }
            Piece::Bogus(..) => {}
            Piece::Start {
                name,
                attrs,
                at,
                end,
            } => {
                let parent = open.last().map_or(page, |(_, s)| *s);
                let shown = shows(name, attrs).and_then(|look| parent.inside(look));
                match shown {
                    Some(_) if VOID.contains(&name.as_str()) => out.push_str(&text[*at..*end]),
                    None if VOID.contains(&name.as_str()) => {}
                    Some(s) if name != "style" && open.len() < DEPTH => {
                        open.push((name.clone(), s));
                        out.push_str(&text[*at..*end]);
                    }
                    _ => k = matching_end(&ps, k, name),
                }
            }
        }
        k += 1;
    }
    out
}

/// The index of the end tag that closes the element opened at `k`, counting the elements of the same name it nests;
/// past the last piece when it never closes.
fn matching_end(ps: &[Piece], k: usize, name: &str) -> usize {
    let mut depth = 0;
    for (j, p) in ps.iter().enumerate().skip(k) {
        match p {
            Piece::Start { name: n, .. } if n == name => depth += 1,
            Piece::End { name: n, .. } if n == name => {
                depth -= 1;
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
    }
    ps.len()
}

/// S04 as the guard runs it: the ward's comment filter, then the other markup a page never shows (CDATA, `<!...>`,
/// `<?...>`), which the ward's filter leaves.
pub fn strip_comments_strict(text: &str) -> String {
    let text = strip_comments(text);
    let mut out = String::with_capacity(text.len());
    for p in pieces(&text) {
        match p {
            Piece::Bogus(..) => {}
            Piece::Text(a, b) | Piece::Raw(a, b) | Piece::Comment(a, b) => {
                out.push_str(&text[a..b])
            }
            Piece::Start { at, end, .. } | Piece::End { at, end, .. } => {
                out.push_str(&text[at..end])
            }
        }
    }
    out
}

/// `value` with its character references decoded, or none when it holds one the guard does not know.
fn decoded(value: &str) -> Option<String> {
    const NAMED: [(&str, char); 18] = [
        ("amp", '&'),
        ("lt", '<'),
        ("gt", '>'),
        ("quot", '"'),
        ("apos", '\''),
        ("colon", ':'),
        ("semi", ';'),
        ("nbsp", ' '),
        ("tab", '\t'),
        ("newline", '\n'),
        ("lpar", '('),
        ("rpar", ')'),
        ("comma", ','),
        ("period", '.'),
        ("sol", '/'),
        ("bsol", '\\'),
        ("num", '#'),
        ("percnt", '%'),
    ];
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .unwrap_or(tail.len());
        let body = &tail[..end];
        let skip = end + usize::from(tail[end..].starts_with(';'));
        let c = if let Some(num) = body.strip_prefix('#') {
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => num.parse().ok(),
            };
            code.and_then(char::from_u32)?
        } else if body.is_empty() {
            out.push('&');
            rest = tail;
            continue;
        } else {
            NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(body))?.1
        };
        out.push(c);
        rest = &tail[skip..];
    }
    out.push_str(rest);
    Some(out)
}

/// A CSS length: its number and unit (empty for a bare number).
fn length(value: &str) -> Option<(f64, &str)> {
    let v = value.trim();
    let split = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(v.len());
    let number: f64 = v[..split].parse().ok()?;
    Some((number, &v[split..]))
}

/// A CSS color as red, green, blue (0 to 255) and alpha (0 to 1).
fn color(value: &str) -> Option<(f64, f64, f64, f64)> {
    const NAMED: [(&str, (f64, f64, f64)); 24] = [
        ("black", (0.0, 0.0, 0.0)),
        ("white", (255.0, 255.0, 255.0)),
        ("red", (255.0, 0.0, 0.0)),
        ("green", (0.0, 128.0, 0.0)),
        ("blue", (0.0, 0.0, 255.0)),
        ("navy", (0.0, 0.0, 128.0)),
        ("maroon", (128.0, 0.0, 0.0)),
        ("purple", (128.0, 0.0, 128.0)),
        ("teal", (0.0, 128.0, 128.0)),
        ("olive", (128.0, 128.0, 0.0)),
        ("gray", (128.0, 128.0, 128.0)),
        ("grey", (128.0, 128.0, 128.0)),
        ("silver", (192.0, 192.0, 192.0)),
        ("orange", (255.0, 165.0, 0.0)),
        ("brown", (165.0, 42.0, 42.0)),
        ("darkred", (139.0, 0.0, 0.0)),
        ("darkblue", (0.0, 0.0, 139.0)),
        ("darkgreen", (0.0, 100.0, 0.0)),
        ("darkgray", (169.0, 169.0, 169.0)),
        ("darkgrey", (169.0, 169.0, 169.0)),
        ("dimgray", (105.0, 105.0, 105.0)),
        ("dimgrey", (105.0, 105.0, 105.0)),
        ("crimson", (220.0, 20.0, 60.0)),
        ("indigo", (75.0, 0.0, 130.0)),
    ];
    let v = value.trim();
    if let Some(&(_, (r, g, b))) = NAMED.iter().find(|(n, _)| *n == v) {
        return Some((r, g, b, 1.0));
    }
    if let Some(hex) = v.strip_prefix('#') {
        let digit = |i: usize, w: usize| {
            let s = hex.get(i * w..i * w + w)?;
            let n = f64::from(u8::from_str_radix(&s.repeat(3 - w), 16).ok()?);
            Some(n)
        };
        let w = if hex.len() <= 4 { 1 } else { 2 };
        if ![3, 4, 6, 8].contains(&hex.len()) {
            return None;
        }
        let alpha = if hex.len() == 4 || hex.len() == 8 {
            digit(3, w)? / 255.0
        } else {
            1.0
        };
        return Some((digit(0, w)?, digit(1, w)?, digit(2, w)?, alpha));
    }
    let inner = v
        .strip_prefix("rgba(")
        .or_else(|| v.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() != 3 && parts.len() != 4 {
        return None;
    }
    let channel = |p: &str| match p.strip_suffix('%') {
        Some(pct) => pct.parse::<f64>().ok().map(|x| x * 2.55),
        None => p.parse::<f64>().ok(),
    };
    let alpha = match parts.get(3) {
        Some(a) => match a.strip_suffix('%') {
            Some(pct) => pct.parse::<f64>().ok()? / 100.0,
            None => a.parse::<f64>().ok()?,
        },
        None => 1.0,
    };
    Some((
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        alpha,
    ))
}

/// How light a color is, from 0 (black) to 1 (white).
fn luminance((r, g, b, _): (f64, f64, f64, f64)) -> f64 {
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0
}

/// Text in this color shows on a white page: dark enough and opaque enough. `inherit` and `currentcolor` follow the
/// parent, which is judged on its own.
fn visible_color(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    if matches!(v.as_str(), "inherit" | "initial" | "currentcolor" | "unset") {
        return true;
    }
    color(&v).is_some_and(|(r, g, b, a)| {
        (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0 <= 0.8 && a >= 0.5
    })
}

/// A background text in a visible color shows on: none, or a light color.
fn visible_background(value: &str) -> bool {
    let v = value.trim();
    matches!(v, "none" | "transparent" | "inherit" | "initial" | "unset")
        || color(v)
            .is_some_and(|(r, g, b, _)| (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0 >= 0.6)
}

/// A font size as `Size`, for a value that names one.
fn font_size(value: &str) -> Option<Size> {
    let word = match value {
        "xx-small" => Some(Size::Px(9.0)),
        "x-small" => Some(Size::Px(10.0)),
        "small" => Some(Size::Px(13.0)),
        "medium" | "initial" => Some(Size::Px(16.0)),
        "large" => Some(Size::Px(18.0)),
        "x-large" => Some(Size::Px(24.0)),
        "xx-large" => Some(Size::Px(32.0)),
        "xxx-large" => Some(Size::Px(48.0)),
        "smaller" => Some(Size::Scale(0.83)),
        "larger" => Some(Size::Scale(1.2)),
        "inherit" | "unset" => Some(Size::Scale(1.0)),
        _ => None,
    };
    word.or_else(|| {
        let (n, unit) = length(value)?;
        match unit {
            "px" => Some(Size::Px(n)),
            "pt" => Some(Size::Px(n * 4.0 / 3.0)),
            "em" => Some(Size::Scale(n)),
            "ex" | "ch" => Some(Size::Scale(n * 0.5)),
            "%" => Some(Size::Scale(n / 100.0)),
            "rem" => Some(Size::Root(n)),
            _ => None,
        }
    })
}

/// A font size readable on a page by itself: 6 pixels, half the parent's, or more.
fn visible_size(value: &str) -> bool {
    match font_size(value) {
        Some(Size::Px(n)) => n >= 6.0,
        Some(Size::Scale(f) | Size::Root(f)) => f >= 0.5,
        None => false,
    }
}

/// Does `value` move text more than 1000 pixels (or 60 em) away, in any of its lengths?
fn far(value: &str) -> bool {
    value.split_whitespace().any(|t| {
        length(t).is_some_and(|(n, unit)| match unit {
            "em" | "rem" | "ex" | "ch" => n.abs() > 60.0,
            "%" | "vw" | "vh" => n.abs() > 100.0,
            _ => n.abs() > 1000.0,
        })
    })
}

/// Properties that cannot hide text, whatever their value.
const FREE: [&str; 33] = [
    "font-weight",
    "font-style",
    "font-family",
    "font-variant",
    "font-stretch",
    "text-decoration",
    "text-decoration-line",
    "text-decoration-color",
    "text-decoration-style",
    "text-decoration-thickness",
    "text-underline-offset",
    "text-align",
    "text-transform",
    "vertical-align",
    "white-space",
    "word-break",
    "overflow-wrap",
    "word-wrap",
    "hyphens",
    "float",
    "clear",
    "cursor",
    "border-collapse",
    "border-spacing",
    "table-layout",
    "caption-side",
    "empty-cells",
    "box-sizing",
    "box-shadow",
    "text-shadow",
    "line-height",
    "mso-line-height-rule",
    "mso-padding-alt",
];

/// One declaration that leaves text visible. Sizes are judged with `overflow` in `visible_style`.
fn visible_declaration(prop: &str, value: &str) -> bool {
    const DISPLAY: [&str; 18] = [
        "block",
        "inline",
        "inline-block",
        "flex",
        "inline-flex",
        "grid",
        "inline-grid",
        "flow-root",
        "contents",
        "list-item",
        "table",
        "inline-table",
        "table-row",
        "table-cell",
        "table-row-group",
        "table-header-group",
        "table-footer-group",
        "table-caption",
    ];
    let small_offset =
        |v: &str| v == "auto" || length(v).is_some_and(|(n, _)| (0.0..=200.0).contains(&n));
    match prop {
        p if FREE.contains(&p) => true,
        p if p.starts_with("border") || p.starts_with("padding") => !far(value),
        p if p.starts_with("list-style") || p.starts_with("outline") => true,
        p if p.starts_with("margin") => !value.contains('-') && !far(value),
        "width" | "height" | "min-width" | "min-height" | "max-width" | "max-height" => true,
        "overflow" | "overflow-x" | "overflow-y" => true,
        "display" => DISPLAY.contains(&value),
        "visibility" => value == "visible",
        "opacity" => match value.strip_suffix('%') {
            Some(pct) => pct.parse::<f64>().is_ok_and(|n| n >= 50.0),
            None => value.parse::<f64>().is_ok_and(|n| n >= 0.5),
        },
        "font-size" => visible_size(value),
        "font" => value
            .split_whitespace()
            .map(|t| t.split('/').next().unwrap_or(t))
            .filter(|t| length(t).is_some())
            .all(visible_size),
        "color" => visible_color(value),
        "background" | "background-color" => visible_background(value),
        "position" => matches!(value, "static" | "relative" | "sticky"),
        "left" | "top" | "right" | "bottom" | "text-indent" => small_offset(value),
        "letter-spacing" | "word-spacing" => {
            value == "normal" || (!value.contains('-') && !far(value))
        }
        _ => false,
    }
}

/// Does an inline style leave text visible? Each declaration must pass `visible_declaration`; a style the guard cannot
/// read (a CSS comment or escape, a character reference it does not know, a function such as `var()` or `calc()`)
/// hides, and so does hidden or clipped overflow on a box that is two pixels or less across, that shifts its text
/// (an indent, padding, a margin, letter spacing) or that sets a line height of 0.
pub fn visible_style(style: &str) -> bool {
    read_style(style).is_some()
}

/// An inline style as a `Look`, or none when it does not leave text visible (`visible_style`).
fn read_style(style: &str) -> Option<Look> {
    let style = decoded(style)?.to_ascii_lowercase();
    if style.contains('\\') || style.contains("/*") || style.contains('(') && !style.contains("rgb")
    {
        return None;
    }
    let mut look = PLAIN;
    let (mut clipped, mut tiny, mut shifted, mut flat) = (false, false, false, false);
    for declaration in style.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let (prop, value) = declaration.split_once(':')?;
        let prop = prop.trim();
        let value = value.trim().trim_end_matches("!important").trim();
        if !visible_declaration(prop, value) {
            return None;
        }
        clipped |= prop.starts_with("overflow") && matches!(value, "hidden" | "clip");
        tiny |= matches!(prop, "width" | "height" | "max-width" | "max-height")
            && length(value).is_some_and(|(n, _)| n <= 2.0);
        shifted |= prop == "text-indent"
            || prop == "letter-spacing"
            || prop.starts_with("padding")
            || prop.starts_with("margin");
        flat |= (prop == "line-height" && length(value).is_some_and(|(n, _)| n == 0.0))
            || (prop == "font"
                && value
                    .split_whitespace()
                    .filter_map(|t| t.split_once('/'))
                    .any(|(_, h)| length(h).is_some_and(|(n, _)| n == 0.0)));
        match prop {
            "opacity" => {
                look.opacity = match value.strip_suffix('%') {
                    Some(pct) => pct.parse::<f64>().ok()? / 100.0,
                    None => value.parse().ok()?,
                }
            }
            "font-size" => look.size = font_size(value),
            "font" => {
                if let Some(size) = value
                    .split_whitespace()
                    .map(|t| t.split('/').next().unwrap_or(t))
                    .find_map(|t| length(t).and(font_size(t)))
                {
                    look.size = Some(size);
                }
            }
            "color" => look.color = color(value).map(luminance).or(look.color),
            "background" | "background-color" => {
                look.background = color(value).map(luminance).or(look.background);
            }
            _ => {}
        }
    }
    (!(clipped && (tiny || shifted || flat))).then_some(look)
}

/// Does a style sheet hide anything? Every declaration in its rules is judged as an inline style is; an `@import`, a
/// comment or an escape hides, since the guard cannot read what they bring.
fn sheet_hides(sheet: &str) -> bool {
    let lower = sheet.to_ascii_lowercase();
    lower.contains("@import")
        || lower.contains('\\')
        || lower.contains("/*")
        || lower
            .split('{')
            .skip(1)
            .filter_map(|segment| segment.split_once('}').map(|(body, _)| body))
            .any(|body| !body.trim().is_empty() && !visible_style(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_are_read_declaration_by_declaration() {
        for style in [
            "font-weight:bold; color: #333",
            "COLOR:Navy;font-size:14px",
            "margin:0 auto;padding:4px",
            "font: 12px/1.5 Arial",
            "color:rgb(20, 20, 20)",
            "overflow:hidden;height:40px",
        ] {
            assert!(visible_style(style), "{style}");
        }
        for style in [
            "display:/**/none",
            "display:\\6e one",
            "display&#58;none",
            "opacity:.0",
            "color:rgba(255,255,255,1)",
            "color:#fefefe",
            "position:absolute;left:-9999px",
            "font-size:.1px",
            "transform:scale(0)",
            "visibility:collapse",
            "display : none",
            "font-size:0",
            "color:var(--x)",
            "height:0;overflow:hidden",
            "mso-hide:all",
            "color:transparent",
            "colour:black",
            "font: 0 Arial",
            "display",
            "color:&bogus;",
        ] {
            assert!(!visible_style(style), "{style}");
        }
        assert_eq!(
            decoded("a&#58;b&#x3A;c&colon;d&amp;"),
            Some("a:b:c:d&".to_string())
        );
        assert_eq!(decoded("&nope;"), None);
    }

    #[test]
    fn html_is_read_as_a_browser_reads_it() {
        let p = "PAYNOW";
        for text in [
            format!("a <span title=\">\" style=\"display:none\">{p}</span> b"),
            format!("a <span style=\"display:none\"><!-- </span> -->{p}</span> b"),
            format!("a <span style=\"display:none\"><b title=\"</span>\">{p}</b></span> b"),
            format!("a <div style=\"display:none\"><script>\"</div>\"</script>{p}</div> b"),
            format!("a <span style=\"display:none\"/>{p}</span> b"),
            format!("<style>.x{{display:none}}</style>a <span class=\"x\">{p}</span> b"),
            format!(
                "a <span class=\"x\">{p}</span> b<div hidden><style>.x{{display:none}}</style></div>"
            ),
            format!("<link rel=\"Stylesheet\" href=\"x.css\">a <span class=\"x\">{p}</span>"),
            format!("a <template>{p}</template> b"),
            format!("a <details><summary>s</summary>{p}</details> b"),
            format!("a <![CDATA[{p}]]> b"),
            format!("a <!{p}> b"),
            format!("a <?{p}?> b"),
            format!("a <SPAN STYLE=\"DISPLAY:NONE\">{p}</SPAN> b"),
            format!("a <span style=display:none>{p}</span> b"),
            format!("a <p hidden>{p}</p> b"),
            format!("a <font color=\"#ffffff\">{p}</font> b"),
            format!("a <script>{p}</script> b"),
            format!("a <span style=\"display:none\">{p}"),
            format!("a <div style=\"display:none\"><div><div></div></div>{p}</div> b"),
        ] {
            assert!(
                !strip_hidden(&text).contains(p),
                "{text}: {}",
                strip_hidden(&text)
            );
        }
        for (text, want) in [
            (
                "1 < 2 and <b>bold</b> <a title=\"display:none\" data-x=hidden>link</a>",
                None,
            ),
            ("<style>p{font-weight:bold}</style>after", Some("after")),
            (
                "<style>@media (max-width:600px){.a{color:#000}}</style>after",
                Some("after"),
            ),
            ("a <details open><summary>s</summary>shown</details>", None),
            ("a <!-- note --!> shown", None),
            ("a <img style=\"display:none\" src=x>b", Some("a b")),
            ("<textarea></div></textarea>kept", None),
        ] {
            let want = want.unwrap_or(text);
            assert_eq!(strip_hidden(text), want, "{text}");
        }
        assert_eq!(
            strip_comments_strict("a<!-- x -->b<![CDATA[c]]>d<!e>f<?g?>h"),
            "abdfh"
        );
    }

    #[test]
    fn hiding_by_the_listed_techniques_is_dropped() {
        let p = "PAYNOW";
        let five = |open: &str, close: &str| format!("{}{p}{}", open.repeat(5), close.repeat(5));
        for text in [
            format!("a <div style=\"display:table-column\">{p}</div> b"),
            format!("a <div style=\"display:table-column-group\">{p}</div> b"),
            format!("a <div style=\"line-height:0;overflow:hidden\">{p}</div> b"),
            format!("a <div style=\"font:12px/0 Arial;overflow:hidden\">{p}</div> b"),
            format!(
                "a <div style=\"width:100px;overflow:hidden;white-space:nowrap;text-indent:200px\">{p}</div> b"
            ),
            format!("a <div style=\"width:100px;padding-left:300px;overflow:hidden\">{p}</div> b"),
            format!("a <div style=\"margin-left:9999px\">{p}</div> b"),
            format!("a <div style=\"border-left:9999px solid #000\">{p}</div> b"),
            format!("a <span style=\"letter-spacing:9999px\">{p}</span> b"),
            five("<span style=\"font-size:0.5em\">", "</span>"),
            five("<span style=\"font-size:50%\">", "</span>"),
            five("<span style=\"opacity:0.5\">", "</span>"),
            format!("a <span style=\"color:#bbb;background:#bbb\">{p}</span> b"),
            format!(
                "a <div style=\"background:#999\"><span style=\"color:#999\">{p}</span></div> b"
            ),
            format!(
                "<table bgcolor=\"#000000\"><tr><td><font color=\"#000000\">{p}</font></td></tr></table>"
            ),
            format!("<body text=\"#ffffff\">a {p} b</body>"),
            format!("a <svg width=\"0\" height=\"0\"><text y=\"10\">{p}</text></svg> b"),
            format!("a <svg><desc>{p}</desc><metadata>{p}</metadata></svg> b"),
            format!("a <title>{p}</title> b"),
            format!("a <dialog>{p}</dialog> b"),
            format!("a <datalist><option value=\"x\">{p}</option></datalist> b"),
            format!("a <ruby>x<rp>{p}</rp><rt>y</rt></ruby> b"),
            format!("a <video src=\"v.mp4\">{p}</video> b"),
            format!("a <audio src=\"a.mp3\">{p}</audio> b"),
            format!("a <canvas>{p}</canvas> b"),
            format!("a <script><!--<script>x</script>{p}</script>--> b"),
        ] {
            assert!(
                !strip_hidden(&text).contains(p),
                "{text}: {}",
                strip_hidden(&text)
            );
        }
        for text in [
            "<span style=\"font-size:0.8em\"><span style=\"font-size:0.8em\">kept</span></span>",
            "<div style=\"background:#fff\"><span style=\"color:#333\">kept</span></div>",
            "<dialog open>kept</dialog>",
            "<p style=\"margin:0 auto;padding:4px\">kept</p>",
            "<script>var a = 1;</script>kept",
        ] {
            assert!(strip_hidden(text).contains("kept"), "{text}");
        }
    }

    #[test]
    fn markup_is_read_in_time_that_grows_with_its_length() {
        for unit in [
            "<!---->",
            "<!-- x --!>",
            "<!-- x",
            "<script><!--<script>",
            "<b>",
        ] {
            let text = unit.repeat(2_000_000 / unit.len());
            let started = std::time::Instant::now();
            strip_hidden(&text);
            strip_comments_strict(&text);
            assert!(
                started.elapsed().as_secs() < 10,
                "{unit}: {:?}",
                started.elapsed()
            );
        }
    }
}
