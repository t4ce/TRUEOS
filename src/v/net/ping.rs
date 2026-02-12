extern crate alloc;

// use core::sync::atomic::{AtomicU16, Ordering};

// use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use trueos_v::vnet as vnet;

use super::dns::{self, DnsConfig, DnsError};
use super::VNet;

// Ping functionality removed as per request