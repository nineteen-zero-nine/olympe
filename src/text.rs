use std::time::{SystemTime, UNIX_EPOCH};

/// Replace each `{key}` of `template` with its value, in one pass, so that a
/// value holding `{another-key}` is left as it is.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        let hit = values.iter().find(|(key, _)| {
            rest[1..]
                .strip_prefix(key)
                .is_some_and(|after| after.starts_with('}'))
        });
        match hit {
            Some((key, value)) => {
                out.push_str(value);
                rest = &rest[key.len() + 2..];
            }
            None => {
                out.push('{');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether `text` can name a project, a process or a specialist: lowercase
/// letters, digits and hyphens, starting with a letter or a digit.
pub fn is_name(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The closest usable name to `text`: `My Project_2` gives `my-project-2`.
pub fn to_name(text: &str) -> String {
    let mut name = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    name.trim_end_matches('-').to_string()
}

/// The first `max` characters of `text`, for error messages.
pub fn excerpt(text: &str, max: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

pub struct Now {
    /// `2026-11-27`
    pub date: String,
    /// `20261127T214005Z`, which sorts by time and fits in a file name.
    pub stamp: String,
}

pub fn now() -> Now {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    at(secs)
}

fn at(secs: i64) -> Now {
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    Now {
        date: format!("{year:04}-{month:02}-{day:02}"),
        stamp: format!(
            "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        ),
    }
}

/// Days since 1970-01-01 to a (year, month, day) in the Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_known_keys_once() {
        let out = fill("a {x} b {y} {z} {x}", &[("x", "{y}"), ("y", "2")]);
        assert_eq!(out, "a {y} b 2 {z} {y}");
    }

    #[test]
    fn names() {
        assert!(is_name("code-review-2"));
        assert!(!is_name(""));
        assert!(!is_name("-review"));
        assert!(!is_name("Review"));
        assert!(!is_name("a/b"));
        assert_eq!(to_name("My Project_2!"), "my-project-2");
        assert_eq!(to_name("--"), "");
    }

    #[test]
    fn formats_known_timestamps() {
        assert_eq!(at(0).stamp, "19700101T000000Z");
        assert_eq!(at(951_782_400).date, "2000-02-29");
        assert_eq!(at(1_795_815_605).stamp, "20261127T214005Z");
    }
}
