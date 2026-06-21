use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::From;
use core::fmt;

use bitstream::BitRead;
use error::{Error, Result};

#[derive(Clone, Debug)]
pub struct Header {
    channel_count: usize,
    sample_rate: u32,
    bitrates: Bitrates,
    frame_lens: FrameLens,
}

impl Header {
    pub fn read<R: BitRead>(reader: &mut R) -> Result<Header> {
        if try!(reader.read_u32()) != 0 {
            return Err(Error::Undecodable("Unsupported Vorbis version"));
        }

        let channel_count = try!(reader.read_u8()) as usize;
        if channel_count == 0 {
            return Err(Error::Undecodable("Invalid channel count"));
        }

        let sample_rate = try!(reader.read_u32());
        if sample_rate == 0 {
            return Err(Error::Undecodable("Invalid sample rate"));
        }

        let bitrate_max = try!(reader.read_i32());
        let bitrate_nom = try!(reader.read_i32());
        let bitrate_min = try!(reader.read_i32());

        let frame_len_short = 1 << try!(reader.read_u8_bits(4)) as usize;
        if frame_len_short < 64 || frame_len_short > 8192 {
            return Err(Error::Undecodable("Invalid short frame length"));
        }
        let frame_len_long = 1 << try!(reader.read_u8_bits(4)) as usize;
        if frame_len_long < 64 || frame_len_long > 8192 {
            return Err(Error::Undecodable("Invalid long frame length"));
        }
        if frame_len_long < frame_len_short {
            return Err(Error::Undecodable("Long frame is shorter than short frame"));
        }

        if !try!(reader.read_bool()) {
            return Err(Error::Undecodable("Invalid framing bit"));
        }

        Ok(Header {
            channel_count: channel_count,
            sample_rate: sample_rate,
            bitrates: Bitrates {
                min: bitrate_min,
                nom: bitrate_nom,
                max: bitrate_max,
            },
            frame_lens: FrameLens {
                short: frame_len_short,
                long: frame_len_long,
            },
        })
    }

