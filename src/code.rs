use rand::{Rng, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;

const WORD_COUNT: usize = 3;
const INITIALS: [&str; 16] = [
    "b", "c", "d", "f", "g", "h", "j", "k", "l", "m", "n", "p", "r", "s", "t", "v",
];
const VOWELS: [&str; 4] = ["a", "e", "i", "o"];
const ENDINGS: [&str; 4] = ["dar", "len", "mor", "tun"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ShortCodeError {
    #[error("short code must have one slot and exactly three words")]
    InvalidTokenCount,
    #[error("slot must be a decimal number that fits in u16")]
    InvalidSlot,
    #[error("invalid code word '{0}'")]
    InvalidWord(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShortCode {
    slot: u16,
    words: [u8; WORD_COUNT],
}

impl ShortCode {
    pub fn new(slot: u16, words: [u8; WORD_COUNT]) -> Self {
        Self { slot, words }
    }

    pub fn generate() -> Self {
        let mut rng = OsRng;
        Self::generate_with_rng(&mut rng)
    }

    pub fn generate_with_rng(rng: &mut impl Rng) -> Self {
        Self {
            slot: rng.gen_range(100..10_000),
            words: [
                rng.gen_range(0..=u8::MAX),
                rng.gen_range(0..=u8::MAX),
                rng.gen_range(0..=u8::MAX),
            ],
        }
    }

    pub fn slot(&self) -> u16 {
        self.slot
    }

    pub fn words(&self) -> [String; WORD_COUNT] {
        self.words.map(encode_word)
    }

    pub fn secret_phrase(&self) -> String {
        self.words().join("-")
    }

    pub fn pairing_identity(&self) -> String {
        format!("altair-vega/pairing/v1/slot/{}", self.slot)
    }

    pub fn normalized(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for ShortCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [first, second, third] = self.words();
        write!(f, "{}-{first}-{second}-{third}", self.slot)
    }
}

impl FromStr for ShortCode {
    type Err = ShortCodeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let tokens = tokenize(value);
        if tokens.len() != WORD_COUNT + 1 {
            return Err(ShortCodeError::InvalidTokenCount);
        }

        let slot = tokens[0]
            .parse::<u16>()
            .map_err(|_| ShortCodeError::InvalidSlot)?;
        let mut words = [0u8; WORD_COUNT];
        for (index, token) in tokens[1..].iter().enumerate() {
            words[index] =
                decode_word(token).ok_or_else(|| ShortCodeError::InvalidWord(token.to_string()))?;
        }

        Ok(Self { slot, words })
    }
}

fn tokenize(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn encode_word(index: u8) -> String {
    let initial = INITIALS[((index >> 4) & 0x0f) as usize];
    let vowel = VOWELS[((index >> 2) & 0x03) as usize];
    let ending = ENDINGS[(index & 0x03) as usize];
    format!("{initial}{vowel}{ending}")
}

fn decode_word(word: &str) -> Option<u8> {
    let mut chars = word.chars();
    let initial = match chars.next()? {
        'b' => 0,
        'c' => 1,
        'd' => 2,
        'f' => 3,
        'g' => 4,
        'h' => 5,
        'j' => 6,
        'k' => 7,
        'l' => 8,
        'm' => 9,
        'n' => 10,
        'p' => 11,
        'r' => 12,
        's' => 13,
        't' => 14,
        'v' => 15,
        _ => return None,
    };

    let vowel = match chars.next()? {
        'a' => 0,
        'e' => 1,
        'i' => 2,
        'o' => 3,
        _ => return None,
    };

    let ending = match chars.as_str() {
        "dar" => 0,
        "len" => 1,
        "mor" => 2,
        "tun" => 3,
        _ => return None,
    };

    Some((initial << 4) | (vowel << 2) | ending)
}

#[cfg(test)]
mod tests {
    use super::{ShortCode, ShortCodeError};
    use std::str::FromStr;

    #[test]
    fn round_trips_canonical_code() {
        let code = ShortCode::from_str("2048-badar-celen-votun").unwrap();
        assert_eq!(code.slot(), 2048);
        assert_eq!(code.to_string(), "2048-badar-celen-votun");
        assert_eq!(code.secret_phrase(), "badar-celen-votun");
        assert_eq!(code.pairing_identity(), "altair-vega/pairing/v1/slot/2048");
    }

    #[test]
    fn normalizes_case_and_separator_noise() {
        let code = ShortCode::from_str("2048 BADAR, celen_votun").unwrap();
        assert_eq!(code.normalized(), "2048-badar-celen-votun");
    }

    #[test]
    fn rejects_invalid_token_count() {
        let error = ShortCode::from_str("2048-badar-celen").unwrap_err();
        assert_eq!(error, ShortCodeError::InvalidTokenCount);
    }

    #[test]
    fn rejects_unknown_words() {
        let error = ShortCode::from_str("2048-badar-banana-votun").unwrap_err();
        assert_eq!(error, ShortCodeError::InvalidWord("banana".to_string()));
    }

    #[test]
    fn generates_parseable_codes() {
        let code = ShortCode::generate();
        let parsed = ShortCode::from_str(&code.to_string()).unwrap();
        assert_eq!(parsed, code);
    }
}
