//! Linear-time suffix array (SA-IS) + LCP (Kasai), over an integer alphabet.
//!
//! This is the engine behind exact, maximal token-clone detection: concatenate
//! every file's normalized token stream (separated by unique sentinels so no
//! match can cross a file boundary), build the suffix array in O(n) via SA-IS
//! (Nong, Zhang & Chan 2009, *Linear Suffix Array Construction by Almost Pure
//! Induced-Sorting*), then derive the LCP array in O(n) via Kasai et al. Runs
//! of LCP ≥ threshold are exact maximal repeats — the clone classes.
//!
//! The input must be a sequence of symbols in `0..alphabet_size` whose **last
//! element is `0` and `0` appears nowhere else** (the unique smallest
//! sentinel). `dupes` guarantees this.
//!
// Index-based loops are intrinsic to induced sorting and Kasai's LCP; the
// iterator rewrites clippy suggests would obscure the algorithm.
#![allow(clippy::needless_range_loop)]

/// Build the suffix array of `s` using SA-IS.
///
/// `alphabet_size` is the exclusive upper bound on symbol values (i.e. symbols
/// are in `0..alphabet_size`). `s` must be non-empty, end in `0`, and contain
/// `0` exactly once (at the end).
pub fn suffix_array(s: &[u32], alphabet_size: usize) -> Vec<u32> {
    let n = s.len();
    let mut sa = vec![0u32; n];
    if n <= 1 {
        return sa;
    }
    let s_usize: Vec<usize> = s.iter().map(|&x| x as usize).collect();
    let mut sa_usize = vec![0usize; n];
    sais(&s_usize, &mut sa_usize, alphabet_size);
    for (dst, &v) in sa.iter_mut().zip(sa_usize.iter()) {
        *dst = v as u32;
    }
    sa
}

#[inline]
fn is_lms(t: &[bool], i: usize) -> bool {
    i > 0 && t[i] && !t[i - 1]
}

/// Whether the two LMS substrings starting at `a` and `b` are identical
/// (same characters and same L/S types, same length). The sentinel-only LMS
/// at `n-1` is unique.
fn lms_substrings_equal(s: &[usize], t: &[bool], a: usize, b: usize) -> bool {
    let n = s.len();
    if a == n - 1 || b == n - 1 {
        return a == b;
    }
    let mut i = 0;
    loop {
        let ai = a + i;
        let bi = b + i;
        if ai >= n || bi >= n {
            return false;
        }
        let a_lms = is_lms(t, ai);
        let b_lms = is_lms(t, bi);
        if i > 0 && a_lms && b_lms {
            return true; // both reached their next LMS at the same offset
        }
        if a_lms != b_lms {
            return false; // one ended before the other
        }
        if s[ai] != s[bi] || t[ai] != t[bi] {
            return false;
        }
        i += 1;
    }
}

/// Bucket head/tail offsets. `tail=true` → one-past-the-end of each bucket.
fn buckets(sizes: &[usize], tail: bool) -> Vec<usize> {
    let mut b = vec![0usize; sizes.len()];
    let mut sum = 0usize;
    for (i, &sz) in sizes.iter().enumerate() {
        sum += sz;
        b[i] = if tail { sum } else { sum - sz };
    }
    b
}

fn bucket_sizes(s: &[usize], k: usize) -> Vec<usize> {
    let mut sizes = vec![0usize; k];
    for &c in s {
        sizes[c] += 1;
    }
    sizes
}

/// Induce L-type then S-type suffixes from already-placed LMS suffixes.
fn induce(s: &[usize], sa: &mut [usize], t: &[bool], sizes: &[usize], k: usize) {
    let n = s.len();
    let none = usize::MAX;
    // L-type: left→right, bucket heads.
    let mut head = buckets(sizes, false);
    for i in 0..n {
        let j = sa[i];
        if j != none && j > 0 && !t[j - 1] {
            let c = s[j - 1];
            sa[head[c]] = j - 1;
            head[c] += 1;
        }
    }
    let _ = k;
    // S-type: right→left, bucket tails.
    let mut tail = buckets(sizes, true);
    for i in (0..n).rev() {
        let j = sa[i];
        if j != none && j > 0 && t[j - 1] {
            let c = s[j - 1];
            tail[c] -= 1;
            sa[tail[c]] = j - 1;
        }
    }
}

