//! A federator's signature as bitcoinj-thin's
//! `BtcECKey.ECDSASignature.decodeFromDER` reads it.
//!
//! rskj reads every signature `addSignature` is given this way before it does
//! anything else, and fails the call where it throws (`Bridge.addSignature`);
//! then it reads each again to sign with it
//! (`BridgeSupport.getDecodedSignatures`).
//!
//! bitcoinj reads with the ASN.1 classes of Bouncy Castle 1.58, which rskj
//! ships under SpongyCastle's package names. `ASN1InputStream` reads one
//! object, and bitcoinj casts it to a sequence of a definite length (a DL
//! sequence) and its first two elements to INTEGERs; nothing after that object
//! is looked at. But Bouncy Castle parses every element of the sequence as it
//! builds it, and every element inside those, and throws where it can't, so a
//! signature is refused for anything after its two integers that Bouncy Castle
//! can't read. This module reads as those classes do, with their streams:
//! `DefiniteLengthInputStream`, `IndefiniteLengthInputStream` with its two
//! bytes of look-ahead for the end-of-contents octets, `ASN1InputStream` for
//! what a definite length holds and `ASN1StreamParser` for what an indefinite
//! one holds. Where they leave bytes unread (a NULL's contents, the odd byte of
//! a BMPString), the next object starts there, as it does in Bouncy Castle.
//!
//! Bouncy Castle sets no limit on how deep objects may nest, and neither does
//! this. It keeps a stack of its own of the objects it is inside, and works
//! out where each stream stands from how far into the signature it has read,
//! where Bouncy Castle passes each byte through every stream in turn, so a
//! signature is read in time that grows with its length, however deep it goes.

use std::collections::BTreeSet;

/// Bouncy Castle threw. rskj treats every way it throws alike.
struct Refused;

type Read<T> = Result<T, Refused>;

// Bouncy Castle's `BERTags`: the bits of a tag byte, and the universal tag
// numbers its readers know.
const CONSTRUCTED: i32 = 0x20;
const APPLICATION: i32 = 0x40;
const TAGGED: i32 = 0x80;
const BOOLEAN: i32 = 0x01;
const INTEGER: i32 = 0x02;
const BIT_STRING: i32 = 0x03;
const OCTET_STRING: i32 = 0x04;
const NULL: i32 = 0x05;
const OBJECT_IDENTIFIER: i32 = 0x06;
const EXTERNAL: i32 = 0x08;
const ENUMERATED: i32 = 0x0a;
const UTF8_STRING: i32 = 0x0c;
const SEQUENCE: i32 = 0x10;
const SET: i32 = 0x11;
const NUMERIC_STRING: i32 = 0x12;
const PRINTABLE_STRING: i32 = 0x13;
const T61_STRING: i32 = 0x14;
const VIDEOTEX_STRING: i32 = 0x15;
const IA5_STRING: i32 = 0x16;
const UTC_TIME: i32 = 0x17;
const GENERALIZED_TIME: i32 = 0x18;
const GRAPHIC_STRING: i32 = 0x19;
const VISIBLE_STRING: i32 = 0x1a;
const GENERAL_STRING: i32 = 0x1b;
const UNIVERSAL_STRING: i32 = 0x1c;
const BMP_STRING: i32 = 0x1e;

/// What a stream reads from: the signature's bytes, or another stream.
#[derive(Clone, Copy)]
enum Source {
    Bytes,
    Stream(usize),
}

/// One of Bouncy Castle's limited input streams. Every byte a stream gives
/// comes from the signature through each stream under it, in order, so where a
/// stream stands follows from how many of the signature's bytes have been read,
/// and is not counted byte by byte.
#[derive(Clone, Copy)]
struct Stream {
    /// The stream it reads from.
    inner: Source,
    /// Its place among the streams open.
    slot: usize,
    /// How far the next byte it gives lies behind the next byte of the
    /// signature: two bytes for each indefinite stream it reads through,
    /// itself among them, as each holds two bytes ahead of what it has given.
    lag: usize,
    /// Where the first to end of the definite streams under it ends.
    first_end_below: usize,
    kind: Kind,
}

#[derive(Clone, Copy)]
enum Kind {
    /// `DefiniteLengthInputStream`, which has nothing left once the signature
    /// has been read up to `end`.
    Definite { end: usize },
    /// `IndefiniteLengthInputStream`, which ends at the end-of-contents octets
    /// `00 00` in its two bytes of look-ahead, and looks for them only while
    /// `eof_on_00`.
    Indefinite { eof_on_00: bool, eof_reached: bool },
}

/// An object, as far as what reads it asks: `decodeFromDER` (the outer
/// sequence, and its first two elements as integers), `DERExternal` (an object
/// identifier, an integer, a tagged object's number) and a constructed OCTET
/// STRING (its parts).
enum Object {
    /// An INTEGER of `len` bytes, at `at` in the signature.
    Integer {
        at: usize,
        len: usize,
    },
    /// An OBJECT IDENTIFIER.
    Oid,
    /// A primitive OCTET STRING, a `DEROctetString`.
    DerOctetString,
    /// A constructed one, a `BEROctetString`.
    BerOctetString,
    Tagged(i32),
    /// The outer object, a SEQUENCE of a definite length with elements, which
    /// `DERFactory.createSequence` makes a `DLSequence`.
    DlSequence(Vec<Object>),
    Other,
}

