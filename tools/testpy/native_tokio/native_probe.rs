#![no_std]
extern crate alloc;
use core::time::Duration;
pub fn exercise_runtime() -> usize {
    let rt=tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    rt.block_on(async{
        let (tx,mut rx)=tokio::sync::mpsc::channel(2);
        let task=tokio::spawn(async move{
            tokio::time::sleep(Duration::from_millis(1)).await;
            tx.send(42usize).await.unwrap();
        });
        let value=rx.recv().await.unwrap();
        task.await.unwrap();
        value
    })
}
pub fn exercise_multi_runtime()->usize{
    let rt=tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_time().build().unwrap();
    rt.block_on(async{
        use tokio::io::{AsyncReadExt,AsyncWriteExt};
        let(mut a,mut b)=tokio::io::duplex(3);
        let writer=tokio::spawn(async move{a.write_all(b"runtime").await.unwrap();a.shutdown().await.unwrap();});
        let mut data=alloc::vec::Vec::new();
        b.read_to_end(&mut data).await.unwrap();writer.await.unwrap();
        let mut set=tokio::task::JoinSet::new();for i in 0..8{set.spawn(async move{tokio::task::yield_now().await;i});}
        let mut sum=0;while let Some(value)=set.join_next().await{sum+=value.unwrap();}
        assert_eq!(data,b"runtime");sum
    })
}
