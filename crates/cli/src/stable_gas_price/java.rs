//! What rskj's price sources read an answer with, as rskj's Java reads it:
//! Jackson 2.15's `ObjectMapper.readTree`, `JsonPointer` and `JsonNode.asLong`,
//! with Jackson's defaults as rskj leaves them; and Java's
//! `Double.parseDouble`, by which `asLong` reads a string and HOCON reads a
//! duration's number.

/// How deep Jackson 2.15 reads a document (`StreamReadConstraints`).
const MAX_NESTING_DEPTH: usize = 1000;

/// How many digits Jackson 2.15 reads in a number (`StreamReadConstraints`).
const MAX_NUMBER_LENGTH: usize = 1000;

const END: &str = "Unexpected end-of-input";

/// Jackson `JsonPointer`: the names and indexes a node is found by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JsonPointer(Vec<String>);

impl JsonPointer {
    /// Jackson `JsonPointer.compile`, which rskj's `valueOf` is: empty, the
    /// whole document; otherwise `/` and a segment after each `/`, in which
    /// `~0` is `~`, `~1` is `/`, and a `~` before anything else stays, with
    /// the character after it.
    pub(super) fn compile(expr: &str) -> Result<Self, String> {
        if expr.is_empty() {
            return Ok(Self(Vec::new()));
        }
        let Some(tail) = expr.strip_prefix('/') else {
            return Err(format!(
                "Invalid input: JSON Pointer expression must start with '/': {expr:?}"
            ));
        };
        let mut segments = Vec::new();
        let mut segment = String::new();
        let mut chars = tail.chars();
        while let Some(c) = chars.next() {
            match c {
                '/' => segments.push(std::mem::take(&mut segment)),
                '~' => match chars.next() {
                    Some('0') => segment.push('~'),
                    Some('1') => segment.push('/'),
                    Some(other) => {
                        segment.push('~');
                        segment.push(other);
                    }
                    None => segment.push('~'),
                },
                c => segment.push(c),
            }
        }
        segments.push(segment);
        Ok(Self(segments))
    }
}

/// Jackson's tree of a document, as far as `at` and `asLong` look into it.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Node {
    /// The members in order. A name holding half a surrogate pair is `None`:
    /// no pointer the configuration can give names it.
    Object(Vec<(Option<String>, Node)>),
    Array(Vec<Node>),
    /// A string. Half a surrogate pair reads as U+FFFD, which `parseAsLong`
    /// reads as no number, as it does the half pair.
    Text(String),
    /// A number with neither a fraction nor an exponent: the low 64 bits
    /// `asLong` takes of Jackson's `int`, `long` or `BigInteger`.
    Integer(i64),
    /// A number with either, read into a double.
    Double(f64),
    Bool(bool),
    Null,
}

impl Node {
    /// Jackson `JsonNode.at`: the node the pointer names, or `None` for
    /// Jackson's `MissingNode`. A name given twice names the last, as
    /// Jackson's `ObjectNode` keeps it.
    pub(super) fn at(&self, pointer: &JsonPointer) -> Option<&Node> {
        pointer.0.iter().try_fold(self, |node, segment| match node {
            Node::Object(members) => members
                .iter()
                .rev()
                .find(|(name, _)| name.as_deref() == Some(segment.as_str()))
                .map(|(_, value)| value),
            Node::Array(elements) => array_index(segment).and_then(|i| elements.get(i)),
            _ => None,
        })
    }

    /// Jackson `JsonNode.asLong()`.
    pub(super) fn as_long(&self) -> i64 {
        match self {
            Node::Integer(n) => *n,
            // `DoubleNode.longValue`: Java's cast, which saturates and takes
            // NaN to 0, as Rust's does.
            Node::Double(d) => *d as i64,
            Node::Text(text) => parse_as_long(text),
            Node::Bool(b) => i64::from(*b),
            Node::Object(_) | Node::Array(_) | Node::Null => 0,
        }
    }
}

/// Jackson `JsonPointer._parseIndex`: an `int` written without a sign or a
/// leading zero names an array's element; any other segment names none.
fn array_index(segment: &str) -> Option<usize> {
    let digits = segment.as_bytes();
    if digits.is_empty() || digits.len() > 10 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if digits.len() > 1 && digits[0] == b'0' {
        return None;
    }
    segment
        .parse::<i32>()
        .ok()
        .and_then(|i| usize::try_from(i).ok())
}

