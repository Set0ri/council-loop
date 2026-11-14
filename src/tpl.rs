//! `string.Template(text).safe_substitute(...)`: the placeholder rules of the prompt packets.
//!
//! `$name` and `${name}` are replaced when the name is known and left alone when it is not, and
//! `$$` is one dollar sign. Names are ASCII: a letter or underscore, then letters, digits, underscores.

use std::collections::HashMap;

fn ident_len(text: &str) -> usize {
    let mut n = 0;
    for (i, c) in text.char_indices() {
        let ok = c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit());
        if !ok {
            break;
        }
        n = i + c.len_utf8();
    }
    n
}

/// Fill a template. Unknown placeholders stay as they were written.
pub fn substitute(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(template.len() + 256);
    let mut rest = template;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if let Some(stripped) = after.strip_prefix('$') {
            out.push('$');
            rest = stripped;
            continue;
        }
        let n = ident_len(after);
        if n > 0 {
            match vars.get(&after[..n]) {
                Some(value) => out.push_str(value),
                None => out.push_str(&rest[at..at + 1 + n]),
            }
            rest = &after[n..];
            continue;
        }
        if let Some(inner) = after.strip_prefix('{') {
            let n = ident_len(inner);
            if n > 0 && inner[n..].starts_with('}') {
                match vars.get(&inner[..n]) {
                    Some(value) => out.push_str(value),
                    None => out.push_str(&rest[at..at + n + 3]),
                }
                rest = &inner[n + 1..];
                continue;
            }
        }
        out.push('$');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Build the variable table for [`substitute`] from `name => value` pairs.
#[macro_export]
macro_rules! vars {
    ($($name:ident = $value:expr),* $(,)?) => {{
        let mut map: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
        $( map.insert(stringify!($name), ($value).to_string()); )*
        map
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_follow_string_template() {
        let v = crate::vars!(name = "Ada", n = 3);
        assert_eq!(
            substitute("Hi $name, ${n} items, $$5, $unknown ${unknown} $ 1 ${n", &v),
            "Hi Ada, 3 items, $5, $unknown ${unknown} $ 1 ${n"
        );
        assert_eq!(
            substitute("$name_x $name. $n3 ${name}x $", &v),
            "$name_x Ada. $n3 Adax $"
        );
        assert_eq!(
            substitute("no placeholders → here", &v),
            "no placeholders → here"
        );
    }
}
