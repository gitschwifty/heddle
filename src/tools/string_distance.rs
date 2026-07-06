//! Levenshtein distance and closest-name helper for tool suggestions.

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

pub fn find_closest<'a>(
    query: &str,
    candidates: &'a [String],
    max_distance: usize,
) -> Option<&'a str> {
    let mut best: Option<&str> = None;
    let mut best_dist = max_distance + 1;
    for c in candidates {
        let d = levenshtein(query, c);
        if d < best_dist {
            best_dist = d;
            best = Some(c.as_str());
        }
    }
    if best_dist <= max_distance {
        best
    } else {
        None
    }
}
