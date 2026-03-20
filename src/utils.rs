use std::path::Path;

use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

/// Format a file path entry for display, progressively abbreviating to fit `max_width`
/// display columns.
///
/// 1. Full relative path
/// 2. Directory segments abbreviated to 3 characters
/// 2. Directory segments abbreviated to 2 characters
/// 3. Directory segments abbreviated to 1 character
/// 4. Ellipsis in the basename
/// 5. Right-aligned truncation
#[must_use]
pub fn format_file_entry(rel: &Path, suffix: &str, max_width: usize) -> String {
    let mut dirs: Vec<String> = rel
        .components()
        .filter_map(|c| {
            if let std::path::Component::Normal(s) = c {
                s.to_str().map(String::from)
            } else {
                None
            }
        })
        .collect();

    let Some(filename) = dirs.pop() else {
        return suffix.to_string();
    };

    // full path
    let path = join_path(&dirs, &filename);
    if fits(&path, suffix, max_width) {
        return with_suffix(&path, suffix);
    }

    // abbreviate directory segments to 3 characters
    for dir in &mut dirs {
        truncate_chars(dir, 3);
    }
    let path = join_path(&dirs, &filename);
    if fits(&path, suffix, max_width) {
        return with_suffix(&path, suffix);
    }

    // abbreviate directory segments to 2 characters
    for dir in &mut dirs {
        truncate_chars(dir, 2);
    }
    let path = join_path(&dirs, &filename);
    if fits(&path, suffix, max_width) {
        return with_suffix(&path, suffix);
    }

    // abbreviate directory segments to 1 character
    for dir in &mut dirs {
        truncate_chars(dir, 1);
    }
    let path = join_path(&dirs, &filename);
    if fits(&path, suffix, max_width) {
        return with_suffix(&path, suffix);
    }

    // ellipsis in the file stem
    let (stem, ext) = split_stem_ext(&filename);
    let dir_prefix = dir_prefix_string(&dirs);
    let stem_chars: Vec<char> = stem.chars().collect();

    if stem_chars.len() > 3 {
        // overhead = dir_prefix + ellipsis + ext + suffix
        let overhead = dir_prefix.width() + 1 + ext.width() + suffix.width();
        let stem_width: usize = stem_chars.iter().filter_map(|c| c.width()).sum();
        if max_width > overhead && max_width - overhead < stem_width {
            let avail_cols = max_width - overhead;
            let (start_n, start_w) =
                chars_within_width(stem_chars.iter().copied(), avail_cols.div_ceil(2));
            let (end_n, _) =
                chars_within_width(stem_chars.iter().rev().copied(), avail_cols - start_w);
            if start_n >= 1 && end_n >= 1 {
                let start: String = stem_chars[..start_n].iter().collect();
                let end: String = stem_chars[stem_chars.len() - end_n..].iter().collect();
                return format!("{dir_prefix}{start}\u{2026}{end}{ext}{suffix}");
            }
        }
    }

    // right-aligned truncation of the most compact form
    let compact_stem = if stem_chars.len() > 3 {
        format!(
            "{}\u{2026}{}",
            stem_chars[0],
            stem_chars[stem_chars.len() - 1]
        )
    } else {
        stem.to_string()
    };
    let compact = format!("{dir_prefix}{compact_stem}{ext}{suffix}");
    let compact_w = compact.width();

    if compact_w <= max_width {
        return compact;
    }

    truncate_left(&compact, compact_w - max_width)
}

fn chars_within_width(chars: impl Iterator<Item = char>, max_cols: usize) -> (usize, usize) {
    let mut total = 0;
    let count = chars
        .take_while(|c| {
            let w = c.width().unwrap_or_default();
            if total + w > max_cols {
                return false;
            }
            total += w;
            true
        })
        .count();
    (count, total)
}

fn fits(path: &str, suffix: &str, max_width: usize) -> bool {
    path.width() + suffix.width() <= max_width
}

fn with_suffix(path: &str, suffix: &str) -> String {
    format!("{path}{suffix}")
}

fn truncate_chars(s: &mut String, max_chars: usize) {
    if let Some((idx, _)) = s.char_indices().nth(max_chars) {
        s.truncate(idx);
    }
}

fn join_path(dirs: &[String], filename: &str) -> String {
    if dirs.is_empty() {
        filename.to_string()
    } else {
        format!("{}/{filename}", dirs.join("/"))
    }
}

fn dir_prefix_string(dirs: &[String]) -> String {
    if dirs.is_empty() {
        String::new()
    } else {
        format!("{}/", dirs.join("/"))
    }
}

fn split_stem_ext(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        Some(pos) if pos > 0 => (&filename[..pos], &filename[pos..]),
        _ => (filename, ""),
    }
}