/// `ASN1StreamParser`: a stream to read objects from, and the limit their
/// lengths are held to.
#[derive(Clone, Copy)]
struct Parser {
    source: Source,
    limit: i32,
}

/// What `ASN1StreamParser.readObject` gives: an object, or a parser of one,
/// which reads its contents when it is loaded.
enum Parsed {
    Loaded(Object),
    /// `DEROctetStringParser`, over a definite stream.
    DerOctetString(usize),
    /// `BEROctetStringParser`.
    BerOctetString(Parser),
    /// The parsers of the sequences and sets, and
    /// `BERApplicationSpecificParser`: a vector of objects.
    Vector(Parser),
    /// `DERExternalParser`.
    External(Parser),
    /// `BERTaggedObjectParser`.
    Tagged {
        constructed: bool,
        tag_no: i32,
        parser: Parser,
    },
}

/// A constructed object whose contents are being read, and what has been read
/// of them.
enum Frame {
    /// `ASN1InputStream.buildObject` of a constructed OCTET STRING, SEQUENCE,
    /// SET or EXTERNAL of a definite length, whose contents an
    /// `ASN1InputStream` of their own reads, held to their length
    /// (`buildDEREncodableVector`). `top` marks the outer object.
    Built {
        inside: Source,
        limit: i32,
        tag_no: i32,
        top: bool,
        items: Vec<Object>,
    },
    /// `ASN1StreamParser.readVector`, each object loaded: the contents of a
    /// sequence, a set, an application-specific object, an EXTERNAL or a
    /// constructed tagged object that a parser reads.
    Vector {
        parser: Parser,
        makes: Makes,
        items: Vec<Object>,
    },
    /// `ConstructedOctetStream`: the parts of a constructed OCTET STRING that a
    /// parser reads, each read to its end (`BEROctetStringParser`).
    Octets(Parser),
}

/// What a `Frame::Vector` makes of the objects it read.
#[derive(Clone, Copy)]
enum Makes {
    Other,
    External,
    Tagged(i32),
}

/// What reading the next object in a frame's contents gives.
enum Step {
    /// The object, read whole.
    Done(Object),
    /// A constructed object, whose contents are read next.
    Open(Frame),
    /// The end of the frame's contents.
    End,
}

impl Frame {
    fn vector(parser: Parser, makes: Makes) -> Self {
        Frame::Vector {
            parser,
            makes,
            items: Vec::new(),
        }
    }

    fn add(&mut self, object: Object) {
        match self {
            Frame::Built { items, .. } | Frame::Vector { items, .. } => items.push(object),
            Frame::Octets(_) => {}
        }
    }

    /// The object, its contents read.
    fn finish(self) -> Read<Object> {
        match self {
            Frame::Built {
                tag_no, top, items, ..
            } => match tag_no {
                // `new BEROctetString(ASN1OctetString[])`, which casts each part
                // to a `DEROctetString`.
                OCTET_STRING
                    if items
                        .iter()
                        .all(|item| matches!(item, Object::DerOctetString)) =>
                {
                    Ok(Object::BerOctetString)
                }
                SEQUENCE if top && !items.is_empty() => Ok(Object::DlSequence(items)),
                SEQUENCE | SET => Ok(Object::Other),
                EXTERNAL => external(&items).map(|()| Object::Other),
                _ => Err(Refused),
            },
            Frame::Vector {
                makes: Makes::Other,
                ..
            } => Ok(Object::Other),
            Frame::Vector {
                makes: Makes::External,
                items,
                ..
            } => external(&items).map(|()| Object::Other),
            Frame::Vector {
                makes: Makes::Tagged(tag_no),
                ..
            } => Ok(Object::Tagged(tag_no)),
            Frame::Octets(_) => Ok(Object::BerOctetString),
        }
    }
}

/// How many bytes a primitive holds, and the first two of them.
#[derive(Default)]
struct Content {
    len: usize,
    head: [u8; 2],
}

impl Content {
    /// `ASN1Integer.isMalformed`: a first byte that only repeats the sign of
    /// the next.
    fn malformed(&self) -> bool {
        let [first, next] = self.head;
        self.len > 1 && ((first == 0x00 && next < 0x80) || (first == 0xff && next >= 0x80))
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    /// How many of the signature's bytes have been read.
    pos: usize,
    streams: Vec<Stream>,
    /// The streams open, each reading from the one before it, the first from
    /// the signature's bytes. Bouncy Castle reads from a stream only once it is
    /// done with those over it, so reading from one closes them.
    open: Vec<usize>,
    /// The slots of the indefinite streams open that look for `00 00`, or have
    /// found them.
    watching: BTreeSet<usize>,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader {
            bytes,
            pos: 0,
            streams: Vec::new(),
            open: Vec::new(),
            watching: BTreeSet::new(),
        }
    }