/// An object or an array being read, with what it holds so far, and an
/// object with the name of the member being read.
enum Open {
    Object(Vec<(Option<String>, Node)>, Option<String>),
    Array(Vec<Node>),
}

/// Jackson `ObjectMapper.readTree(String)`: the document's first value, with
/// anything after it left unread; `None` for whitespace alone, Jackson's
/// `MissingNode`. Read as Jackson's parser reads it by default, which
/// refuses comments, single quotes, names without quotes, trailing commas,
/// `NaN`, a leading zero or plus, a control character unescaped and an
/// escape it doesn't know, and a document past its depth or a number past
/// its digits. A number at the top must end the document or be followed by
/// whitespace, and a word anywhere must not run on into what a Java name
/// goes on with.
pub(super) fn read_tree(text: &str) -> Result<Option<Node>, String> {
    let mut reader = Reader { text, at: 0 };
    if !reader.skip_whitespace()? {
        return Ok(None);
    }
    // Iterative, not recursive: a document a thousand deep is read on the
    // price's own thread, whatever its stack.
    let mut open: Vec<Open> = Vec::new();
    loop {
        let mut node = match reader.peek() {
            Some(b'{') => {
                reader.enter(open.len())?;
                if reader.closes(b'}')? {
                    Node::Object(Vec::new())
                } else {
                    open.push(Open::Object(Vec::new(), reader.member_name()?));
                    continue;
                }
            }
            Some(b'[') => {
                reader.enter(open.len())?;
                if reader.closes(b']')? {
                    Node::Array(Vec::new())
                } else {
                    open.push(Open::Array(Vec::new()));
                    continue;
                }
            }
            _ => reader.scalar(open.is_empty())?,
        };
        // A whole value: into what holds it, and out of each container it
        // was the last of.
        loop {
            let close = match open.last_mut() {
                None => return Ok(Some(node)),
                Some(Open::Object(members, name)) => {
                    members.push((name.take(), node));
                    b'}'
                }
                Some(Open::Array(elements)) => {
                    elements.push(node);
                    b']'
                }
            };
            if !reader.skip_whitespace()? {
                return Err(END.into());
            }
            match reader.peek() {
                Some(b',') => {
                    reader.at += 1;
                    if !reader.skip_whitespace()? {
                        return Err(END.into());
                    }
                    if let Some(Open::Object(_, name)) = open.last_mut() {
                        *name = reader.member_name()?;
                    }
                    break;
                }
                Some(b) if b == close => {
                    reader.at += 1;
                    node = match open.pop() {
                        Some(Open::Object(members, _)) => Node::Object(members),
                        Some(Open::Array(elements)) => Node::Array(elements),
                        None => return Err(END.into()),
                    };
                }
                _ => {
                    return Err(format!(
                        "Unexpected character at {}: was expecting comma to separate entries",
                        reader.at
                    ))
                }
            }
        }
    }
}

struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    /// Past JSON's whitespace; whether anything is left. Any other control
    /// character is refused, as Jackson's `_skipWSOrEnd` refuses it.
    fn skip_whitespace(&mut self) -> Result<bool, String> {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => self.at += 1,
                0..=0x1f => return Err(format!("Illegal character (CTRL-CHAR, code {b})")),
                _ => return Ok(true),
            }
        }
        Ok(false)
    }

    /// Into an object or an array inside `outside` others, as deep as
    /// Jackson goes.
    fn enter(&mut self, outside: usize) -> Result<(), String> {
        let depth = outside + 1;
        if depth > MAX_NESTING_DEPTH {
            return Err(format!(
                "Document nesting depth ({depth}) exceeds the maximum allowed ({MAX_NESTING_DEPTH})"
            ));
        }
        self.at += 1;
        Ok(())
    }

    /// Whether the container just entered closes at once, with `close`.
    fn closes(&mut self, close: u8) -> Result<bool, String> {
        if !self.skip_whitespace()? {
            return Err(END.into());
        }
        let closes = self.peek() == Some(close);
        if closes {
            self.at += 1;
        }
        Ok(closes)
    }

    /// A member's name and its colon, up to its value.
    fn member_name(&mut self) -> Result<Option<String>, String> {
        if self.peek() != Some(b'"') {
            return Err(format!(
                "Unexpected character at {}: was expecting double-quote to start field name",
                self.at
            ));
        }
        self.at += 1;
        let name = String::from_utf16(&self.string()?).ok();
        if !self.skip_whitespace()? {
            return Err(END.into());
        }
        if self.peek() != Some(b':') {
            return Err(format!(
                "Unexpected character at {}: was expecting a colon to separate field name and value",
                self.at
            ));
        }
        self.at += 1;
        if !self.skip_whitespace()? {
            return Err(END.into());
        }
        Ok(name)
    }

    /// A string, a number or a word; `top` for the document's own value.
    fn scalar(&mut self, top: bool) -> Result<Node, String> {
        match self.peek() {
            Some(b'"') => {
                self.at += 1;
                Ok(Node::Text(String::from_utf16_lossy(&self.string()?)))
            }
            Some(b'-' | b'0'..=b'9') => self.number(top),
            Some(b't') => self.word("true", Node::Bool(true)),
            Some(b'f') => self.word("false", Node::Bool(false)),
            Some(b'n') => self.word("null", Node::Null),
            Some(_) => Err(format!(
                "Unexpected character at {}: expected a valid value",
                self.at
            )),
            None => Err(END.into()),
        }
    }

    /// A string's UTF-16 units, from past its opening quote to past its
    /// closing one.
    fn string(&mut self) -> Result<Vec<u16>, String> {
        let mut units = Vec::new();
        loop {
            let Some(c) = self
                .text
                .get(self.at..)
                .and_then(|rest| rest.chars().next())
            else {
                return Err(format!(
                    "{END}: was expecting closing quote for a string value"
                ));
            };
            self.at += c.len_utf8();
            match c {
                '"' => return Ok(units),
                '\\' => units.push(self.escape()?),
                '\0'..='\u{1f}' => {
                    return Err(format!(
                        "Illegal unquoted character (CTRL-CHAR, code {}): has to be escaped",
                        u32::from(c)
                    ))
                }
                c => units.extend_from_slice(c.encode_utf16(&mut [0; 2])),
            }
        }
    }

    /// What a backslash in a string stands for: one of JSON's escapes, or
    /// `u` and four hex digits, half a surrogate pair included.
    fn escape(&mut self) -> Result<u16, String> {
        let Some(b) = self.peek() else {
            return Err(END.into());
        };
        let unit = match b {
            b'b' => 0x08,
            b't' => 0x09,
            b'n' => 0x0a,
            b'f' => 0x0c,
            b'r' => 0x0d,
            b'"' | b'/' | b'\\' => u16::from(b),
            b'u' => {
                let hex = self
                    .text
                    .as_bytes()
                    .get(self.at + 1..self.at + 5)
                    .ok_or(END)?;
                let mut unit = 0u16;
                for &digit in hex {
                    let value = char::from(digit).to_digit(16).ok_or(
                        "Unexpected character: expected a hex-digit for character escape sequence",
                    )?;
                    unit = unit << 4 | value as u16;
                }
                self.at += 4;
                unit
            }
            _ => return Err(format!("Unrecognized character escape (code {b})")),
        };
        self.at += 1;
        Ok(unit)
    }

    /// A number as Jackson's parser reads one: `-`, then `0` or digits
    /// without a leading zero, then a fraction and an exponent, each with a
    /// digit at least. At the top it ends the document or whitespace follows
    /// it (`_verifyRootSpace`).
    fn number(&mut self, top: bool) -> Result<Node, String> {
        let text = self.text;
        let bytes = text.as_bytes();
        let digits = |from: usize| {
            bytes.get(from..).map_or(0, |rest| {
                rest.iter().take_while(|b| b.is_ascii_digit()).count()
            })
        };
        let start = self.at;
        if bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        let whole = digits(self.at);
        if whole == 0 {
            return Err("Unexpected character: expected digit (0-9) to follow minus sign".into());
        }
        if whole > 1 && bytes.get(self.at) == Some(&b'0') {
            return Err("Invalid numeric value: Leading zeroes not allowed".into());
        }
        self.at += whole;
        let (mut fraction, mut exponent, mut integer) = (0, 0, true);
        if self.peek() == Some(b'.') {
            integer = false;
            fraction = digits(self.at + 1);
            if fraction == 0 {
                return Err("Unexpected character: Decimal point not followed by a digit".into());
            }
            self.at += 1 + fraction;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            integer = false;
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            exponent = digits(self.at);
            if exponent == 0 {
                return Err(
                    "Unexpected character: Exponent indicator not followed by a digit".into(),
                );
            }
            self.at += exponent;
        }
        if top && !matches!(self.peek(), None | Some(b' ' | b'\t' | b'\n' | b'\r')) {
            return Err("Unexpected character: Expected space separating root-level values".into());
        }
        let length = whole + fraction + exponent;
        if length > MAX_NUMBER_LENGTH {
            return Err(format!(
                "Number value length ({length}) exceeds the maximum allowed ({MAX_NUMBER_LENGTH})"
            ));
        }
        let token = &text[start..self.at];
        if integer {
            Ok(Node::Integer(low_64_bits(token)))
        } else {
            // Rust reads a double as Java's `Double.parseDouble` does: to the
            // nearest, and past the largest as infinite.
            token
                .parse::<f64>()
                .map(Node::Double)
                .map_err(|e| e.to_string())
        }
    }

    /// `true`, `false` or `null`, as Jackson's `_matchToken` takes one: a
    /// word run on into what a Java name goes on with is no word.
    fn word(&mut self, word: &str, node: Node) -> Result<Node, String> {
        let rest = self.text.get(self.at..).unwrap_or_default();
        let Some(after) = rest.strip_prefix(word) else {
            return Err(format!("Unrecognized token: was expecting '{word}'"));
        };
        if let Some(next) = after.chars().next() {
            if next >= '0' && next != ']' && next != '}' && is_java_identifier_part(next) {
                return Err(format!("Unrecognized token '{word}{next}'"));
            }
        }
        self.at += word.len();
        Ok(node)
    }
}

