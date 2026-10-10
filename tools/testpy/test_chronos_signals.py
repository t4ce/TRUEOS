#!/usr/bin/env python3
"""Host tests of production civil-boundary subscription logic."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    production = (ROOT / 'src/chronos/signals.rs').read_text()
    production = production.split('#[trueos_executor::task]')[0]
    production = production.replace('use spin::Mutex;', 'use crate::spin::Mutex;')
    source = r'''
extern crate alloc;
mod spin {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
        pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
    }
}
mod wait {
    use std::sync::atomic::{AtomicU32,Ordering};
    pub struct WaitQueue(AtomicU32);
    impl WaitQueue {
        pub const fn new()->Self {Self(AtomicU32::new(0))}
        pub fn observe(&self)->u32 {self.0.load(Ordering::Acquire)}
        pub fn notify_all(&self) {self.0.fetch_add(1,Ordering::Release);}
        pub async fn wait_after(&self,_:u32) {std::future::pending::<()>().await}
    }
}
mod time {pub fn uptime_seconds()->u64 {0}}
mod locale {pub fn local_unix_time_seconds(utc:u64)->u64 {utc+3600}}
mod chronos {
    pub fn best_effort_unix_time_seconds()->Option<u64> {Some(59)}
    mod signals {
'''
    source += production
    source += r'''
    #[test]
    fn civil_boundaries_and_subscriptions() {
        let utc = Every::HOUR.utc();
        assert_eq!(utc.bucket(3599,7199),0);
        assert_eq!(utc.bucket(3600,7200),1);
        for (every,seconds) in [(Every::MINUTE,60),(Every::HOUR,3600),
            (Every::THREE_HOURS,10800),(Every::SIX_HOURS,21600),
            (Every::TWELVE_HOURS,43200)] {
            assert_eq!(every.bucket(0,seconds-1),0);
            assert_eq!(every.bucket(0,seconds),1);
        }
        assert_eq!(Every::minutes(15).unwrap().bucket(0,900),1);
        assert_eq!(Every::hours(3),Some(Every::THREE_HOURS));
        assert!(Every::seconds(0).is_none());
        assert!(Every::minutes(u64::MAX).is_none());
        assert!(Every::hours(u64::MAX).is_none());
        assert!(Every::MINUTE.offset_seconds(60).is_none());
        let shifted = Every::MINUTE.offset_seconds(30).unwrap();
        assert_eq!(shifted.bucket(0,89),0);
        assert_eq!(shifted.bucket(0,90),1);
        let mut minute = subscribe(Every::MINUTE);
        let mut hour = subscribe(Every::HOUR);
        let mut three_hours = subscribe(Every::THREE_HOURS);
        assert!(minute.take().is_none());
        publish(Tick {unix_seconds:59,local_seconds:3659});
        assert!(minute.take().is_none());
        publish(Tick {unix_seconds:60,local_seconds:3660});
        assert_eq!(minute.take().unwrap().unix_seconds,60);
        assert!(minute.take().is_none());
        assert!(hour.take().is_none());
        publish(Tick {unix_seconds:60,local_seconds:3660});
        assert!(minute.take().is_none());
        // Late polling and forward correction deliver one latest event.
        publish(Tick {unix_seconds:120,local_seconds:3720});
        publish(Tick {unix_seconds:7200,local_seconds:10800});
        assert_eq!(minute.take().unwrap().unix_seconds,7200);
        assert!(minute.take().is_none());
        assert_eq!(hour.take().unwrap().local_seconds,10800);
        assert_eq!(three_hours.take().unwrap().local_seconds,10800);
        // Backward NTP/DST corrections also refresh clock consumers.
        publish(Tick {unix_seconds:60,local_seconds:3660});
        assert_eq!(minute.take().unwrap().local_seconds,3660);
        assert!(hour.take().is_some());
        drop(minute); drop(hour); drop(three_hours);
        publish(Tick {unix_seconds:120,local_seconds:3720});
        assert!(SUBSCRIBERS.lock().is_empty());
    }
    }
}
'''
    with tempfile.TemporaryDirectory(prefix='chronos-signals-') as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', '-Awarnings',
                        str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
        subprocess.run([str(path / 'test')], check=True)


if __name__ == '__main__':
    main()