    /// `new ASN1InputStream(bytes).readObject()`: the outer object, read whole.
    /// Bouncy Castle reads an object inside another by calling itself; here
    /// each constructed object being read is a frame on a stack, and its
    /// contents are read in the same order.
    fn signature(&mut self) -> Read<Object> {
        let limit = i32::try_from(self.bytes.len()).unwrap_or(i32::MAX);
        let mut frames: Vec<Frame> = Vec::new();
        let mut step = self.object(Source::Bytes, limit, true)?;
        loop {
            let object = match step {
                Step::Open(frame) => {
                    frames.push(frame);
                    None
                }
                Step::Done(object) => Some(object),
                // With no frame open, the bytes held no object, and bitcoinj
                // throws.
                Step::End => Some(frames.pop().ok_or(Refused)?.finish()?),
            };
            if let Some(object) = object {
                let Some(frame) = frames.last_mut() else {
                    return Ok(object);
                };
                frame.add(object);
            }
            let Some(frame) = frames.last() else {
                return Err(Refused);
            };
            step = self.next(frame)?;
        }
    }

    /// The next object in `frame`'s contents, or their end.
    fn next(&mut self, frame: &Frame) -> Read<Step> {
        match *frame {
            Frame::Built { inside, limit, .. } => self.object(inside, limit, false),
            Frame::Vector { parser, .. } => match self.parsed(parser)? {
                Some(parsed) => self.load(parsed),
                None => Ok(Step::End),
            },
            // `ConstructedOctetStream`, which casts each part to an
            // `ASN1OctetStringParser`.
            Frame::Octets(parser) => match self.parsed(parser)? {
                Some(Parsed::DerOctetString(def)) => {
                    self.drain(def).map(|_| Step::Done(Object::DerOctetString))
                }
                Some(Parsed::BerOctetString(inner)) => Ok(Step::Open(Frame::Octets(inner))),
                Some(_) => Err(Refused),
                None => Ok(Step::End),
            },
        }
    }

    /// `read()` of `source`: its next byte, or -1 at its end. A stream throws
    /// `EOFException` where one under it has ended, as where the signature has.
    fn read(&mut self, source: Source) -> Read<i32> {
        self.close_above(source);
        let Source::Stream(i) = source else {
            let byte = self.byte_at(self.pos);
            self.pos += usize::from(byte >= 0);
            return Ok(byte);
        };
        if self.at_end(i) {
            return Ok(-1);
        }
        if self.pos >= self.bytes.len() || self.ended_below(i) {
            return Err(Refused);
        }
        let byte = self.byte_at(self.pos.checked_sub(self.streams[i].lag).ok_or(Refused)?);
        self.pos += 1;
        Ok(byte)
    }

    /// The signature's byte at `at`, or -1 past its end.
    fn byte_at(&self, at: usize) -> i32 {
        self.bytes.get(at).map_or(-1, |&byte| i32::from(byte))
    }

    /// Whether stream `i` has come to its end: a definite stream with nothing
    /// left, or an indefinite one at `00 00` while it looks for them.
    fn at_end(&mut self, i: usize) -> bool {
        match self.streams[i].kind {
            Kind::Definite { end } => end <= self.pos,
            Kind::Indefinite { .. } => self.check_for_eof(i),
        }
    }

    /// Whether a stream under stream `i` has ended, so that it gives -1, on
    /// which the stream over it throws. (What `checkForEof` marks on the way
    /// no longer matters once the read throws.)
    fn ended_below(&self, i: usize) -> bool {
        let Stream {
            slot,
            first_end_below,
            ..
        } = self.streams[i];
        first_end_below <= self.pos
            || self.watching.range(..slot).any(|&below| {
                self.open
                    .get(below)
                    .is_some_and(|&j| match self.streams[j].kind {
                        Kind::Indefinite {
                            eof_on_00,
                            eof_reached,
                        } => eof_reached || (eof_on_00 && self.look_ahead(j) == [0, 0]),
                        Kind::Definite { .. } => false,
                    })
            })
    }

    /// The two bytes indefinite stream `i` holds ahead of what it has given.
    fn look_ahead(&self, i: usize) -> [i32; 2] {
        let at = self.pos.wrapping_sub(self.streams[i].lag);
        [self.byte_at(at), self.byte_at(at.wrapping_add(1))]
    }

    /// `IndefiniteLengthInputStream.checkForEof`: whether stream `i` is at its
    /// end, which, the first time it finds `00 00` while it looks for them, it
    /// marks, and tells the stream it reads from (`setParentEofDetect`).
    fn check_for_eof(&mut self, i: usize) -> bool {
        let Kind::Indefinite {
            eof_on_00,
            eof_reached,
        } = self.streams[i].kind
        else {
            return false;
        };
        if eof_reached || !eof_on_00 || self.look_ahead(i) != [0, 0] {
            return eof_reached;
        }
        self.mark_eof_reached(i);
        self.eof_detect(self.streams[i].inner);
        true
    }

    /// `setParentEofDetect(true)` of a stream that reads from `source`: an
    /// indefinite stream there looks for `00 00` again, and if it is at them,
    /// it has ended, and tells the one it reads from in turn.
    fn eof_detect(&mut self, mut source: Source) {
        while let Source::Stream(j) = source {
            let Kind::Indefinite { eof_reached, .. } = self.streams[j].kind else {
                return;
            };
            self.set_eof_on_00(j, true);
            if eof_reached || self.look_ahead(j) != [0, 0] {
                return;
            }
            self.mark_eof_reached(j);
            source = self.streams[j].inner;
        }
    }

