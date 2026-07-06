//! Cascading fuzzy match for the edit_file tool.
//! Attempts exact, whitespace-normalized, indent-flexible, and line-fuzzy
//! matching in order.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchResult {
    pub level: u8,
    pub start_index: usize,
    pub matched_text: String,
}

pub fn cascading_match(content: &str, search: &str) -> Option<MatchResult> {
    if let Some(idx) = content.find(search) {
        return Some(MatchResult {
            level: 0,
            start_index: idx,
            matched_text: search.to_string(),
        });
    }
    if let Some(r) = match_whitespace_normalized(content, search) {
        return Some(r);
    }
    if let Some(r) = match_indent_flexible(content, search) {
        return Some(r);
    }
    match_line_fuzzy(content, search)
}

fn normalize_line(line: &str) -> String {
    let leading: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let rest = &line[leading.len()..];
    let mut collapsed = String::with_capacity(rest.len());
    let mut prev_ws = false;
    for c in rest.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                collapsed.push(' ');
            }
            prev_ws = true;
        } else {
            collapsed.push(c);
            prev_ws = false;
        }
    }
    let mut s = format!("{leading}{}", collapsed.trim_end());
    // The trim above kills trailing spaces inside collapsed; that's intended.
    if s.ends_with(' ') {
        s = s.trim_end().to_string();
    }
    s
}

fn sum_line_lengths(lines: &[&str], end: usize) -> usize {
    let mut total = 0;
    for line in lines.iter().take(end) {
        total += line.len() + 1;
    }
    total
}

fn match_line_block<F>(
    content_lines: &[&str],
    search_lines: &[&str],
    level: u8,
    compare: F,
) -> Option<MatchResult>
where
    F: Fn(&str, &str) -> bool,
{
    if search_lines.is_empty() {
        return None;
    }
    if content_lines.len() < search_lines.len() {
        return None;
    }
    for i in 0..=content_lines.len() - search_lines.len() {
        let mut matches = true;
        for j in 0..search_lines.len() {
            if !compare(content_lines[i + j], search_lines[j]) {
                matches = false;
                break;
            }
        }
        if matches {
            let matched: Vec<&str> = content_lines[i..i + search_lines.len()].to_vec();
            let matched_text = matched.join("\n");
            let start_index = sum_line_lengths(content_lines, i);
            return Some(MatchResult {
                level,
                start_index,
                matched_text,
            });
        }
    }
    None
}

fn match_indent_flexible(content: &str, search: &str) -> Option<MatchResult> {
    let content_lines: Vec<&str> = content.split('\n').collect();
    let search_lines: Vec<&str> = search.split('\n').collect();
    match_line_block(&content_lines, &search_lines, 2, |c, s| {
        c.trim_start() == s.trim_start()
    })
}

fn match_line_fuzzy(content: &str, search: &str) -> Option<MatchResult> {
    let content_lines: Vec<&str> = content.split('\n').collect();
    let search_lines: Vec<&str> = search.split('\n').collect();
    let strip = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).collect() };
    match_line_block(&content_lines, &search_lines, 3, |c, s| {
        strip(c) == strip(s)
    })
}

