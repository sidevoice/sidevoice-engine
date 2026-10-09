//! How far a transcript is from what was said: the word error rate of the two texts, normalised.

/// `text` as it is compared: lower case, letters and digits only, the accents of vowels (and of `ç`) folded, single
/// spaces. `ñ` stays itself: it is a letter of its own, not an accent.
pub(crate) fn normalised(text: &str) -> String {
    let folded: String = text
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The word error rate of `heard` against `said`: words substituted, deleted and inserted (the fewest that turn one
/// into the other), over the words said, both texts [`normalised`]. 0 is a perfect transcript; it can exceed 1.
pub(crate) fn wer(said: &str, heard: &str) -> f64 {
    let (said, heard) = (normalised(said), normalised(heard));
    let said: Vec<&str> = said.split(' ').filter(|word| !word.is_empty()).collect();
    let heard: Vec<&str> = heard.split(' ').filter(|word| !word.is_empty()).collect();
    if said.is_empty() {
        return if heard.is_empty() { 0.0 } else { 1.0 };
    }
    // Levenshtein over words, one row at a time.
    let mut row: Vec<usize> = (0..=heard.len()).collect();
    for (i, said_word) in said.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, heard_word) in heard.iter().enumerate() {
            let substitution = diagonal + usize::from(said_word != heard_word);
            diagonal = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[heard.len()] as f64 / said.len() as f64
}