    /// `setEofOn00` of indefinite stream `i`.
    fn set_eof_on_00(&mut self, i: usize, on: bool) {
        let slot = self.streams[i].slot;
        if let Kind::Indefinite {
            eof_on_00,
            eof_reached,
        } = &mut self.streams[i].kind
        {
            *eof_on_00 = on;
            if on || *eof_reached {
                self.watching.insert(slot);
            } else {
                self.watching.remove(&slot);
            }
        }
    }

    fn mark_eof_reached(&mut self, i: usize) {
        if let Kind::Indefinite { eof_reached, .. } = &mut self.streams[i].kind {
            *eof_reached = true;
        }
    }

    /// Closes the streams over `source`, which Bouncy Castle is done with once
    /// it reads from `source`. A definite stream that came to its end told the
    /// stream it reads from, when it did, that that one may end at `00 00`
    /// (`setParentEofDetect`). No byte is read between its end and its
    /// closing, so the stream it tells holds the same two bytes then as now,
    /// and here it tells it as it is closed.
    fn close_above(&mut self, source: Source) {
        let keep = match source {
            Source::Bytes => 0,
            Source::Stream(i) => self.streams[i].slot + 1,
        };
        while self.open.len() > keep {
            let Some(j) = self.open.pop() else { break };
            self.watching.remove(&self.open.len());
            let Stream { inner, kind, .. } = self.streams[j];
            if matches!(kind, Kind::Definite { end } if end <= self.pos) {
                self.eof_detect(inner);
            }
        }
    }

    /// Opens a stream of `kind` over `inner`.
    fn open_stream(&mut self, inner: Source, kind: Kind) -> usize {
        self.close_above(inner);
        let (lag, first_end_below) = match inner {
            Source::Bytes => (0, usize::MAX),
            Source::Stream(j) => {
                let Stream {
                    lag,
                    first_end_below,
                    kind,
                    ..
                } = self.streams[j];
                let end = match kind {
                    Kind::Definite { end } => end,
                    Kind::Indefinite { .. } => usize::MAX,
                };
                (lag, first_end_below.min(end))
            }
        };
        let look_ahead = if matches!(kind, Kind::Indefinite { .. }) {
            2
        } else {
            0
        };
        let i = self.streams.len();
        self.streams.push(Stream {
            inner,
            slot: self.open.len(),
            lag: lag + look_ahead,
            first_end_below,
            kind,
        });
        self.open.push(i);
        i
    }

    /// A `DefiniteLengthInputStream` of the next `length` bytes `inner` gives.
    fn definite(&mut self, inner: Source, length: usize) -> usize {
        let end = self.pos.saturating_add(length);
        self.open_stream(inner, Kind::Definite { end })
    }

    /// An `IndefiniteLengthInputStream` over `inner`, which reads its two bytes
    /// of look-ahead as it is made.
    fn indefinite(&mut self, inner: Source) -> Read<usize> {
        self.read(inner)?;
        if self.read(inner)? < 0 {
            return Err(Refused);
        }
        let i = self.open_stream(
            inner,
            Kind::Indefinite {
                eof_on_00: true,
                eof_reached: false,
            },
        );
        self.watching.insert(self.streams[i].slot);
        self.check_for_eof(i);
        Ok(i)
    }

    /// `getRemaining` of definite stream `def`.
    fn remaining(&self, def: usize) -> usize {
        match self.streams[def].kind {
            Kind::Definite { end } => end.saturating_sub(self.pos),
            Kind::Indefinite { .. } => 0,
        }
    }

    /// `DefiniteLengthInputStream.toByteArray`: every byte the definite stream
    /// has left, or `EOFException` where what it wraps ends first.
    fn drain(&mut self, def: usize) -> Read<Content> {
        let mut content = Content::default();
        while self.remaining(def) > 0 {
            let b = u8::try_from(self.read(Source::Stream(def))?).map_err(|_| Refused)?;
            if let Some(slot) = content.head.get_mut(content.len) {
                *slot = b;
            }
            content.len += 1;
        }
        Ok(content)
    }

    /// `ASN1InputStream.readTagNumber`: the number in the tag byte's five bits,
    /// or where they are all set, in the bytes after it, seven bits a byte, in
    /// a Java `int` that drops what it shifts out.
    fn tag_number(&mut self, source: Source, tag: i32) -> Read<i32> {
        let mut tag_no = tag & 0x1f;
        if tag_no == 0x1f {
            tag_no = 0;
            let mut b = self.read(source)?;
            if b & 0x7f == 0 {
                return Err(Refused);
            }
            while b >= 0 && b & 0x80 != 0 {
                tag_no |= b & 0x7f;
                tag_no = tag_no.wrapping_shl(7);
                b = self.read(source)?;
            }
            if b < 0 {
                return Err(Refused);
            }
            tag_no |= b & 0x7f;
        }
        Ok(tag_no)
    }

