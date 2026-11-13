//! Slash commands, from `imperatorium/slash.py`: what counts as one. Typing
//! one into a terminal comes later in step 2.

/// slash.parse: (name lowercased, rest) for a slash command, else None.
pub fn parse(text: &str) -> Option<(String, String)> {
    let text = crate::pyish::strip(text);
    let mut chars = text.chars();
    if chars.next() != Some('/') {
        return None;
    }
    match chars.next() {
        None | Some('/') | Some(' ') => return None,
        _ => {}
    }
    let body = &text[1..];
    let (name, rest) = body.split_once(' ').unwrap_or((body, ""));
    Some((name.to_lowercase(), crate::pyish::strip(rest).to_string()))
}

pub fn is_command(text: &str) -> bool {
    parse(text).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_matches_python() {
        assert_eq!(
            parse("  /Model opus "),
            Some(("model".into(), "opus".into()))
        );
        assert_eq!(parse("/"), None);
        assert_eq!(parse("// x"), None);
        assert_eq!(parse("/ x"), None);
        assert_eq!(parse("hello /x"), None);
    }
}
