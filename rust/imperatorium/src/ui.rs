//! The window's pages and fonts, compiled in. The sources are the files in
//! `imperatorium/web/` (the Python dispatcher reads the same ones); this
//! assembles each page from them as `ui.py` does and adds what
//! `ui._with_meta` adds per start - the window token, the boot id and
//! `look_head` - exactly as it does.

use serde_json::Value;

use crate::pyish::{float_repr, truthy};

macro_rules! web {
    ($name:literal) => {
        include_str!(concat!("../../../imperatorium/web/", $name))
    };
}

const APP_CSS: &str = web!("app.css");
const MATRIX_CSS: &str = web!("matrix.css");
const SETTINGS_CSS: &str = web!("settings.css");
const SHARED_JS: &str = web!("shared.js");
const APP_JS: &str = web!("app.js");
const MATRIX_JS: &str = web!("matrix.js");
const SETTINGS_JS: &str = web!("settings.js");

/// The files the pages link to, by the name in `/ui/<name>`.
const ASSETS: &[(&str, &str, &str)] = &[
    ("app.css", "text/css; charset=utf-8", APP_CSS),
    ("matrix.css", "text/css; charset=utf-8", MATRIX_CSS),
    ("settings.css", "text/css; charset=utf-8", SETTINGS_CSS),
    ("shared.js", "text/javascript; charset=utf-8", SHARED_JS),
    ("app.js", "text/javascript; charset=utf-8", APP_JS),
    ("matrix.js", "text/javascript; charset=utf-8", MATRIX_JS),
    ("settings.js", "text/javascript; charset=utf-8", SETTINGS_JS),
];

/// What identifies a version of a file: the first 12 hex digits of its
/// SHA-256. Pages carry it as `?v=`, so a changed file is a new URL.
pub fn version(name: &str) -> Option<&'static str> {
    use sha2::{Digest, Sha256};
    use std::sync::OnceLock;
    static VERSIONS: OnceLock<Vec<String>> = OnceLock::new();
    let all = VERSIONS.get_or_init(|| {
        ASSETS
            .iter()
            .map(|(_, _, body)| {
                let sum = Sha256::digest(body.as_bytes());
                sum.iter().take(6).map(|b| format!("{b:02x}")).collect()
            })
            .collect()
    });
    ASSETS
        .iter()
        .position(|(n, _, _)| *n == name)
        .map(|i| all[i].as_str())
}

/// `/ui/<name>`: (Content-Type, body) of a style or script, if there is one.
pub fn asset(name: &str) -> Option<(&'static str, &'static str)> {
    ASSETS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, ctype, body)| (*ctype, *body))
}

/// A page's skeleton (`{{STYLES}}` and `{{SCRIPTS}}` marks) and the files
/// that fill them.
pub struct Page {
    template: &'static str,
    styles: &'static [&'static str],
    scripts: &'static [&'static str],
}

pub const SHELL: Page = Page {
    template: web!("shell.html"),
    styles: &["app.css"],
    scripts: &["shared.js", "app.js"],
};
pub const MATRIX: Page = Page {
    template: web!("matrix.html"),
    styles: &["app.css", "matrix.css"],
    scripts: &["shared.js", "matrix.js"],
};
pub const SETTINGS: Page = Page {
    template: web!("settings.html"),
    styles: &["app.css", "settings.css"],
    scripts: &["shared.js", "settings.js"],
};

impl Page {
    /// The page with its styles and scripts linked as `/ui/<name>?v=<hash>`
    /// (see `asset`), so a browser keeps them between loads and the page
    /// itself is a few kilobytes. (The Python dispatcher sends the same
    /// page with them inline.)
    pub fn linked(&self) -> String {
        let (before, rest) = self.template.split_once("{{STYLES}}").unwrap_or((self.template, ""));
        let (middle, after) = rest.split_once("{{SCRIPTS}}").unwrap_or((rest, ""));
        let url = |n: &str| format!("/ui/{n}?v={}", version(n).unwrap_or(""));
        let mut out = String::with_capacity(self.template.len() + 600);
        out.push_str(before);
        for n in self.styles {
            out.push_str(&format!("<link rel=stylesheet href=\"{}\">", url(n)));
        }
        out.push_str(middle);
        for n in self.scripts {
            out.push_str(&format!("<script src=\"{}\"></script>", url(n)));
        }
        out.push_str(after);
        out
    }
}