    /// `ASN1InputStream.readLength`: -1 for the indefinite form, and a long
    /// form of at most four bytes, read into a Java `int`, held below `limit`.
    fn length(&mut self, source: Source, limit: i32) -> Read<i32> {
        let first = self.read(source)?;
        if first < 0 {
            return Err(Refused);
        }
        if first == 0x80 {
            return Ok(-1);
        }
        if first < 0x80 {
            return Ok(first);
        }
        let size = first & 0x7f;
        if size > 4 {
            return Err(Refused);
        }
        let mut length: i32 = 0;
        for _ in 0..size {
            let next = self.read(source)?;
            if next < 0 {
                return Err(Refused);
            }
            length = length.wrapping_shl(8).wrapping_add(next);
        }
        if length < 0 || length >= limit {
            return Err(Refused);
        }
        Ok(length)
    }

    /// `ASN1InputStream.readObject` from `source`, whose lengths `limit` holds:
    /// the next object, or the end. `top` marks the outer object.
    fn object(&mut self, source: Source, limit: i32, top: bool) -> Read<Step> {
        let tag = self.read(source)?;
        if tag < 0 {
            return Ok(Step::End);
        }
        // A tag byte of 0 is an end-of-contents marker where no indefinite
        // length is open.
        if tag == 0 {
            return Err(Refused);
        }
        let tag_no = self.tag_number(source, tag)?;
        let constructed = tag & CONSTRUCTED != 0;
        let length = self.length(source, limit)?;
        let Ok(len) = usize::try_from(length) else {
            if !constructed {
                return Err(Refused);
            }
            let parser = Parser {
                source: Source::Stream(self.indefinite(source)?),
                limit,
            };
            let parsed = if tag & APPLICATION != 0 {
                Parsed::Vector(parser)
            } else if tag & TAGGED != 0 {
                Parsed::Tagged {
                    constructed: true,
                    tag_no,
                    parser,
                }
            } else {
                indefinite_universal(tag_no, parser)?
            };
            return self.load(parsed);
        };
        let def = self.definite(source, len);
        let inside = Source::Stream(def);
        if tag & APPLICATION != 0 {
            self.drain(def)?;
            return Ok(Step::Done(Object::Other));
        }
        if tag & TAGGED != 0 {
            return self.tagged(
                Parser {
                    source: inside,
                    limit: length,
                },
                constructed,
                tag_no,
            );
        }
        if !constructed {
            return self.primitive(def, tag_no).map(Step::Done);
        }
        if !matches!(tag_no, OCTET_STRING | SEQUENCE | SET | EXTERNAL) {
            return Err(Refused);
        }
        Ok(Step::Open(Frame::Built {
            inside,
            limit: length,
            tag_no,
            top,
            items: Vec::new(),
        }))
    }

    /// `ASN1InputStream.createPrimitiveDERObject`: the primitive the definite
    /// stream `def` holds.
    fn primitive(&mut self, def: usize, tag_no: i32) -> Read<Object> {
        match tag_no {
            BIT_STRING => {
                // `ASN1BitString.fromInputStream`: a byte that counts the
                // unused bits, at most 7, and none where no data follows.
                let data = self.remaining(def).checked_sub(1).ok_or(Refused)?;
                let pad = self.read(Source::Stream(def))?;
                self.drain(def)?;
                if pad > 7 || (data == 0 && pad != 0) {
                    return Err(Refused);
                }
                Ok(Object::Other)
            }
            BMP_STRING => {
                // `getBMPCharBuffer`: two bytes a character, an odd one left
                // unread.
                for _ in 0..self.remaining(def) / 2 {
                    self.read(Source::Stream(def))?;
                    self.read(Source::Stream(def))?;
                }
                Ok(Object::Other)
            }
            BOOLEAN => match self.drain(def)?.len {
                1 => Ok(Object::Other),
                _ => Err(Refused),
            },
            ENUMERATED => {
                let content = self.drain(def)?;
                if content.len == 0 || content.malformed() {
                    return Err(Refused);
                }
                Ok(Object::Other)
            }
            INTEGER => {
                let at = self.pos.saturating_sub(self.streams[def].lag);
                let content = self.drain(def)?;
                if content.malformed() {
                    return Err(Refused);
                }
                Ok(Object::Integer {
                    at,
                    len: content.len,
                })
            }
            // `DERNull.INSTANCE`, its contents not read.
            NULL => Ok(Object::Other),
            OBJECT_IDENTIFIER => self.drain(def).map(|_| Object::Oid),
            OCTET_STRING => self.drain(def).map(|_| Object::DerOctetString),
            GENERALIZED_TIME | GENERAL_STRING | IA5_STRING | NUMERIC_STRING | PRINTABLE_STRING
            | T61_STRING | UNIVERSAL_STRING | UTC_TIME | UTF8_STRING | VISIBLE_STRING
            | GRAPHIC_STRING | VIDEOTEX_STRING => self.drain(def).map(|_| Object::Other),
            _ => Err(Refused),
        }
    }