    pub fn channel_count(&self) -> usize {
        self.channel_count
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn bitrates(&self) -> Bitrates {
        self.bitrates
    }

    pub fn frame_lens(&self) -> FrameLens {
        self.frame_lens
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bitrates {
    min: i32,
    nom: i32,
    max: i32,
}

impl Bitrates {
    pub fn min(&self) -> i32 {
        self.min
    }

    pub fn nom(&self) -> i32 {
        self.nom
    }

    pub fn max(&self) -> i32 {
        self.max
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameKind {
    Short,
    Long,
}

#[derive(Clone, Copy, Debug)]
pub struct FrameLens {
    short: usize,
    long: usize,
}

impl FrameLens {
    pub fn new(short: usize, long: usize) -> Self {
        assert!(long >= short);
        FrameLens {
            short: short,
            long: long,
        }
    }

    pub fn short(&self) -> usize {
        self.short
    }

    pub fn long(&self) -> usize {
        self.long
    }

    pub fn get(&self, kind: FrameKind) -> usize {
        match kind {
            FrameKind::Short => self.short,
            FrameKind::Long => self.long,
        }
    }
}

#[derive(Debug)]
pub enum CommentTag<'a> {
    Title,
    Version,
    Album,
    TrackNumber,
    Artist,
    Performer,
    Copyright,
    License,
    Organization,
    Description,
    Genre,
    Date,
    Location,
    Contact,
    Isrc,
    Custom(&'a str),
}

impl<'a> CommentTag<'a> {
    pub fn normalize(self) -> Self {
        if let CommentTag::Custom(s) = self {
            CommentTag::from(s)
        } else {
            self
        }
    }
}

impl<'a> AsRef<str> for CommentTag<'a> {
    fn as_ref(&self) -> &str {
        match self {
            &CommentTag::Title        => "TITLE",
            &CommentTag::Version      => "VERSION",
            &CommentTag::Album        => "ALBUM",
            &CommentTag::TrackNumber  => "TRACKNUMBER",
            &CommentTag::Artist       => "ARTIST",
            &CommentTag::Performer    => "PERFORMER",
            &CommentTag::Copyright    => "COPYRIGHT",
            &CommentTag::License      => "LICENSE",
            &CommentTag::Organization => "ORGANIZATION",
            &CommentTag::Description  => "DESCRIPTION",
            &CommentTag::Genre        => "GENRE",
            &CommentTag::Date         => "DATE",
            &CommentTag::Location     => "LOCATION",
            &CommentTag::Contact      => "CONTACT",
            &CommentTag::Isrc         => "ISRC",
            &CommentTag::Custom(s)    => s,
        }
    }
}

impl<'a> From<&'a str> for CommentTag<'a> {
    fn from(s: &'a str) -> Self {
        match s {
            s if "TITLE".eq_ignore_ascii_case(s)        => CommentTag::Title,
            s if "VERSION".eq_ignore_ascii_case(s)      => CommentTag::Version,
            s if "ALBUM".eq_ignore_ascii_case(s)        => CommentTag::Album,
            s if "TRACKNUMBER".eq_ignore_ascii_case(s)  => CommentTag::TrackNumber,
            s if "ARTIST".eq_ignore_ascii_case(s)       => CommentTag::Artist,
            s if "PERFORMER".eq_ignore_ascii_case(s)    => CommentTag::Performer,
            s if "COPYRIGHT".eq_ignore_ascii_case(s)    => CommentTag::Copyright,
            s if "LICENSE".eq_ignore_ascii_case(s)      => CommentTag::License,
            s if "ORGANIZATION".eq_ignore_ascii_case(s) => CommentTag::Organization,
            s if "DESCRIPTION".eq_ignore_ascii_case(s)  => CommentTag::Description,
            s if "GENRE".eq_ignore_ascii_case(s)        => CommentTag::Genre,
            s if "DATE".eq_ignore_ascii_case(s)         => CommentTag::Date,
            s if "LOCATION".eq_ignore_ascii_case(s)     => CommentTag::Location,
            s if "CONTACT".eq_ignore_ascii_case(s)      => CommentTag::Contact,
            s if "ISRC".eq_ignore_ascii_case(s)         => CommentTag::Isrc,
            _ => CommentTag::Custom(s),
        }
    }
}

impl<'a> fmt::Display for CommentTag<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            &CommentTag::Title        => "Title",
            &CommentTag::Version      => "Version",
            &CommentTag::Album        => "Album",
            &CommentTag::TrackNumber  => "Track number",
            &CommentTag::Artist       => "Artist",
            &CommentTag::Performer    => "Performer",
            &CommentTag::Copyright    => "Copyright",
            &CommentTag::License      => "License",
            &CommentTag::Organization => "Organization",
            &CommentTag::Description  => "Description",
            &CommentTag::Genre        => "Genre",
            &CommentTag::Date         => "Date",
            &CommentTag::Location     => "Location",
            &CommentTag::Contact      => "Contact",
            &CommentTag::Isrc         => "ISRC",
            &CommentTag::Custom(s)    => s,
        };
        write!(f, "{}", s)
    }
}

impl<'a> PartialEq for CommentTag<'a> {
    fn eq(&self, other: &CommentTag) -> bool {
        self.as_ref().eq_ignore_ascii_case(other.as_ref())
    }
}

#[derive(Clone, Debug)]
pub struct Comments {
    vendor: Option<String>,
    comments: Box<[String]>,
}

impl Comments {
    pub fn read<R: BitRead>(reader: &mut R) -> Result<Self> {
        let vendor = try!(Self::read_string(reader));

        let comment_count = try!(reader.read_u32()) as usize;
        let mut comments = Vec::with_capacity(comment_count);
        for _ in 0..comment_count {
            let s = try!(Self::read_string(reader));
            if let Some(s) = s {
                comments.push(s);
            }
        }

        let framing_bit = try!(reader.read_bool());
        if !framing_bit {
            return Err(Error::Undecodable("Invalid framing bit"));
        }

        Ok(Comments {
            vendor: vendor,
            comments: comments.into_boxed_slice(),
        })
    }

    pub fn vendor(&self) -> Option<&str> {
        self.vendor.as_ref().map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.comments.len()
    }

    pub fn raw(&self) -> &[String] {
        &self.comments
    }

    pub fn iter<'a>(&'a self) -> Box<Iterator<Item=(CommentTag<'a>, &'a str)> + 'a> {
        let iter = self.comments.iter()
            .filter_map(move |ref s| {
                let mut split_iter = s.splitn(2, '=');
                let tag = split_iter.next();
                let val = split_iter.next();
                if let (Some(tag), Some(val)) = (tag, val) {
                    Some((CommentTag::from(tag), val))
                } else {
                    None
                }
            });
        Box::new(iter)
    }

    pub fn by_tag<'a>(&'a self, tag: CommentTag<'a>) -> Box<Iterator<Item=&'a str> + 'a> {
        let iter = self.iter()
            .filter_map(move |(t, v)| if t == tag {
                Some(v)
            } else {
                None
            });
        Box::new(iter)
    }

    fn read_string<R: BitRead>(reader: &mut R) -> Result<Option<String>> {
        let len = try!(reader.read_u32()) as usize;
        let mut bytes = vec![0; len];
        try!(reader.read_exact(&mut bytes));
        Ok(String::from_utf8(bytes).ok())
    }
}

impl<'a> IntoIterator for &'a Comments {
    type Item = (CommentTag<'a>, &'a str);
    type IntoIter = Box<Iterator<Item=Self::Item> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