const FONTS: &[(&str, &[u8])] = &[
    (
        "mono-latin-ext.woff2",
        include_bytes!("../../../imperatorium/fonts/mono-latin-ext.woff2"),
    ),
    (
        "mono-latin.woff2",
        include_bytes!("../../../imperatorium/fonts/mono-latin.woff2"),
    ),
    (
        "next-italic-latin-ext.woff2",
        include_bytes!("../../../imperatorium/fonts/next-italic-latin-ext.woff2"),
    ),
    (
        "next-italic-latin.woff2",
        include_bytes!("../../../imperatorium/fonts/next-italic-latin.woff2"),
    ),
    (
        "next-latin-ext.woff2",
        include_bytes!("../../../imperatorium/fonts/next-latin-ext.woff2"),
    ),
    (
        "next-latin.woff2",
        include_bytes!("../../../imperatorium/fonts/next-latin.woff2"),
    ),
];

/// ui.font: a bundled .woff2 by its exact name.
pub fn font(name: &str) -> Option<&'static [u8]> {
    FONTS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

const FONT_BASE: &[(&str, f64)] = &[("chat", 14.0), ("matrix", 13.0)];
const HIGHLIGHTS_ON: &[&str] = &[
    "code", "diffs", "prose", "tools", "signals", "mute", "guides", "shape", "prompts",
];

/// Python's `$`: the end, or just before a final newline.
fn full(s: &str, ok: impl Fn(&str) -> bool) -> bool {
    ok(s.strip_suffix('\n').unwrap_or(s))
}

fn word(s: &str) -> bool {
    full(s, |t| {
        !t.is_empty() && t.bytes().all(|b| b.is_ascii_lowercase())
    })
}

fn hex6(s: &str) -> bool {
    full(s, |t| {
        t.len() == 7 && t.starts_with('#') && t[1..].bytes().all(|b| b.is_ascii_hexdigit())
    })
}

/// `round(x, 4)`, which rounds the exact binary value, as `{:.4}` does.
fn round4(x: f64) -> f64 {
    format!("{x:.4}").parse().unwrap_or(x)
}

/// Python's `float(value)` for a look font size: a number or numeric text.
fn as_float(v: &Value) -> Option<f64> {
    crate::pyish::py_float(v)
}

