//! What a piece of text is, so its tile can say so: a color, code, an email
//! address or a phone number. Every check is strict: when in doubt, text
//! stays text.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Form {
    Text,
    Code,
    /// Red, green, blue and alpha.
    Color([u8; 4]),
    Email,
    Phone,
}

/// Only the start of a long text is looked at.
const LOOK: usize = 4096;

pub fn detect(text: &str) -> Form {
    let head = match text.char_indices().nth(LOOK) {
        Some((at, _)) => &text[..at],
        None => text,
    };
    let t = head.trim();
    if let Some(c) = color(t) {
        Form::Color(c)
    } else if is_email(t) {
        Form::Email
    } else if is_phone(t) {
        Form::Phone
    } else if is_code(head) {
        Form::Code
    } else {
        Form::Text
    }
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`/`rgba()` and
/// `hsl()`/`hsla()`, with spaces or commas and an optional alpha.
pub fn color(t: &str) -> Option<[u8; 4]> {
    let t = t.to_ascii_lowercase();
    if let Some(hex) = t.strip_prefix('#') {
        let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
        return match digits[..] {
            [r, g, b] => Some([r * 17, g * 17, b * 17, 255]),
            [r, g, b, a] => Some([r * 17, g * 17, b * 17, a * 17]),
            [r1, r2, g1, g2, b1, b2] => Some([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2, 255]),
            [r1, r2, g1, g2, b1, b2, a1, a2] => Some([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2, a1 * 16 + a2]),
            _ => None,
        };
    }
    let (name, rest) = t.split_once('(')?;
    let args = rest.strip_suffix(')')?;
    let parts: Vec<&str> = args.split([' ', ',', '/']).filter(|p| !p.is_empty()).collect();
    let alpha = match parts.get(3) {
        None => 255,
        Some(a) => (fraction(a)? * 255.0).round() as u8,
    };
    if parts.len() > 4 || parts.len() < 3 {
        return None;
    }
    match name.trim() {
        "rgb" | "rgba" => {
            let ch = |p: &str| match p.strip_suffix('%') {
                Some(pc) => percent(pc).map(|f| (f * 255.0).round() as u8),
                None => p.parse::<f32>().ok().filter(|v| (0.0..=255.0).contains(v)).map(|v| v.round() as u8),
            };
            Some([ch(parts[0])?, ch(parts[1])?, ch(parts[2])?, alpha])
        }
        "hsl" | "hsla" => {
            let h = parts[0].trim_end_matches("deg").parse::<f32>().ok()?.rem_euclid(360.0);
            let s = percent(parts[1].strip_suffix('%')?)?;
            let l = percent(parts[2].strip_suffix('%')?)?;
            let [r, g, b] = hsl_to_rgb(h, s, l);
            Some([r, g, b, alpha])
        }
        _ => None,
    }
}

fn percent(p: &str) -> Option<f32> {
    p.parse::<f32>().ok().filter(|v| (0.0..=100.0).contains(v)).map(|v| v / 100.0)
}

/// An alpha as `0.5` or `50%`.
fn fraction(p: &str) -> Option<f32> {
    match p.strip_suffix('%') {
        Some(pc) => percent(pc),
        None => p.parse::<f32>().ok().filter(|v| (0.0..=1.0).contains(v)),
    }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|v| ((v + m) * 255.0).round() as u8)
}

pub fn hex([r, g, b, a]: [u8; 4]) -> String {
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

fn alpha_suffix(a: u8) -> String {
    if a == 255 { String::new() } else { format!(" / {}", (a as f32 / 255.0 * 100.0).round() / 100.0) }
}

pub fn rgb([r, g, b, a]: [u8; 4]) -> String {
    format!("rgb({r} {g} {b}{})", alpha_suffix(a))
}

pub fn hsl([r, g, b, a]: [u8; 4]) -> String {
    let [r, g, b] = [r, g, b].map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    format!("hsl({} {}% {}%{})", h.round(), (s * 100.0).round(), (l * 100.0).round(), alpha_suffix(a))
}

fn is_email(t: &str) -> bool {
    let Some((local, domain)) = t.split_once('@') else { return false };
    let ok = |c: char| c.is_alphanumeric() || "._%+-".contains(c);
    t.len() <= 254
        && !local.is_empty()
        && local.chars().all(ok)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain.chars().all(|c| c.is_alphanumeric() || ".-".contains(c))
}

/// Seven to fifteen digits with a leading "+", brackets, spaces or dashes.
/// Bare digit strings and dates are not phone numbers.
fn is_phone(t: &str) -> bool {
    if !t.chars().all(|c| c.is_ascii_digit() || " +-()".contains(c)) || t.rfind('+').is_some_and(|i| i > 0) {
        return false;
    }
    let digits = t.chars().filter(char::is_ascii_digit).count();
    let groups: Vec<usize> = t.split('-').map(str::len).collect();
    let date = matches!(groups[..], [4, 1..=2, 1..=2] | [1..=2, 1..=2, 4]);
    (7..=15).contains(&digits) && (t.starts_with('+') || t.contains(['(', ' ', '-'])) && !date
}

/// A shell command, a line ending like a statement or block, or several
/// lines that mostly look like code.
fn is_code(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let Some(first) = lines.first() else { return false };
    let f = first.trim_start();
    // "#" is left out: it also starts Markdown headings.
    let prompt = f.starts_with("$ ") || f.starts_with("> ") || f.starts_with("PS>") || f.starts_with("PS ") && f.contains('>');
    // A line that ends like a statement or block; indentation alone also
    // fits a list, so it only counts beside such lines.
    let statement = |l: &&str| {
        let t = l.trim_end();
        t.ends_with([';', '{', '}']) || t.ends_with(')') && t.contains('(') && !t.ends_with(".)")
    };
    let indented = |l: &&str| l.starts_with('\t') || l.starts_with("  ");
    let strong = lines.iter().filter(|l| statement(l)).count();
    let prose = lines.iter().any(|l| l.trim_end().ends_with(['.', '?', '!']));
    match lines.len() {
        1 => prompt || first.trim_end().ends_with([';', '{']) || first.contains("=>") || first.contains("::"),
        n => prompt || strong > 0 && !prose && lines.iter().filter(|l| statement(l) || indented(l)).count() * 2 >= n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(detect("#3B82F6"), Form::Color([59, 130, 246, 255]));
        assert_eq!(detect(" #fff "), Form::Color([255, 255, 255, 255]));
        assert_eq!(detect("#3b82f680"), Form::Color([59, 130, 246, 128]));
        assert_eq!(detect("rgb(59, 130, 246)"), Form::Color([59, 130, 246, 255]));
        assert_eq!(detect("rgba(59 130 246 / 50%)"), Form::Color([59, 130, 246, 128]));
        assert_eq!(detect("hsl(217 91% 60%)"), Form::Color([60, 131, 246, 255]));
        assert_eq!(detect("#12"), Form::Text);
        assert_eq!(detect("rgb(300 0 0)"), Form::Text);
        assert_eq!(detect("The color is #3b82f6"), Form::Text);
    }

    #[test]
    fn color_formats() {
        let c = [59, 130, 246, 255];
        assert_eq!(hex(c), "#3b82f6");
        assert_eq!(rgb(c), "rgb(59 130 246)");
        assert_eq!(hsl(c), "hsl(217 91% 60%)");
        assert_eq!(rgb([0, 0, 0, 128]), "rgb(0 0 0 / 0.5)");
        assert_eq!(hex([0, 0, 0, 128]), "#00000080");
    }

    #[test]
    fn emails_and_phones() {
        assert_eq!(detect("dana.k+invoices@example.co.uk"), Form::Email);
        assert_eq!(detect("not an@email"), Form::Text);
        assert_eq!(detect("@handle"), Form::Text);
        assert_eq!(detect("+44 20 7946 0958"), Form::Phone);
        assert_eq!(detect("(555) 123-4567"), Form::Phone);
        assert_eq!(detect("555-1234"), Form::Phone);
        assert_eq!(detect("2026-11-03"), Form::Text);
        assert_eq!(detect("12345678"), Form::Text);
        assert_eq!(detect("1-2"), Form::Text);
    }

    #[test]
    fn code_and_prose() {
        assert_eq!(detect("let x = 1;"), Form::Code);
        assert_eq!(detect("$ cargo build --release"), Form::Code);
        assert_eq!(detect("PS> Get-ChildItem"), Form::Code);
        assert_eq!(detect("fn main() {\n    println!(\"hi\");\n}"), Form::Code);
        assert_eq!(detect("Meet at 4 (by the gate)"), Form::Text);
        assert_eq!(detect("Send the files.\nThen call me (after lunch)."), Form::Text);
        assert_eq!(detect("PS C:\\src> cargo test"), Form::Code);
        assert_eq!(detect("Shopping:\n  milk\n  bread"), Form::Text);
        assert_eq!(detect("# Notes\nRemember the keys"), Form::Text);
    }
}