/// Java `Character.isJavaIdentifierPart(char)`, for the character after a
/// word: a letter or a digit, `$` or `_`, or a control Java takes as
/// ignorable, exactly through Latin-1; past it, a letter or a digit, where
/// Java's answer is that of the Unicode its version carries. A character
/// past 16 bits is half a surrogate pair to Java's `char`, which is none.
fn is_java_identifier_part(c: char) -> bool {
    match c {
        '\u{0}'..='\u{8}' | '\u{e}'..='\u{1b}' | '\u{7f}'..='\u{9f}' => true,
        '$' | '_' | '0'..='9' | 'A'..='Z' | 'a'..='z' => true,
        // The cent, pound, currency and yen signs, the soft hyphen, and
        // Latin-1's letters.
        '\u{a2}'..='\u{a5}' | '\u{aa}' | '\u{ad}' | '\u{b5}' | '\u{ba}' => true,
        '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{ff}' => true,
        '\u{0}'..='\u{ff}' | '\u{10000}'.. => false,
        _ => c.is_alphanumeric(),
    }
}

/// A whole number's low 64 bits, two's complement: `asLong` of Jackson's
/// `IntNode`, `LongNode` and `BigIntegerNode` alike.
fn low_64_bits(token: &str) -> i64 {
    let (negative, digits) = match token.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, token),
    };
    let magnitude = digits.bytes().fold(0u64, |value, digit| {
        value
            .wrapping_mul(10)
            .wrapping_add(u64::from(digit.wrapping_sub(b'0')))
    });
    (if negative {
        magnitude.wrapping_neg()
    } else {
        magnitude
    }) as i64
}

/// Jackson `NumberInput.parseAsLong(text, 0)`, which `TextNode.asLong` reads
/// by: trimmed of characters up to the space, a `+` dropped, then digits
/// read as a `long`, or 0 past one; anything else read as a Java double and
/// cast, or 0 where it is none.
pub(super) fn parse_as_long(text: &str) -> i64 {
    let s = text.trim_matches(|c: char| c <= ' ');
    if s.is_empty() {
        return 0;
    }
    let s = s.strip_prefix('+').unwrap_or(s);
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse::<i64>().unwrap_or(0);
    }
    parse_double(s).map_or(0, |d| d as i64)
}

