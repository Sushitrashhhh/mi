/// Detects a wake phrase (e.g. "hey mi") inside a short whisper transcript.
///
/// We reuse the already-loaded whisper model rather than a dedicated wake model,
/// so this is a fuzzy text match: normalize the transcript, then score it against
/// the phrase and a small set of common mishears ("hey me", "hemi", ...).
pub struct WakeDetector {
    /// The phrase split into normalized word tokens, e.g. ["hey", "mi"].
    tokens: Vec<String>,
    /// The phrase joined with no spaces, e.g. "heymi" (catches "hemi").
    joined: String,
    threshold: f32,
}

impl WakeDetector {
    pub fn new(phrase: &str, threshold: f32) -> Self {
        let tokens: Vec<String> = normalize(phrase)
            .split(' ')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        let joined = tokens.concat();
        Self {
            tokens,
            joined,
            threshold,
        }
    }

    /// 0.0–1.0 confidence that the transcript contains the wake phrase.
    ///
    /// Matching is word-structured (not raw character overlap): it slides a
    /// window of the phrase's word-length over the transcript and averages the
    /// per-word similarity. This keeps unrelated speech well below threshold.
    pub fn score(&self, transcript: &str) -> f32 {
        if self.tokens.is_empty() {
            return 0.0;
        }
        let words: Vec<String> = normalize(transcript)
            .split(' ')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if words.is_empty() {
            return 0.0;
        }

        let mut best = 0.0f32;

        // A single token that whisper merged into one word (e.g. "heymi"/"hemi").
        if !self.joined.is_empty() {
            for w in &words {
                best = best.max(similarity(w, &self.joined));
            }
        }

        // Multi-word phrase: average per-word similarity over each window.
        let n = self.tokens.len();
        if words.len() >= n {
            for window in words.windows(n) {
                let sum: f32 = window
                    .iter()
                    .zip(&self.tokens)
                    .map(|(w, t)| similarity(w, t))
                    .sum();
                best = best.max(sum / n as f32);
            }
        }

        best
    }

    pub fn is_wake(&self, transcript: &str) -> bool {
        self.score(transcript) >= self.threshold
    }
}

fn normalize(s: &str) -> String {
    let mut out = String::new();
    let mut prev_space = true; // trim leading space
    for c in s.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim_end().to_string()
}

/// Normalized Levenshtein similarity in 0.0–1.0.
fn similarity(a: &str, b: &str) -> f32 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let dist = levenshtein(&a, &b);
    let max_len = a.len().max(b.len());
    1.0 - (dist as f32 / max_len as f32)
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_and_variants_match() {
        let d = WakeDetector::new("hey mi", 0.6);
        assert!(d.is_wake("Hey, mi!"));
        assert!(d.is_wake("hey me"));
        assert!(d.is_wake("okay hey mi what time is it"));
    }

    #[test]
    fn test_rejects_unrelated() {
        let d = WakeDetector::new("hey mi", 0.6);
        assert!(!d.is_wake("hello there how are you"));
        assert!(!d.is_wake("the weather is nice today"));
        assert!(!d.is_wake(""));
    }

    #[test]
    fn test_score_monotonic() {
        let d = WakeDetector::new("hey mi", 0.6);
        assert!(d.score("hey mi") > d.score("hey there"));
    }
}
