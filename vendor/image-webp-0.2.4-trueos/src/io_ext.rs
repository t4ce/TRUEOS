use byteorder_lite::ByteOrder;
use core3::io::{self, Read};
pub(crate) trait ReadBytesExt: Read {
    fn read_u8(&mut self) -> io::Result<u8> { let mut bytes=[0;1];self.read_exact(&mut bytes)?;Ok(bytes[0]) }
    fn read_u16<B: ByteOrder>(&mut self) -> io::Result<u16> { let mut bytes=[0;2];self.read_exact(&mut bytes)?;Ok(B::read_u16(&bytes)) }
    fn read_u24<B: ByteOrder>(&mut self) -> io::Result<u32> { let mut bytes=[0;3];self.read_exact(&mut bytes)?;Ok(B::read_u24(&bytes)) }
    fn read_u32<B: ByteOrder>(&mut self) -> io::Result<u32> { let mut bytes=[0;4];self.read_exact(&mut bytes)?;Ok(B::read_u32(&bytes)) }
}
impl<R:Read+?Sized> ReadBytesExt for R {}

pub(crate) trait SeekExt: core3::io::Seek {
    fn stream_position(&mut self) -> core3::io::Result<u64> { self.seek(core3::io::SeekFrom::Current(0)) }
    fn seek_relative(&mut self, offset:i64) -> core3::io::Result<()> { self.seek(core3::io::SeekFrom::Current(offset)).map(|_| ()) }
}
impl<S:core3::io::Seek+?Sized> SeekExt for S {}