    /// `ASN1StreamParser.readObject`: the next object `parser` holds, or none at
    /// its end.
    fn parsed(&mut self, parser: Parser) -> Read<Option<Parsed>> {
        let tag = self.read(parser.source)?;
        if tag < 0 {
            return Ok(None);
        }
        // `set00Check(false)`: an indefinite stream doesn't end at `00 00`
        // inside an object it is reading.
        if let Source::Stream(j) = parser.source {
            self.set_eof_on_00(j, false);
        }
        let tag_no = self.tag_number(parser.source, tag)?;
        let constructed = tag & CONSTRUCTED != 0;
        let length = self.length(parser.source, parser.limit)?;
        let Ok(len) = usize::try_from(length) else {
            if !constructed {
                return Err(Refused);
            }
            let inner = Parser {
                source: Source::Stream(self.indefinite(parser.source)?),
                limit: parser.limit,
            };
            return Ok(Some(if tag & APPLICATION != 0 {
                Parsed::Vector(inner)
            } else if tag & TAGGED != 0 {
                Parsed::Tagged {
                    constructed: true,
                    tag_no,
                    parser: inner,
                }
            } else {
                indefinite_universal(tag_no, inner)?
            }));
        };
        let def = self.definite(parser.source, len);
        let inside = Parser {
            source: Source::Stream(def),
            limit: length,
        };
        if tag & APPLICATION != 0 {
            self.drain(def)?;
            return Ok(Some(Parsed::Loaded(Object::Other)));
        }
        if tag & TAGGED != 0 {
            return Ok(Some(Parsed::Tagged {
                constructed,
                tag_no,
                parser: inside,
            }));
        }
        if constructed {
            return match tag_no {
                OCTET_STRING => Ok(Some(Parsed::BerOctetString(inside))),
                SEQUENCE | SET => Ok(Some(Parsed::Vector(inside))),
                EXTERNAL => Ok(Some(Parsed::External(inside))),
                _ => Err(Refused),
            };
        }
        if tag_no == OCTET_STRING {
            return Ok(Some(Parsed::DerOctetString(def)));
        }
        self.primitive(def, tag_no)
            .map(|object| Some(Parsed::Loaded(object)))
    }

    /// `getLoadedObject` of what `parsed` is: read whole, or opened to read.
    fn load(&mut self, parsed: Parsed) -> Read<Step> {
        match parsed {
            Parsed::Loaded(object) => Ok(Step::Done(object)),
            Parsed::DerOctetString(def) => {
                self.drain(def).map(|_| Step::Done(Object::DerOctetString))
            }
            Parsed::BerOctetString(parser) => Ok(Step::Open(Frame::Octets(parser))),
            Parsed::Vector(parser) => Ok(Step::Open(Frame::vector(parser, Makes::Other))),
            Parsed::External(parser) => Ok(Step::Open(Frame::vector(parser, Makes::External))),
            Parsed::Tagged {
                constructed,
                tag_no,
                parser,
            } => self.tagged(parser, constructed, tag_no),
        }
    }

    /// `ASN1StreamParser.readTaggedObject`: a primitive's bytes, or the objects
    /// a constructed one holds.
    fn tagged(&mut self, parser: Parser, constructed: bool, tag_no: i32) -> Read<Step> {
        if constructed {
            return Ok(Step::Open(Frame::vector(parser, Makes::Tagged(tag_no))));
        }
        if let Source::Stream(def) = parser.source {
            self.drain(def)?;
        }
        Ok(Step::Done(Object::Tagged(tag_no)))
    }
}

/// What an indefinite length may hold of the universal tags (`readIndef`).
fn indefinite_universal(tag_no: i32, parser: Parser) -> Read<Parsed> {
    match tag_no {
        EXTERNAL => Ok(Parsed::External(parser)),
        OCTET_STRING => Ok(Parsed::BerOctetString(parser)),
        SEQUENCE | SET => Ok(Parsed::Vector(parser)),
        _ => Err(Refused),
    }
}

/// `DERExternal(ASN1EncodableVector)`: an object identifier, an integer and an
/// object that isn't tagged, each if it is there, then a tagged object
/// numbered 0, 1 or 2, and nothing after it.
fn external(items: &[Object]) -> Read<()> {
    let mut offset = 0;
    if matches!(items.get(offset), Some(Object::Oid)) {
        offset += 1;
    }
    if matches!(items.get(offset), Some(Object::Integer { .. })) {
        offset += 1;
    }
    if matches!(items.get(offset), Some(item) if !matches!(item, Object::Tagged(_))) {
        offset += 1;
    }
    match items.get(offset) {
        Some(Object::Tagged(tag_no)) if items.len() == offset + 1 && (0..=2).contains(tag_no) => {
            Ok(())
        }
        _ => Err(Refused),
    }
}

/// The two integers of a signature as bitcoinj's
/// `BtcECKey.ECDSASignature.decodeFromDER` reads them, or `None` where it
/// throws: the outer object must be a SEQUENCE of a definite length whose
/// first two elements are INTEGERs, each read as the unsigned number its bytes
/// spell (`getPositiveValue`), so that a negative encoding is read as a
/// positive number, not refused, and an INTEGER of no bytes as 0.
pub(crate) fn der_signature_integers(sig: &[u8]) -> Option<(&[u8], &[u8])> {
    let Ok(Object::DlSequence(items)) = Reader::new(sig).signature() else {
        return None;
    };
    let integer = |item: Option<&Object>| match item {
        Some(&Object::Integer { at, len }) => sig.get(at..at.checked_add(len)?),
        _ => None,
    };
    Some((integer(items.first())?, integer(items.get(1))?))
}