/// ui.look_head: text sizes, highlight colours and the html.hl-* classes.
pub fn look_head(look: &Value) -> String {
    let empty = serde_json::Map::new();
    let look = look.as_object().unwrap_or(&empty);
    let fonts = look
        .get("fonts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut props: Vec<String> = Vec::new();
    for (name, base) in FONT_BASE {
        let size = match fonts.get(*name).filter(|v| truthy(Some(v))) {
            Some(v) => as_float(v).unwrap_or(*base),
            None => *base,
        };
        props.push(format!("--{name}z:{}", float_repr(round4(size / base))));
    }
    let mut colors: Vec<(String, Value)> = look
        .get("colors")
        .and_then(Value::as_object)
        .map(|c| c.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    colors.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, v) in colors {
        let v = crate::pyish::py_str(&v);
        if word(&name) && hex6(&v) {
            props.push(format!("--hl-{name}:{}", v.to_lowercase()));
        }
    }
    let hl = look.get("hl").filter(|h| !h.is_null());
    let mut on: Vec<String> = HIGHLIGHTS_ON
        .iter()
        .filter(|n| hl.is_none() || truthy(hl.and_then(|h| h.get(**n))))
        .map(|n| n.to_string())
        .collect();
    if let Some(Value::Object(h)) = hl {
        for (n, v) in h {
            if truthy(Some(v)) && !on.contains(n) && word(n) {
                on.push(n.clone());
            }
        }
    }
    let classes = if on.is_empty() {
        String::new()
    } else {
        let args: Vec<String> = on
            .iter()
            .map(|n| crate::pyish::json_dumps(&Value::String(format!("hl-{n}")), true))
            .collect();
        format!(
            "<script>document.documentElement.classList.add({})</script>",
            args.join(",")
        )
    };
    format!(
        "<style id=lookvars>html:root{{{}}}</style>{classes}",
        props.join(";")
    )
}

/// html.escape(s, quote=True).
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// ui._with_meta: the token, the boot id and (unless `look` is None) the
/// look, after the page's first `<meta charset=utf-8>`.
pub fn with_meta(token: &str, page: &str, boot: &str, look: Option<&Value>) -> String {
    let head = "<meta charset=utf-8>";
    let extra = format!(
        "<meta name=imp-window content=\"{}\"><meta name=imp-boot content=\"{}\">{}",
        escape(token),
        escape(boot),
        look.map(look_head).unwrap_or_default()
    );
    page.replacen(head, &format!("{head}{extra}"), 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn look_head_matches_python() {
        // python -c "from imperatorium import ui; print(ui.look_head(None))"
        assert_eq!(
            look_head(&Value::Null),
            "<style id=lookvars>html:root{--chatz:1.0;--matrixz:1.0}</style><script>document.documentElement\
             .classList.add(\"hl-code\",\"hl-diffs\",\"hl-prose\",\"hl-tools\",\"hl-signals\",\"hl-mute\",\
             \"hl-guides\",\"hl-shape\",\"hl-prompts\")</script>"
        );
        let l = json!({"fonts": {"chat": 15, "matrix": 13}, "hl": {"code": true, "mute": false},
                       "colors": {"error": "#FF0000", "Bad": "#000000"}});
        assert_eq!(
            look_head(&l),
            "<style id=lookvars>html:root{--chatz:1.0714;--matrixz:1.0;--hl-error:#ff0000}</style>\
             <script>document.documentElement.classList.add(\"hl-code\")</script>"
        );
    }

    #[test]
    fn meta_is_inserted_once() {
        let page = "<html><head><meta charset=utf-8><meta charset=utf-8></head></html>";
        let out = with_meta("t<k>", page, "b\"1", None);
        assert_eq!(
            out,
            "<html><head><meta charset=utf-8><meta name=imp-window content=\"t&lt;k&gt;\">\
             <meta name=imp-boot content=\"b&quot;1\"><meta charset=utf-8></head></html>"
        );
    }

    #[test]
    fn pages_link_their_files_by_version() {
        let page = SHELL.linked();
        let v = version("app.js").unwrap();
        assert_eq!(v.len(), 12);
        assert!(page.contains(&format!("<script src=\"/ui/app.js?v={v}\"></script>")));
        assert!(page.contains("<link rel=stylesheet href=\"/ui/app.css?v="));
        assert!(!page.contains("{{"));
        assert!(page.len() < 20_000, "a linked page is a skeleton");
        // shared.js comes before the script that uses it.
        assert!(page.find("/ui/shared.js").unwrap() < page.find("/ui/app.js").unwrap());
        for p in [&SHELL, &MATRIX, &SETTINGS] {
            for n in p.styles.iter().chain(p.scripts) {
                assert!(asset(n).is_some() && version(n).is_some(), "{n}");
            }
        }
        assert!(asset("../ui.rs").is_none());
    }

    #[test]
    fn fonts_are_bundled() {
        assert!(font("next-latin.woff2").is_some_and(|b| b.starts_with(b"wOF2")));
        assert!(font("../secret").is_none());
    }

    // test_ui.py LookHead (the Python look_head tests; the window JavaScript
    // ones are in imperatorium/web/tests).
    #[test]
    fn look_head_defaults_turn_every_highlight_on() {
        let head = look_head(&Value::Null);
        assert!(head.contains("--chatz:1.0;--matrixz:1.0"), "{head}");
        for name in ["code", "diffs", "prose", "tools", "signals", "mute", "guides", "shape"] {
            assert!(head.contains(&format!("\"hl-{name}\"")), "{name}: {head}");
        }
    }

    #[test]
    fn look_head_lets_in_only_hex_colours_and_known_shapes() {
        let head = look_head(&json!({
            "fonts": {"chat": 21}, "hl": {"code": true, "diffs": false},
            "colors": {"keyword": "#AABBCC", "string": "red;}</style>", "bad name": "#000000"}}));
        assert!(head.contains("--chatz:1.5"), "{head}");
        assert!(head.contains("--hl-keyword:#aabbcc"), "{head}");
        assert!(!head.contains("red"), "{head}");
        assert!(!head.contains("bad name"), "{head}");
        assert!(head.contains("\"hl-code\""), "{head}");
        assert!(!head.contains("\"hl-diffs\""), "{head}");
    }

    #[test]
    fn settings_page_has_no_look_and_the_matrix_has() {
        let page = "<html><head><meta charset=utf-8></head></html>";
        assert!(!with_meta("t", page, "b", None).contains("<style id=lookvars"));
        assert!(with_meta("", page, "b", Some(&Value::Null)).contains("<style id=lookvars"));
    }
}
