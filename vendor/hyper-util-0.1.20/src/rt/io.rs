use core::marker::Unpin;
use core::pin::Pin;
use core::task::{ready, Poll};

use hyper::{io, rt::{Read, ReadBuf, Write}};

use core::future::poll_fn;

pub(crate) async fn read<T>(io: &mut T, buf: &mut [u8]) -> Result<usize, io::Error>
where
    T: Read + Unpin,
{
    poll_fn(move |cx| {
        let mut buf = ReadBuf::new(buf);
        ready!(Pin::new(&mut *io).poll_read(cx, buf.unfilled()))?;
        Poll::Ready(Ok(buf.filled().len()))
    })
    .await
}

pub(crate) async fn write_all<T>(io: &mut T, buf: &[u8]) -> Result<(), io::Error>
where
    T: Write + Unpin,
{
    let mut n = 0;
    poll_fn(move |cx| {
        while n < buf.len() {
            n += ready!(Pin::new(&mut *io).poll_write(cx, &buf[n..])?);
        }
        Poll::Ready(Ok(()))
    })
    .await
}