#[cfg(test)]
mod tests {
    use super::der_signature_integers;
    use alloy_primitives::hex;

    /// Signatures, each with the two integers `decodeFromDER` reads from it, or
    /// `None` where it throws, as the release jar of rskj 9.0.4 reads them.
    const CASES: &[(&str, &str, Option<(&str, &str)>)] = &[
        (
            "two integers in a sequence, as federators sign",
            "3006020105020107",
            Some(("05", "07")),
        ),
        (
            "integers of 33 and 32 bytes",
            "3045022100c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1\
             02207f11111111111111111111111111111111111111111111111111111111111111",
            Some((
                "00c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1",
                "7f11111111111111111111111111111111111111111111111111111111111111",
            )),
        ),
        (
            "negative encodings, read as unsigned",
            "30060201800201ff",
            Some(("80", "ff")),
        ),
        (
            "integers of no bytes, read as 0",
            "300402000200",
            Some(("", "")),
        ),
        (
            "a first byte 00 that only repeats the sign",
            "300702020005020107",
            None,
        ),
        (
            "a first byte ff that only repeats the sign",
            "30070202ff80020107",
            None,
        ),
        (
            "the outer length in the long form",
            "308106020105020107",
            Some(("05", "07")),
        ),
        (
            "an INTEGER tag in the high-tag-number form",
            "30071f020105020107",
            Some(("05", "07")),
        ),
        (
            "a tag number past a Java int, which keeps its low bits",
            "300c1f8180808080020105020107",
            Some(("05", "07")),
        ),
        (
            "bytes after the sequence",
            "3006020105020107ffff",
            Some(("05", "07")),
        ),
        ("r alone", "3003020105", None),
        ("an empty sequence", "3000", None),
        (
            "a sequence of an indefinite length",
            "30800201050201070000",
            None,
        ),
        ("a set", "3106020105020107", None),
        ("no bytes", "", None),
        ("a NULL after s", "30080201050201070500", Some(("05", "07"))),
        (
            "a NULL that claims a byte, which is never read",
            "30080201050201070501",
            Some(("05", "07")),
        ),
        (
            "a NULL whose byte is read as the next tag",
            "3009020105020107050100",
            None,
        ),
        (
            "a BMPString whose odd byte begins the next object",
            "300c0201050201071e0300410500",
            Some(("05", "07")),
        ),
        (
            "end-of-contents octets where no indefinite length is open",
            "30080201050201070000",
            None,
        ),
        ("a BOOLEAN of two bytes", "300a0201050201070102ffff", None),
        (
            "a BIT STRING with 8 unused bits",
            "300a020105020107030208ff",
            None,
        ),
        (
            "a BIT STRING of no data that has unused bits",
            "3009020105020107030101",
            None,
        ),
        (
            "a BIT STRING of no data",
            "3009020105020107030100",
            Some(("05", "07")),
        ),
        ("an ENUMERATED of no bytes", "30080201050201070a00", None),
        (
            "an object identifier of any bytes",
            "300b0201050201070603ffffff",
            Some(("05", "07")),
        ),
        (
            "a universal tag no reader knows",
            "30080201050201070700",
            None,
        ),
        ("a constructed INTEGER", "30080201050201072200", None),
        (
            "a length in five bytes",
            "300e02010502010704850000000001aa",
            None,
        ),
        (
            "a long-form length as long as the limit",
            "3009020105020107058109",
            None,
        ),
        (
            "a short-form length past the limit, which is not held to it",
            "30080201050201070508",
            Some(("05", "07")),
        ),
        (
            "a length past the sequence",
            "300a0201050201070405aabb",
            None,
        ),
        (
            "a sequence of an indefinite length inside",
            "300a02010502010730800000",
            Some(("05", "07")),
        ),
        (
            "an indefinite length that never ends",
            "30080201050201073080",
            None,
        ),
        (
            "00 00 inside an object inside an indefinite length",
            "300e0201050201073080040200000000",
            Some(("05", "07")),
        ),
        (
            "a NULL that claims a byte, inside an indefinite length",
            "300c020105020107308005010000",
            None,
        ),
        (
            "an empty definite object just before the end-of-contents",
            "300c020105020107308030000000",
            Some(("05", "07")),
        ),
        (
            "an indefinite length whose look-ahead runs past its definite one",
            "300b0201050201073003308005",
            None,
        ),
        (
            "an indefinite OCTET STRING of OCTET STRINGs",
            "300f02010502010724800401aa04000000",
            Some(("05", "07")),
        ),
        (
            "one with a part that is not an OCTET STRING",
            "300c020105020107248005000000",
            None,
        ),
        (
            "one with a part of parts",
            "3011020105020107248024800401aa00000000",
            Some(("05", "07")),
        ),
        (
            "a definite OCTET STRING of OCTET STRINGs",
            "300d02010502010724050401aa0400",
            Some(("05", "07")),
        ),
        (
            "one with a constructed part",
            "300c020105020107240424020400",
            None,
        ),
        (
            "an EXTERNAL",
            "300c0201050201072804a0020500",
            Some(("05", "07")),
        ),
        (
            "an EXTERNAL with an identifier and an integer",
            "3012020105020107280a06012a020101a0020500",
            Some(("05", "07")),
        ),
        (
            "an EXTERNAL whose tagged object is numbered 3",
            "300c0201050201072804a3020500",
            None,
        ),
        (
            "an EXTERNAL with an object after its tagged one",
            "300e0201050201072806a00205000500",
            None,
        ),
        (
            "an EXTERNAL of an indefinite length",
            "300e0201050201072880a10205000000",
            Some(("05", "07")),
        ),
        (
            "an application-specific object, its contents not read",
            "30090201050201076101ff",
            Some(("05", "07")),
        ),
        (
            "one of an indefinite length, its contents read",
            "300c020105020107608005000000",
            Some(("05", "07")),
        ),
        (
            "a private tag, read as application-specific",
            "3009020105020107c101ff",
            Some(("05", "07")),
        ),
        (
            "a primitive tagged object",
            "30090201050201078001ff",
            Some(("05", "07")),
        ),
        (
            "a tagged object whose contents are not an object",
            "3009020105020107a001ff",
            None,
        ),
        (
            "a tagged object of an indefinite length",
            "300c020105020107a08005000000",
            Some(("05", "07")),
        ),
        (
            "a private constructed tag, read as application-specific",
            "3009020105020107e101ff",
            Some(("05", "07")),
        ),
        (
            "a SET of an indefinite length inside",
            "300a02010502010731800000",
            Some(("05", "07")),
        ),
        (
            "a primitive of an indefinite length",
            "300a02010502010704800000",
            None,
        ),
        (
            "a high tag number whose first byte holds no bits",
            "300a0201050201071f800500",
            None,
        ),
        (
            "a length past the sequence, into bytes after it",
            "300a0201050201070405aabbccddee",
            None,
        ),
        (
            "an empty indefinite SEQUENCE inside another",
            "300e0201050201073080308000000000",
            Some(("05", "07")),
        ),
        (
            "a primitive of an indefinite length inside an indefinite length",
            "300e0201050201073080048000000000",
            None,
        ),
    ];