/// Java `Double.parseDouble`: trimmed of characters up to the space, a sign,
/// then `NaN`, `Infinity`, a decimal number, or a hexadecimal one with a
/// binary exponent, either of the last two with a `d` or `f` after it if it
/// likes; `None` where Java throws `NumberFormatException`.
pub(super) fn parse_double(text: &str) -> Option<f64> {
    let s = text.trim_matches(|c: char| c <= ' ');
    let (negative, body) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let magnitude = match body {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        _ => {
            let body = body.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(body);
            match body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
                Some(hex) => hex_float(hex)?,
                None if decimal_literal(body) => body.parse::<f64>().ok()?,
                None => return None,
            }
        }
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// Digits with at most one point and at least one digit, then an optional
/// exponent: Java's decimal floating-point literal, unsigned.
fn decimal_literal(s: &str) -> bool {
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(at) => (&s[..at], Some(&s[at + 1..])),
        None => (s, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if !(digits(whole) && digits(fraction)) || (whole.is_empty() && fraction.is_empty()) {
        return false;
    }
    exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    })
}

/// What follows `0x` in a Java hexadecimal float, its `d` or `f` gone: hex
/// digits with a point or without, then `p` and a signed decimal exponent,
/// rounded to the nearest double, ties to even, as Java's
/// `FloatingDecimal.parseHexString` rounds it.
fn hex_float(s: &str) -> Option<f64> {
    let (significand, exponent) = s.split_once(['p', 'P'])?;
    let (whole, fraction) = significand.split_once('.').unwrap_or((significand, ""));
    let hex = |t: &str| t.bytes().all(|b| b.is_ascii_hexdigit());
    if (whole.is_empty() && fraction.is_empty()) || !hex(whole) || !hex(fraction) {
        return None;
    }
    let (up, exponent) = match exponent.as_bytes().first() {
        Some(b'-') => (false, &exponent[1..]),
        Some(b'+') => (true, &exponent[1..]),
        _ => (true, exponent),
    };
    if exponent.is_empty() || !exponent.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // The digits as one integer of up to 64 bits, and whether any dropped
    // past those was other than zero.
    let (mut mantissa, mut dropped, mut sticky) = (0u64, 0i64, false);
    for digit in whole.bytes().chain(fraction.bytes()) {
        let value = u64::from(char::from(digit).to_digit(16).unwrap_or(0));
        if mantissa >> 60 == 0 {
            mantissa = mantissa << 4 | value;
        } else {
            dropped += 1;
            sticky |= value != 0;
        }
    }
    if mantissa == 0 {
        return Some(0.0);
    }
    // Java reads the exponent as an `int`; past one, the number is infinite
    // or zero.
    let Ok(exponent) = exponent.parse::<i32>() else {
        return Some(if up { f64::INFINITY } else { 0.0 });
    };
    let exponent = if up {
        i64::from(exponent)
    } else {
        -i64::from(exponent)
    };
    let scale = exponent - 4 * fraction.len() as i64 + 4 * dropped;
    Some(round_to_double(mantissa, sticky, scale))
}

/// `mantissa` times two to the `scale`, with `sticky` where more was below
/// its last bit, to the nearest double, ties to even.
fn round_to_double(mantissa: u64, sticky: bool, scale: i64) -> f64 {
    let top = scale + i64::from(63 - mantissa.leading_zeros());
    if top > 1023 {
        return f64::INFINITY;
    }
    // The last bit a double keeps: 52 below its top one, or the smallest
    // subnormal's.
    let last = (top - 52).max(-1074);
    let shift = last - scale;
    if shift <= 0 {
        // Every bit is kept.
        return mantissa as f64 * power_of_two(scale);
    }
    if shift > 65 {
        // Under a quarter of the smallest subnormal.
        return 0.0;
    }
    let wide = u128::from(mantissa);
    let kept = wide >> shift;
    let rest = wide & ((1u128 << shift) - 1);
    let half = 1u128 << (shift - 1);
    let up = rest > half || (rest == half && (sticky || kept & 1 == 1));
    (kept + u128::from(up)) as f64 * power_of_two(last)
}