fn match_whitespace_normalized(content: &str, search: &str) -> Option<MatchResult> {
    let content_lines: Vec<&str> = content.split('\n').collect();
    let search_lines: Vec<&str> = search.split('\n').collect();
    let norm_content_lines: Vec<String> = content_lines.iter().map(|l| normalize_line(l)).collect();
    let norm_search_lines: Vec<String> = search_lines.iter().map(|l| normalize_line(l)).collect();
    let norm_content = norm_content_lines.join("\n");
    let norm_search = norm_search_lines.join("\n");

    let norm_idx = norm_content.find(&norm_search)?;
    let match_start = norm_idx;
    let match_end = norm_idx + norm_search.len();

    // Map normalized positions back to original byte offsets.
    let mut pos = 0;
    let mut start_line: Option<usize> = None;
    let mut start_char_in_line = 0;
    let mut end_line: Option<usize> = None;
    let mut end_char_in_line = 0;

    for (i, norm_line) in norm_content_lines.iter().enumerate() {
        let line_end = pos + norm_line.len();
        if start_line.is_none() && match_start >= pos && match_start <= line_end {
            start_line = Some(i);
            start_char_in_line = match_start - pos;
        }
        if match_end >= pos && match_end <= line_end {
            end_line = Some(i);
            end_char_in_line = match_end - pos;
            break;
        }
        pos = line_end + 1;
    }

    let start_line = start_line?;
    let end_line = end_line?;
    let orig_start_line = content_lines[start_line];
    let norm_start_line = norm_content_lines[start_line].as_str();
    let orig_end_line = content_lines[end_line];
    let norm_end_line = norm_content_lines[end_line].as_str();

    let orig_start = map_char_in_line(orig_start_line, norm_start_line, start_char_in_line, false);
    let orig_end = map_char_in_line(orig_end_line, norm_end_line, end_char_in_line, true);

    let abs_start = sum_line_lengths(&content_lines, start_line) + orig_start;
    let abs_end = sum_line_lengths(&content_lines, end_line) + orig_end;
    let matched_text = content[abs_start..abs_end].to_string();
    Some(MatchResult {
        level: 1,
        start_index: abs_start,
        matched_text,
    })
}

fn map_char_in_line(
    orig_line: &str,
    norm_line: &str,
    norm_char_pos: usize,
    end_mode: bool,
) -> usize {
    if end_mode && norm_char_pos >= norm_line.len() {
        return orig_line.len();
    }
    if norm_char_pos == 0 {
        return 0;
    }
    let leading: String = orig_line
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    if norm_char_pos <= leading.len() {
        return norm_char_pos;
    }
    let orig_bytes = orig_line.as_bytes();
    let norm_bytes = norm_line.as_bytes();
    let mut orig_pos = leading.len();
    let mut norm_pos = leading.len();
    while norm_pos < norm_char_pos && orig_pos < orig_bytes.len() {
        let orig_char = orig_bytes[orig_pos] as char;
        let norm_char = norm_bytes.get(norm_pos).copied().unwrap_or(b'?') as char;
        if norm_char == ' ' && orig_char.is_whitespace() {
            norm_pos += 1;
            orig_pos += 1;
            while orig_pos < orig_bytes.len() && (orig_bytes[orig_pos] as char).is_whitespace() {
                orig_pos += 1;
            }
        } else {
            norm_pos += 1;
            orig_pos += 1;
        }
    }
    orig_pos
}

#[derive(Debug, Clone)]
pub struct ClosestMatch {
    pub line: usize,
    pub snippet: String,
}

pub fn find_closest_match(content: &str, search: &str) -> Option<ClosestMatch> {
    let content_lines: Vec<&str> = content.split('\n').collect();
    let search_lines: Vec<&str> = search.split('\n').collect();
    let first_search = search_lines.first().map(|s| s.trim().to_lowercase())?;
    if first_search.is_empty() {
        return None;
    }
    let words: Vec<&str> = first_search.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let mut best_line: i64 = -1;
    let mut best_score = 0usize;
    for (i, line) in content_lines.iter().enumerate() {
        let line_lower = line.trim().to_lowercase();
        if line_lower.is_empty() {
            continue;
        }
        let mut score = 0;
        for w in &words {
            if line_lower.contains(w) {
                score += w.len();
            }
        }
        if words.len() == 1 {
            let lcs = longest_common_substring(&line_lower, &first_search);
            score = score.max(lcs);
        }
        if score > best_score {
            best_score = score;
            best_line = i as i64;
        }
    }
    if best_line < 0 || best_score < 3 {
        return None;
    }
    let bl = best_line as usize;
    let start = bl.saturating_sub(1);
    let end = (bl + 2).min(content_lines.len());
    let snippet = content_lines[start..end].join("\n");
    Some(ClosestMatch {
        line: bl + 1,
        snippet,
    })
}

fn longest_common_substring(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut best = 0;
    for i in 0..a.len() {
        for j in 0..b.len() {
            let mut len = 0;
            while i + len < a.len() && j + len < b.len() && a[i + len] == b[j + len] {
                len += 1;
            }
            if len > best {
                best = len;
            }
        }
    }
    best
}
