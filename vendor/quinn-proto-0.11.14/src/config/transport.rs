use std::{fmt, sync::Arc};
#[cfg(feature = "qlog")]
use std::{io, sync::Mutex};
use std as hostlib;
use hostlib::time::Instant;

#[cfg(feature = "qlog")]
use qlog::streamer::QlogStreamer;

#[cfg(feature = "qlog")]
use crate::QlogStream;
use crate::{
    Duration, INITIAL_MTU, MAX_UDP_PAYLOAD, VarInt, VarIntBoundsExceeded, congestion,
    connection::qlog::QlogSink,
};

/// Parameters governing the core QUIC state machine
///
/// Default values should be suitable for most internet applications. Applications protocols which
/// forbid remotely-initiated streams should set `max_concurrent_bidi_streams` and
/// `max_concurrent_uni_streams` to zero.
///
/// In some cases, performance or resource requirements can be improved by tuning these values to
/// suit a particular application and/or network connection. In particular, data window sizes can be
/// tuned for a particular expected round trip time, link capacity, and memory availability. Tuning
/// for higher bandwidths and latencies increases worst-case memory consumption, but does not impair
/// performance at lower bandwidths and latencies. The default configuration is tuned for a 100Mbps
/// link with a 100ms round trip time.
pub struct TransportConfig {
    pub(crate) max_concurrent_bidi_streams: VarInt,
    pub(crate) max_concurrent_uni_streams: VarInt,
    pub(crate) max_idle_timeout: Option<VarInt>,
    pub(crate) stream_receive_window: VarInt,
    pub(crate) receive_window: VarInt,
    pub(crate) send_window: u64,
    pub(crate) send_fairness: bool,

    pub(crate) packet_threshold: u32,
    pub(crate) time_threshold: f32,
    pub(crate) initial_rtt: Duration,
    pub(crate) initial_mtu: u16,
    pub(crate) min_mtu: u16,
    pub(crate) mtu_discovery_config: Option<MtuDiscoveryConfig>,
    pub(crate) pad_to_mtu: bool,
    pub(crate) ack_frequency_config: Option<AckFrequencyConfig>,

    pub(crate) persistent_congestion_threshold: u32,
    pub(crate) keep_alive_interval: Option<Duration>,
    pub(crate) crypto_buffer_size: usize,
    pub(crate) allow_spin: bool,
    pub(crate) datagram_receive_buffer_size: Option<usize>,
    pub(crate) datagram_send_buffer_size: usize,

/// Parameters for controlling the peer's acknowledgement frequency
///
/// The parameters provided in this config will be sent to the peer at the beginning of the
/// connection, so it can take them into account when sending acknowledgements (see each parameter's
/// description for details on how it influences acknowledgement frequency).
///
/// Quinn's implementation follows the fourth draft of the
/// [QUIC Acknowledgement Frequency extension](https://datatracker.ietf.org/doc/html/draft-ietf-quic-ack-frequency-04).
/// The defaults produce behavior slightly different than the behavior without this extension,
/// because they change the way reordered packets are handled (see
/// [`AckFrequencyConfig::reordering_threshold`] for details).
#[derive(Clone, Debug)]
pub struct AckFrequencyConfig {
    pub(crate) ack_eliciting_threshold: VarInt,
    pub(crate) max_ack_delay: Option<Duration>,
    pub(crate) reordering_threshold: VarInt,
}

impl AckFrequencyConfig {
    /// The ack-eliciting threshold we will request the peer to use
    ///
    /// This threshold represents the number of ack-eliciting packets an endpoint may receive
    /// without immediately sending an ACK.
    ///
    /// The remote peer should send at least one ACK frame when more than this number of
    /// ack-eliciting packets have been received. A value of 0 results in a receiver immediately
    /// acknowledging every ack-eliciting packet.
    ///
    /// Defaults to 1, which sends ACK frames for every other ack-eliciting packet.
    pub fn ack_eliciting_threshold(&mut self, value: VarInt) -> &mut Self {
        self.ack_eliciting_threshold = value;
        self
    }

    /// The `max_ack_delay` we will request the peer to use
    ///
    /// This parameter represents the maximum amount of time that an endpoint waits before sending
    /// an ACK when the ack-eliciting threshold hasn't been reached.
    ///
    /// The effective `max_ack_delay` will be clamped to be at least the peer's `min_ack_delay`
    /// transport parameter, and at most the greater of the current path RTT or 25ms.
    ///
    /// Defaults to `None`, in which case the peer's original `max_ack_delay` will be used, as
    /// obtained from its transport parameters.
    pub fn max_ack_delay(&mut self, value: Option<Duration>) -> &mut Self {
        self.max_ack_delay = value;
        self
    }