/// Two to the `exponent`, for one from the smallest subnormal's to the
/// largest double's.
fn power_of_two(exponent: i64) -> f64 {
    if exponent >= -1022 {
        f64::from_bits(((exponent + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (exponent + 1074))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(expr: &str) -> JsonPointer {
        JsonPointer::compile(expr).unwrap()
    }

    /// `asLong` of what `at` names in `document`, as rskj's provider reads a
    /// price.
    fn long_at(document: &str, at: &str) -> Option<i64> {
        let tree = read_tree(document).unwrap();
        tree.as_ref()
            .and_then(|root| root.at(&pointer(at)))
            .map(Node::as_long)
    }

    #[test]
    fn a_pointer_is_compiled_as_jacksons() {
        assert_eq!(pointer("").0, Vec::<String>::new());
        assert!(JsonPointer::compile("price").is_err());
        assert_eq!(pointer("/").0, [""]);
        assert_eq!(pointer("//").0, ["", ""]);
        assert_eq!(pointer("/bitcoin/usd").0, ["bitcoin", "usd"]);
        assert_eq!(pointer("/a~1b/c~0d").0, ["a/b", "c~d"]);
        assert_eq!(pointer("/~01").0, ["~1"]);
        assert_eq!(pointer("/a~2").0, ["a~2"], "a ~ before anything else stays");
        assert_eq!(pointer("/a~").0, ["a~"]);
        assert_eq!(
            pointer("/a~/b").0,
            ["a~/b"],
            "and keeps the / after it in the segment"
        );
        assert_eq!(pointer("/~~1").0, ["~~1"]);
    }

    #[test]
    fn an_array_is_indexed_as_jackson_indexes_one() {
        let document = "[10, 11, 12]";
        assert_eq!(long_at(document, "/0"), Some(10));
        assert_eq!(long_at(document, "/2"), Some(12));
        assert_eq!(long_at(document, "/3"), None);
        assert_eq!(long_at(document, "/01"), None, "a leading zero");
        assert_eq!(long_at(document, "/-1"), None);
        assert_eq!(long_at(document, "/+1"), None);
        assert_eq!(array_index("2147483647"), Some(2_147_483_647));
        assert_eq!(array_index("2147483648"), None, "past an int");
        assert_eq!(
            long_at(r#"{"0":5}"#, "/0"),
            Some(5),
            "an object's member by its name"
        );
    }

    #[test]
    fn a_tree_is_read_as_jackson_reads_one() {
        let document = r#" { "bitcoin" : { "usd" : 60000 , "eur" : [ 1 , 2.5 , "3" ] } , "ok" : true , "none" : null } "#;
        assert_eq!(long_at(document, "/bitcoin/usd"), Some(60_000));
        assert_eq!(long_at(document, "/bitcoin/eur/1"), Some(2));
        assert_eq!(long_at(document, "/bitcoin/eur/2"), Some(3));
        assert_eq!(long_at(document, "/ok"), Some(1));
        assert_eq!(long_at(document, "/none"), Some(0));
        assert_eq!(
            long_at(document, "/bitcoin"),
            Some(0),
            "an object reads as 0"
        );
        assert_eq!(long_at(document, "/btc"), None);
        assert_eq!(long_at(document, "/ok/x"), None, "nothing is under a value");
        assert_eq!(
            long_at(r#"{"p":1,"p":2}"#, "/p"),
            Some(2),
            "a name given twice is the last"
        );
        assert_eq!(
            long_at(r#"{"a\/b":7}"#, "/a~1b"),
            Some(7),
            "an escape in a name"
        );
        assert_eq!(
            long_at(r#"{"\ud800":7,"p":1}"#, "/p"),
            Some(1),
            "half a surrogate pair is read"
        );
        assert_eq!(long_at(r#"{"p":"\ud800"}"#, "/p"), Some(0));
        assert_eq!(
            long_at(r#"{"😀":7}"#, "/\u{1f600}"),
            Some(7),
            "a pair escaped is its character"
        );
        assert_eq!(long_at("{\"\u{e9}\":7}", "/\u{e9}"), Some(7));
        assert_eq!(
            read_tree("").unwrap(),
            None,
            "nothing is Jackson's MissingNode"
        );
        assert_eq!(read_tree(" \t\r\n").unwrap(), None);
        assert_eq!(
            long_at(r#"{"p":1} trailing [garbage"#, "/p"),
            Some(1),
            "what follows the value is left unread"
        );
        assert_eq!(long_at(r#"[[[{"p":[{}]}]]]"#, "/0/0/0/p/0"), Some(0));
    }

    #[test]
    fn what_jackson_refuses_by_default_is_refused() {
        for refused in [
            "{",
            "[",
            r#"{"p":1"#,
            r#"{"p":1,}"#,
            "[1,]",
            "[,1]",
            "[1 2]",
            r#"{"p" 1}"#,
            "{p:1}",
            "{'p':1}",
            r#"{"p":01}"#,
            r#"{"p":-}"#,
            r#"{"p":1.}"#,
            r#"{"p":.5}"#,
            r#"{"p":+1}"#,
            r#"{"p":1e}"#,
            r#"{"p":1e+}"#,
            r#"{"p":NaN}"#,
            r#"{"p":Infinity}"#,
            r#"{"p":"a\qb"}"#,
            "[\"a\tb\"]",
            r#"{"p":"\u12"}"#,
            r#"{"p":"unclosed}"#,
            "/* c */ 1",
            "# c\n1",
            "\u{1}1",
            r#"{"p":tru}"#,
            r#"{"p":truex}"#,
            r#"{"p":nul}"#,
            "\u{feff}{}",
        ] {
            assert!(read_tree(refused).is_err(), "{refused:?} was read");
        }
    }

    #[test]
    fn a_number_or_a_word_at_the_top_is_read_as_jackson_reads_it() {
        let top = |document: &str| read_tree(document).map(|tree| tree.map(|node| node.as_long()));
        assert_eq!(top("42"), Ok(Some(42)));
        assert_eq!(top("42 x"), Ok(Some(42)), "whitespace, then anything");
        assert!(top("42x").is_err(), "Jackson's _verifyRootSpace");
        assert!(top("42,").is_err());
        assert!(top("42]").is_err());
        assert!(top("4.2e1x").is_err());
        assert_eq!(top("true"), Ok(Some(1)));
        assert_eq!(top("false x"), Ok(Some(0)));
        assert_eq!(
            top("true,"),
            Ok(Some(1)),
            "below '0', nothing a name goes on with"
        );
        assert_eq!(top("true:"), Ok(Some(1)));
        assert_eq!(top("true]"), Ok(Some(1)));
        assert_eq!(
            top("true\u{b2}"),
            Ok(Some(1)),
            "a superscript two is no part of a Java name"
        );
        assert_eq!(
            top("true\u{1f600}"),
            Ok(Some(1)),
            "nor is half a surrogate pair"
        );
        assert!(top("true_").is_err());
        assert!(top("true1").is_err());
        assert!(top("true\u{e9}").is_err());
        assert!(top("true\u{a5}").is_err(), "a currency sign is part of one");
        assert!(
            top("null\u{ad}").is_err(),
            "and a soft hyphen, which Java ignores in one"
        );
        assert_eq!(top("\"7\" x"), Ok(Some(7)));
    }

    #[test]
    fn jacksons_limits_are_kept() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        assert!(read_tree(&nested(1000)).is_ok());
        assert!(read_tree(&nested(1001)).is_err());
        assert!(read_tree(&"1".repeat(1000)).is_ok());
        assert!(read_tree(&"1".repeat(1001)).is_err());
        assert!(read_tree(&format!("1.{}", "1".repeat(999))).is_ok());
        assert!(
            read_tree(&format!("1.{}", "1".repeat(1000))).is_err(),
            "a fraction's digits count"
        );
        assert!(
            read_tree(&format!("1e{}", "1".repeat(1000))).is_err(),
            "and an exponent's"
        );
    }

    #[test]
    fn a_whole_number_reads_as_its_low_64_bits() {
        assert_eq!(long_at("9223372036854775807", ""), Some(i64::MAX));
        assert_eq!(long_at("-9223372036854775808", ""), Some(i64::MIN));
        assert_eq!(
            long_at("9223372036854775808", ""),
            Some(i64::MIN),
            "BigInteger.longValue"
        );
        assert_eq!(long_at("18446744073709551616", ""), Some(0));
        assert_eq!(long_at("18446744073709551621", ""), Some(5));
        assert_eq!(long_at("-18446744073709551617", ""), Some(-1));
        assert_eq!(
            long_at("100000000000000000000000", ""),
            Some(100_000_000_000_000_000_000_000u128 as u64 as i64)
        );
        assert_eq!(long_at("-0", ""), Some(0));
    }

    #[test]
    fn a_double_reads_as_javas_cast() {
        assert_eq!(long_at(r#"{"p":60000.9}"#, "/p"), Some(60_000));
        assert_eq!(long_at(r#"{"p":-60000.9}"#, "/p"), Some(-60_000));
        assert_eq!(long_at(r#"{"p":1e4}"#, "/p"), Some(10_000));
        assert_eq!(long_at(r#"{"p":1E+4}"#, "/p"), Some(10_000));
        assert_eq!(
            long_at(r#"{"p":1e400}"#, "/p"),
            Some(i64::MAX),
            "past a double: infinite"
        );
        assert_eq!(long_at(r#"{"p":-1e400}"#, "/p"), Some(i64::MIN));
        assert_eq!(long_at(r#"{"p":1e-400}"#, "/p"), Some(0));
        assert_eq!(
            long_at(r#"{"p":9.3e18}"#, "/p"),
            Some(i64::MAX),
            "past a long: the cast saturates"
        );
    }

    #[test]
    fn a_string_reads_as_jacksons_parse_as_long() {
        for (text, long) in [
            ("60000", 60_000),
            (" 60000 ", 60_000),
            ("\t60000\u{1}", 60_000),
            ("+60000", 60_000),
            ("-60000", -60_000),
            ("60000.9", 60_000),
            ("6e4", 60_000),
            ("1.5e3d", 1_500),
            ("1.5f", 1),
            ("0x1p4", 16),
            ("-0x1.8p1", -3),
            ("99999999999999999999", 0),
            ("9223372036854775807", i64::MAX),
            ("abc", 0),
            ("", 0),
            ("+", 0),
            ("-", 0),
            ("+-5", -5),
            ("-+5", 0),
            ("++5", 5),
            ("NaN", 0),
            ("Infinity", i64::MAX),
            ("-Infinity", i64::MIN),
            ("inf", 0),
            ("1e", 0),
            ("0x10", 0),
            ("1_000", 0),
            ("\u{663}", 0),
        ] {
            assert_eq!(parse_as_long(text), long, "{text:?}");
        }
    }

    #[test]
    fn a_double_is_read_as_java_reads_one() {
        for (text, value) in [
            ("1", 1.0),
            ("1.", 1.0),
            (".5", 0.5),
            ("1e3", 1000.0),
            ("1E-3", 0.001),
            ("+2.5", 2.5),
            ("-2.5", -2.5),
            ("2.5d", 2.5),
            ("2.5F", 2.5),
            (" 7 ", 7.0),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("0x1p3", 8.0),
            ("0X1.8P1", 3.0),
            ("0x.8p1", 1.0),
            ("0x1.p1", 2.0),
            ("0x1p3d", 8.0),
            ("0x1P-1", 0.5),
            ("0x0p0", 0.0),
            ("0x0.0p99999999999", 0.0),
            ("0x1p99999999999", f64::INFINITY),
            ("0x1p-99999999999", 0.0),
            ("0x1.fffffffffffffp1023", f64::MAX),
            ("0x1p1024", f64::INFINITY),
            ("0x1.fffffffffffff8p1023", f64::INFINITY),
            ("0x1p-1022", f64::MIN_POSITIVE),
            ("0x1p-1074", f64::from_bits(1)),
            ("0x1p-1075", 0.0),
            ("0x1.8p-1075", f64::from_bits(1)),
            ("0x1.00000000000008p0", 1.0),
            ("0x1.00000000000018p0", 1.0 + 2.0 * f64::EPSILON),
            ("0x1.0000000000000801p0", 1.0 + f64::EPSILON),
            ("0x0000000000000000000001p0", 1.0),
            ("0x10000000000000000000p-76", 1.0),
        ] {
            assert_eq!(parse_double(text), Some(value), "{text:?}");
        }
        assert!(parse_double("NaN").is_some_and(f64::is_nan));
        for refused in [
            "",
            ".",
            "e5",
            "1e",
            "1e+",
            "1.2.3",
            "0x1",
            "0x1p",
            "0xp1",
            "0x.p1",
            "0xgp1",
            "1d5",
            "inf",
            "nan",
            "NaNd",
            "Infinityf",
            "1_0",
            "0x1p1.5",
            "--1",
        ] {
            assert_eq!(parse_double(refused), None, "{refused:?}");
        }
    }
}