fn sais(s: &[usize], sa: &mut [usize], k: usize) {
    let n = s.len();
    let none = usize::MAX;
    if n == 1 {
        sa[0] = 0;
        return;
    }
    // Type map: true = S-type, false = L-type.
    let mut t = vec![false; n];
    t[n - 1] = true;
    for i in (0..n - 1).rev() {
        t[i] = s[i] < s[i + 1] || (s[i] == s[i + 1] && t[i + 1]);
    }

    let sizes = bucket_sizes(s, k);

    // --- Step 1: place LMS suffixes at bucket tails, then induce. ---
    for v in sa.iter_mut() {
        *v = none;
    }
    let mut tail = buckets(&sizes, true);
    for i in 1..n {
        if is_lms(&t, i) {
            let c = s[i];
            tail[c] -= 1;
            sa[tail[c]] = i;
        }
    }
    induce(s, sa, &t, &sizes, k);

    // --- Step 2: collect sorted LMS positions, name them. ---
    let mut lms_sorted: Vec<usize> = Vec::new();
    for i in 0..n {
        let j = sa[i];
        if j != none && is_lms(&t, j) {
            lms_sorted.push(j);
        }
    }
    let mut names = vec![none; n];
    let mut name = 0usize;
    names[lms_sorted[0]] = 0;
    let mut prev = lms_sorted[0];
    for &cur in lms_sorted.iter().skip(1) {
        if !lms_substrings_equal(s, &t, prev, cur) {
            name += 1;
        }
        names[cur] = name;
        prev = cur;
    }
    let num_names = name + 1;

    // Reduced string in text order of LMS positions.
    let mut lms_positions: Vec<usize> = (1..n).filter(|&i| is_lms(&t, i)).collect();
    lms_positions.sort_unstable();
    let reduced: Vec<usize> = lms_positions.iter().map(|&p| names[p]).collect();

    // --- Step 3: recurse (or base case) to sort the reduced string. ---
    let mut reduced_sa = vec![0usize; reduced.len()];
    if num_names == reduced.len() {
        // All names unique → SA is the inverse permutation.
        for (i, &nm) in reduced.iter().enumerate() {
            reduced_sa[nm] = i;
        }
    } else {
        sais(&reduced, &mut reduced_sa, num_names);
    }

    // --- Step 4: place LMS suffixes in true sorted order, induce final SA. ---
    for v in sa.iter_mut() {
        *v = none;
    }
    let mut tail = buckets(&sizes, true);
    // Walk reduced_sa to get LMS positions in sorted order, place at bucket tails.
    for idx in (0..reduced_sa.len()).rev() {
        let p = lms_positions[reduced_sa[idx]];
        let c = s[p];
        tail[c] -= 1;
        sa[tail[c]] = p;
    }
    induce(s, sa, &t, &sizes, k);
}

/// LCP array via Kasai et al.: `lcp[i]` is the longest common prefix length of
/// the suffixes at `sa[i-1]` and `sa[i]`; `lcp[0] == 0`.
pub fn lcp_kasai(s: &[u32], sa: &[u32]) -> Vec<u32> {
    let n = s.len();
    let mut lcp = vec![0u32; n];
    if n == 0 {
        return lcp;
    }
    let mut rank = vec![0usize; n];
    for (i, &p) in sa.iter().enumerate() {
        rank[p as usize] = i;
    }
    let mut h = 0usize;
    for i in 0..n {
        if rank[i] > 0 {
            let j = sa[rank[i] - 1] as usize;
            while i + h < n && j + h < n && s[i + h] == s[j + h] {
                h += 1;
            }
            lcp[rank[i]] = h as u32;
            h = h.saturating_sub(1);
        } else {
            h = 0;
        }
    }
    lcp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Naive O(n² log n) suffix array for cross-checking.
    fn naive_sa(s: &[u32]) -> Vec<u32> {
        let n = s.len();
        let mut idx: Vec<u32> = (0..n as u32).collect();
        idx.sort_by(|&a, &b| s[a as usize..].cmp(&s[b as usize..]));
        idx
    }

    fn naive_lcp(s: &[u32], sa: &[u32]) -> Vec<u32> {
        let mut lcp = vec![0u32; sa.len()];
        for i in 1..sa.len() {
            let a = &s[sa[i - 1] as usize..];
            let b = &s[sa[i] as usize..];
            let mut k = 0;
            while k < a.len() && k < b.len() && a[k] == b[k] {
                k += 1;
            }
            lcp[i] = k as u32;
        }
        lcp
    }

    /// Build a valid SA-IS input: symbols in 1..=max, terminated by a unique 0.
    fn with_sentinel(body: &[u32]) -> (Vec<u32>, usize) {
        let mut s: Vec<u32> = body.iter().map(|&x| x + 1).collect();
        s.push(0);
        let k = s.iter().copied().max().unwrap() as usize + 1;
        (s, k)
    }

    #[test]
    fn matches_naive_on_small_strings() {
        let cases: &[&[u32]] = &[
            &[],
            &[0],
            &[1, 1, 1, 1],
            &[3, 1, 2, 1, 2, 3],
            &[1, 2, 1, 2, 1, 2, 1],
            &[5, 4, 3, 2, 1],
            &[1, 2, 3, 4, 5],
        ];
        for c in cases {
            let (s, k) = with_sentinel(c);
            let sa = suffix_array(&s, k);
            assert_eq!(sa, naive_sa(&s), "SA mismatch on {c:?}");
            assert_eq!(
                lcp_kasai(&s, &sa),
                naive_lcp(&s, &sa),
                "LCP mismatch on {c:?}"
            );
        }
    }

    #[test]
    fn matches_naive_on_random_strings() {
        // Deterministic LCG so the test is reproducible.
        let mut state: u64 = 0x9E3779B97F4A7C15;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u32
        };
        for _ in 0..400 {
            let len = (next() % 60) as usize + 1;
            let alpha = (next() % 4) + 1; // small alphabet → many repeats
            let body: Vec<u32> = (0..len).map(|_| next() % alpha).collect();
            let (s, k) = with_sentinel(&body);
            let sa = suffix_array(&s, k);
            assert_eq!(sa, naive_sa(&s), "SA mismatch on {body:?}");
            assert_eq!(
                lcp_kasai(&s, &sa),
                naive_lcp(&s, &sa),
                "LCP mismatch on {body:?}"
            );
        }
    }

    #[test]
    fn finds_repeat_via_lcp() {
        // "abcabc" → the repeat "abc" (len 3) shows up as an LCP ≥ 3.
        let (s, k) = with_sentinel(&[1, 2, 3, 1, 2, 3]);
        let sa = suffix_array(&s, k);
        let lcp = lcp_kasai(&s, &sa);
        assert!(
            lcp.iter().any(|&l| l >= 3),
            "expected an LCP ≥ 3, got {lcp:?}"
        );
    }
}
