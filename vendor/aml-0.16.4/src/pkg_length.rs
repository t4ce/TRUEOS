use crate::{
    parser::{take, take_n, Parser, Propagate},
    AmlContext,
    AmlError,
};
use bit_field::BitField;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PkgLength {
    pub raw_length: u32,
    /// The offset in the structure's stream to stop parsing at - the "end" of the PkgLength. We need to track this
    /// instead of the actual length encoded in the PkgLength as we often need to parse some stuff between the
    /// PkgLength and the explicit-length structure.
    pub end_offset: u32,
}

impl PkgLength {
    pub fn from_raw_length(stream: &[u8], raw_length: u32) -> Result<PkgLength, AmlError> {
        Ok(PkgLength {
            raw_length,
            end_offset: (stream.len() as u32).checked_sub(raw_length).ok_or(AmlError::InvalidPkgLength)?,
        })
    }

    /// Returns `true` if the given stream is still within the structure this `PkgLength` refers
    /// to.
    pub fn still_parsing(&self, stream: &[u8]) -> bool {
        stream.len() as u32 > self.end_offset
    }
}

pub fn pkg_length<'a, 'c>() -> impl Parser<'a, 'c, PkgLength>
where
    'c: 'a,
{
    move |input: &'a [u8], context: &'c mut AmlContext| -> crate::parser::ParseResult<'a, 'c, PkgLength> {
        let (new_input, context, raw_length) = raw_pkg_length().parse(input, context)?;

        /*
         * NOTE: we use the original input here, because `raw_length` includes the length of the
         * `PkgLength`.
         */
        match PkgLength::from_raw_length(input, raw_length) {
            Ok(pkg_length) => Ok((new_input, context, pkg_length)),
            Err(err) => Err((input, context, Propagate::Err(err))),
        }
    }
}

/// Parses a `PkgLength` and returns the *raw length*. If you want an instance of `PkgLength`, use
/// `pkg_length` instead.
pub fn raw_pkg_length<'a, 'c>() -> impl Parser<'a, 'c, u32>
where
    'c: 'a,
{
    /*
     * PkgLength := PkgLeadByte |
     * <PkgLeadByte ByteData> |
     * <PkgLeadByte ByteData ByteData> |
     * <PkgLeadByte ByteData ByteData ByteData>
     *
     * The length encoded by the PkgLength includes the number of bytes used to encode it.
     */
    move |input: &'a [u8], context: &'c mut AmlContext| {
        let (new_input, context, lead_byte) = take().parse(input, context)?;
        let byte_count = lead_byte.get_bits(6..8);

        if byte_count == 0 {
            let length = u32::from(lead_byte.get_bits(0..6));
            return Ok((new_input, context, length));
        }

        let (new_input, context, length): (&[u8], &mut AmlContext, u32) = match take_n(byte_count as u32)
            .parse(new_input, context)
        {
            Ok((new_input, context, bytes)) => {
                let initial_length = u32::from(lead_byte.get_bits(0..4));
                (
                    new_input,
                    context,
                    bytes
                        .iter()
                        .enumerate()
                        .fold(initial_length, |length, (i, &byte)| length + (u32::from(byte) << (4 + i * 8))),
                )
            }

            /*
             * The stream was too short. We return an error, making sure to return the
             * *original* stream (that we haven't consumed any of).
             */
            Err((_, context, _)) => return Err((input, context, Propagate::Err(AmlError::UnexpectedEndOfStream))),
        };

        Ok((new_input, context, length))
    }
}