fn truncate_left(s: &str, excess_cols: usize) -> String {
    let mut removed = 0;
    let mut byte_offset = 0;
    for ch in s.chars() {
        if removed >= excess_cols {
            break;
        }
        removed += ch.width().unwrap_or(0);
        byte_offset += ch.len_utf8();
    }
    s[byte_offset..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_path_when_it_fits() {
        let path = Path::new("src/main.rs");
        assert_eq!(format_file_entry(path, " (3/5)", 50), "src/main.rs (3/5)");
    }

    #[test]
    fn dirs_abbreviated_to_two_chars() {
        let path = Path::new("src/components/widgets/MyFile.rs");
        assert_eq!(
            format_file_entry(path, " (1/2)", 30),
            "sr/co/wi/MyFile.rs (1/2)"
        );
    }

    #[test]
    fn dirs_abbreviated_to_one_char() {
        let path = Path::new("src/components/widgets/MyFile.rs");
        assert_eq!(
            format_file_entry(path, " (1/2)", 21),
            "s/c/w/MyFile.rs (1/2)"
        );
    }

    #[test]
    fn ellipsis_in_stem_minimal() {
        let path = Path::new("src/components/widgets/MyFile.rs");
        // overhead = "s/c/w/"(6) + ellipsis(1) + ".rs"(3) + " (1/2)"(6) = 16
        // avail = 18 - 16 = 2 → start=1, end=1 → "M…e"
        assert_eq!(
            format_file_entry(path, " (1/2)", 18),
            "s/c/w/M\u{2026}e.rs (1/2)"
        );
    }

    #[test]
    fn ellipsis_in_stem_with_more_chars() {
        let path = Path::new("src/components/widgets/MyFile.rs");
        // overhead = 16, avail = 20 - 16 = 4 → start=2, end=2 → "My…le"
        assert_eq!(
            format_file_entry(path, " (1/2)", 20),
            "s/c/w/My\u{2026}le.rs (1/2)"
        );
    }

    #[test]
    fn right_aligned_truncation() {
        let path = Path::new("src/components/widgets/MyFile.rs");
        // compact: "s/c/w/M…e.rs (1/2)" = 18 cols, truncate 3
        assert_eq!(
            format_file_entry(path, " (1/2)", 15),
            "/w/M\u{2026}e.rs (1/2)"
        );
    }

    #[test]
    fn no_directories() {
        let path = Path::new("README.md");
        assert_eq!(format_file_entry(path, " (2/3)", 20), "README.md (2/3)");
    }

    #[test]
    fn no_directories_with_ellipsis() {
        let path = Path::new("README.md");
        // overhead = 0 + 1 + 3 + 6 = 10, avail = 13 - 10 = 3 → start=2, end=1 → "RE…E"
        assert_eq!(
            format_file_entry(path, " (2/3)", 13),
            "RE\u{2026}E.md (2/3)"
        );
    }

    #[test]
    fn no_extension() {
        let path = Path::new("src/Makefile");
        assert_eq!(format_file_entry(path, " (1/1)", 17), "sr/Makefile (1/1)");
    }

    #[test]
    fn no_extension_with_ellipsis() {
        let path = Path::new("src/Makefile");
        // dir_prefix="s/"(2), stem="Makefile"(8), ext=""(0), suffix=" (1/1)"(6)
        // overhead = 2 + 1 + 0 + 6 = 9, avail = 13 - 9 = 4 → start=2, end=2 → "Ma…le"
        assert_eq!(
            format_file_entry(path, " (1/1)", 13),
            "s/Ma\u{2026}le (1/1)"
        );
    }

    #[test]
    fn short_stem_skips_ellipsis() {
        let path = Path::new("src/a.rs");
        // stem="a" (1 char, <= 3), ellipsis skipped
        // compact: "s/a.rs (1/1)" = 12, truncate 2
        assert_eq!(format_file_entry(path, " (1/1)", 10), "a.rs (1/1)");
    }

    #[test]
    fn short_dirs_not_over_abbreviated() {
        let path = Path::new("a/b/MyFile.rs");
        assert_eq!(format_file_entry(path, " (1/1)", 25), "a/b/MyFile.rs (1/1)");
    }

    #[test]
    fn very_narrow_truncation() {
        let path = Path::new("src/components/MyFile.rs");
        assert_eq!(format_file_entry(path, " (1/2)", 5), "(1/2)");
    }

    #[test]
    fn empty_path_returns_suffix() {
        let path = Path::new("");
        assert_eq!(format_file_entry(path, " (0/0)", 20), " (0/0)");
    }

    #[test]
    fn dotfile_treated_as_extensionless() {
        let path = Path::new(".gitignore");
        assert_eq!(format_file_entry(path, " (1/1)", 20), ".gitignore (1/1)");
    }
}