    fn read(sig: &[u8]) -> Option<(String, String)> {
        der_signature_integers(sig).map(|(r, s)| (hex::encode(r), hex::encode(s)))
    }

    #[test]
    fn reads_a_signature_as_bouncy_castle_1_58_does() {
        for &(what, sig, integers) in CASES {
            let expected = integers.map(|(r, s)| (r.to_owned(), s.to_owned()));
            assert_eq!(read(&hex::decode(sig).unwrap()), expected, "{what}");
        }
    }

    /// A tag byte and a definite length.
    fn header(tag: u8, len: usize) -> Vec<u8> {
        match u8::try_from(len) {
            Ok(short) if short < 0x80 => vec![tag, short],
            _ => {
                let bytes = len.to_be_bytes();
                let skip = bytes.iter().take_while(|&&byte| byte == 0).count();
                let size = u8::try_from(bytes.len() - skip).unwrap();
                [&[tag, 0x80 | size][..], &bytes[skip..]].concat()
            }
        }
    }

    /// A signature of `r` 5 and `s` 7, then `nest`.
    fn signature(nest: &[u8]) -> Vec<u8> {
        let content = [&[0x02, 0x01, 0x05, 0x02, 0x01, 0x07][..], nest].concat();
        [header(0x30, content.len()), content].concat()
    }

    /// As deep as a signature goes, each checked against the release jar.
    #[test]
    fn reads_as_deep_as_a_signature_goes() {
        let read_5_and_7 = Some(("05".to_owned(), "07".to_owned()));
        // 45,000 SEQUENCEs of an indefinite length, one inside another,
        // around a NULL: 180 kB.
        let depth = 45_000;
        let nest = [
            b"\x30\x80".repeat(depth),
            b"\x05\x00".to_vec(),
            b"\x00\x00".repeat(depth),
        ]
        .concat();
        assert_eq!(read(&signature(&nest)), read_5_and_7);
        // The same, its last end-of-contents octets left off.
        assert_eq!(read(&signature(&nest[..nest.len() - 2])), None);
        // 20,000 SEQUENCEs of a definite length.
        let mut headers = Vec::new();
        let mut inner = 2;
        for _ in 0..20_000 {
            let header = header(0x30, inner);
            inner += header.len();
            headers.push(header);
        }
        let nest = [
            headers.into_iter().rev().flatten().collect(),
            b"\x05\x00".to_vec(),
        ]
        .concat();
        assert_eq!(read(&signature(&nest)), read_5_and_7);
        // 30,000 OCTET STRINGs of an indefinite length, each a part of the
        // one around it.
        let depth = 30_000;
        let nest = [
            b"\x24\x80".repeat(depth),
            b"\x04\x01\xaa".to_vec(),
            b"\x00\x00".repeat(depth),
        ]
        .concat();
        assert_eq!(read(&signature(&nest)), read_5_and_7);
    }
}