    /// The reordering threshold we will request the peer to use
    ///
    /// This threshold represents the amount of out-of-order packets that will trigger an endpoint
    /// to send an ACK, without waiting for `ack_eliciting_threshold` to be exceeded or for
    /// `max_ack_delay` to be elapsed.
    ///
    /// A value of 0 indicates out-of-order packets do not elicit an immediate ACK. A value of 1
    /// immediately acknowledges any packets that are received out of order (this is also the
    /// behavior when the extension is disabled).
    ///
    /// It is recommended to set this value to [`TransportConfig::packet_threshold`] minus one.
    /// Since the default value for [`TransportConfig::packet_threshold`] is 3, this value defaults
    /// to 2.
    pub fn reordering_threshold(&mut self, value: VarInt) -> &mut Self {
        self.reordering_threshold = value;
        self
    }
}

impl Default for AckFrequencyConfig {
    fn default() -> Self {
        Self {
            ack_eliciting_threshold: VarInt(1),
            max_ack_delay: None,
            reordering_threshold: VarInt(2),
        }
    }
}

/// Configuration for qlog trace logging
#[cfg(feature = "qlog")]
pub struct QlogConfig {
    writer: Option<Box<dyn io::Write + Send + Sync>>,
    title: Option<String>,
    description: Option<String>,
    start_time: Instant,
}

#[cfg(feature = "qlog")]
impl QlogConfig {
    /// Where to write a qlog `TraceSeq`
    pub fn writer(&mut self, writer: Box<dyn io::Write + Send + Sync>) -> &mut Self {
        self.writer = Some(writer);
        self
    }

    /// Title to record in the qlog capture
    pub fn title(&mut self, title: Option<String>) -> &mut Self {
        self.title = title;
        self
    }

    /// Description to record in the qlog capture
    pub fn description(&mut self, description: Option<String>) -> &mut Self {
        self.description = description;
        self
    }

    /// Epoch qlog event times are recorded relative to
    pub fn start_time(&mut self, start_time: Instant) -> &mut Self {
        self.start_time = start_time;
        self
    }

    /// Construct the [`QlogStream`] described by this configuration
    pub fn into_stream(self) -> Option<QlogStream> {
        use tracing::warn;

        let writer = self.writer?;
        let trace = qlog::TraceSeq::new(
            qlog::VantagePoint {
                name: None,
                ty: qlog::VantagePointType::Unknown,
                flow: None,
            },
            self.title.clone(),
            self.description.clone(),
            Some(qlog::Configuration {
                time_offset: Some(0.0),
                original_uris: None,
            }),
            None,
        );

        let mut streamer = QlogStreamer::new(
            qlog::QLOG_VERSION.into(),
            self.title,
            self.description,
            None,
            self.start_time,
            trace,
            qlog::events::EventImportance::Core,
            writer,
        );

        match streamer.start_log() {
            Ok(()) => Some(QlogStream(Arc::new(Mutex::new(streamer)))),
            Err(e) => {
                warn!("could not initialize endpoint qlog streamer: {e}");
                None
            }
        }
    }
}

#[cfg(feature = "qlog")]
impl Default for QlogConfig {
    fn default() -> Self {
        Self {
            writer: None,
            title: None,
            description: None,
            start_time: Instant::now(),
        }
    }
}

/// Parameters governing MTU discovery.
///
/// # The why of MTU discovery
///
/// By design, QUIC ensures during the handshake that the network path between the client and the
/// server is able to transmit unfragmented UDP packets with a body of 1200 bytes. In other words,
/// once the connection is established, we know that the network path's maximum transmission unit
/// (MTU) is of at least 1200 bytes (plus IP and UDP headers). Because of this, a QUIC endpoint can
/// split outgoing data in packets of 1200 bytes, with confidence that the network will be able to
/// deliver them (if the endpoint were to send bigger packets, they could prove too big and end up
/// being dropped).
///
/// There is, however, a significant overhead associated to sending a packet. If the same
/// information can be sent in fewer packets, that results in higher throughput. The amount of
/// packets that need to be sent is inversely proportional to the MTU: the higher the MTU, the
/// bigger the packets that can be sent, and the fewer packets that are needed to transmit a given
/// amount of bytes.
///
/// Most networks have an MTU higher than 1200. Through MTU discovery, endpoints can detect the
/// path's MTU and, if it turns out to be higher, start sending bigger packets.
///
/// # MTU discovery internals
///
/// Quinn implements MTU discovery through DPLPMTUD (Datagram Packetization Layer Path MTU
/// Discovery), described in [section 14.3 of RFC
/// 9000](https://www.rfc-editor.org/rfc/rfc9000.html#section-14.3). This method consists of sending
/// QUIC packets padded to a particular size (called PMTU probes), and waiting to see if the remote
/// peer responds with an ACK. If an ACK is received, that means the probe arrived at the remote
/// peer, which in turn means that the network path's MTU is of at least the packet's size. If the
/// probe is lost, it is sent another 2 times before concluding that the MTU is lower than the
/// packet's size.
///
/// MTU discovery runs on a schedule (e.g. every 600 seconds) specified through
/// [`MtuDiscoveryConfig::interval`]. The first run happens right after the handshake, and
/// subsequent discoveries are scheduled to run when the interval has elapsed, starting from the
/// last time when MTU discovery completed.
///
/// Since the search space for MTUs is quite big (the smallest possible MTU is 1200, and the highest
/// is 65527), Quinn performs a binary search to keep the number of probes as low as possible. The
/// lower bound of the search is equal to [`TransportConfig::initial_mtu`] in the
/// initial MTU discovery run, and is equal to the currently discovered MTU in subsequent runs. The
/// upper bound is determined by the minimum of [`MtuDiscoveryConfig::upper_bound`] and the
/// `max_udp_payload_size` transport parameter received from the peer during the handshake.
///
/// # Black hole detection
///
/// If, at some point, the network path no longer accepts packets of the detected size, packet loss
/// will eventually trigger black hole detection and reset the detected MTU to 1200. In that case,
/// MTU discovery will be triggered after [`MtuDiscoveryConfig::black_hole_cooldown`] (ignoring the
/// timer that was set based on [`MtuDiscoveryConfig::interval`]).
///
/// # Interaction between peers
///
/// There is no guarantee that the MTU on the path between A and B is the same as the MTU of the
/// path between B and A. Therefore, each peer in the connection needs to run MTU discovery
/// independently in order to discover the path's MTU.
#[derive(Clone, Debug)]
pub struct MtuDiscoveryConfig {
    pub(crate) interval: Duration,
    pub(crate) upper_bound: u16,
    pub(crate) minimum_change: u16,
    pub(crate) black_hole_cooldown: Duration,
}

impl MtuDiscoveryConfig {
    /// Specifies the time to wait after completing MTU discovery before starting a new MTU
    /// discovery run.
    ///
    /// Defaults to 600 seconds, as recommended by [RFC
    /// 8899](https://www.rfc-editor.org/rfc/rfc8899).
    pub fn interval(&mut self, value: Duration) -> &mut Self {
        self.interval = value;
        self
    }

    /// Specifies the upper bound to the max UDP payload size that MTU discovery will search for.
    ///
    /// Defaults to 1452, to stay within Ethernet's MTU when using IPv4 and IPv6. The highest
    /// allowed value is 65527, which corresponds to the maximum permitted UDP payload on IPv6.
    ///
    /// It is safe to use an arbitrarily high upper bound, regardless of the network path's MTU. The
    /// only drawback is that MTU discovery might take more time to finish.
    pub fn upper_bound(&mut self, value: u16) -> &mut Self {
        self.upper_bound = value.min(MAX_UDP_PAYLOAD);
        self
    }

    /// Specifies the amount of time that MTU discovery should wait after a black hole was detected
    /// before running again. Defaults to one minute.
    ///
    /// Black hole detection can be spuriously triggered in case of congestion, so it makes sense to
    /// try MTU discovery again after a short period of time.
    pub fn black_hole_cooldown(&mut self, value: Duration) -> &mut Self {
        self.black_hole_cooldown = value;
        self
    }

    /// Specifies the minimum MTU change to stop the MTU discovery phase.
    /// Defaults to 20.
    pub fn minimum_change(&mut self, value: u16) -> &mut Self {
        self.minimum_change = value;
        self
    }
}

impl Default for MtuDiscoveryConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(600),
            upper_bound: 1452,
            black_hole_cooldown: Duration::from_secs(60),
            minimum_change: 20,
        }
    }
}

/// Maximum duration of inactivity to accept before timing out the connection
///
/// This wraps an underlying [`VarInt`], representing the duration in milliseconds. Values can be
/// constructed by converting directly from `VarInt`, or using `TryFrom<Duration>`.
///
/// ```
/// # use core::time::Duration; use std::convert::TryFrom;
/// # use quinn_proto::{IdleTimeout, VarIntBoundsExceeded, VarInt};
/// # fn main() -> Result<(), VarIntBoundsExceeded> {
/// // A `VarInt`-encoded value in milliseconds
/// let timeout = IdleTimeout::from(VarInt::from_u32(10_000));
///
/// // Try to convert a `Duration` into a `VarInt`-encoded timeout
/// let timeout = IdleTimeout::try_from(Duration::from_secs(10))?;
/// # Ok(())
/// # }
/// ```
#[derive(Default, Copy, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IdleTimeout(VarInt);

impl From<VarInt> for IdleTimeout {
    fn from(inner: VarInt) -> Self {
        Self(inner)
    }
}

impl std::convert::TryFrom<Duration> for IdleTimeout {
    type Error = VarIntBoundsExceeded;

    fn try_from(timeout: Duration) -> Result<Self, Self::Error> {
        let inner = VarInt::try_from(timeout.as_millis())?;
        Ok(Self(inner))
    }
}

impl fmt::Debug for IdleTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
