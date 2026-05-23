use crate::{IpNet, Ipv4Net, Ipv6Net};
use core::fmt;
#[cfg(not(feature = "std"))]
use core::net::{Ipv4Addr, Ipv6Addr};
#[cfg(feature = "std")]
use std::net::{Ipv4Addr, Ipv6Addr};
use serde::{self, Serialize, Deserialize, Serializer, Deserializer};
use serde::ser::SerializeTuple;
use serde::de::{EnumAccess, Error, VariantAccess, Visitor};

impl Serialize for IpNet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        if serializer.is_human_readable() {
            match *self {
                IpNet::V4(ref a) => a.serialize(serializer),
                IpNet::V6(ref a) => a.serialize(serializer),
            }
        } else {
            match *self {
                IpNet::V4(ref a) => serializer.serialize_newtype_variant("IpNet", 0, "V4", a),
                IpNet::V6(ref a) => serializer.serialize_newtype_variant("IpNet", 1, "V6", a),
            }
        }
    }
}

impl<'de> Deserialize<'de> for IpNet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de>
    {
        if deserializer.is_human_readable() {
            struct IpNetVisitor;

            impl<'de> Visitor<'de> for IpNetVisitor {
                type Value = IpNet;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("IPv4 or IPv6 network address")
                }

                fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
                    where E: Error
                {
                    s.parse().map_err(Error::custom)
                }
            }

            deserializer.deserialize_str(IpNetVisitor)
        } else {
            struct EnumVisitor;

            #[derive(Serialize, Deserialize)]
            enum IpNetKind {
                V4,
                V6,
            }

            impl<'de> Visitor<'de> for EnumVisitor {
                type Value = IpNet;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("IPv4 or IPv6 network address")
                }

                fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
                    where A: EnumAccess<'de>
                {
                    match data.variant()? {
                        (IpNetKind::V4, v) => v.newtype_variant().map(IpNet::V4),
                        (IpNetKind::V6, v) => v.newtype_variant().map(IpNet::V6),
                    }
                }
            }

            deserializer.deserialize_enum("IpNet", &["V4", "V6"], EnumVisitor)
        }
    }
}

impl Serialize for Ipv4Net {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        if serializer.is_human_readable() {
            #[cfg(feature = "ser_as_str")]
            {
                let mut buf = heapless::String::<18>::new();
                fmt::write(&mut buf, format_args!("{self}")).unwrap();
                serializer.serialize_str(&buf)
            }
            #[cfg(not(feature = "ser_as_str"))]
            serializer.collect_str(self)
        } else {
            let mut seq = serializer.serialize_tuple(5)?;
            for octet in &self.addr().octets() {
                seq.serialize_element(octet)?;
            }
            seq.serialize_element(&self.prefix_len())?;
            seq.end()
        }
    }
}

impl<'de> Deserialize<'de> for Ipv4Net {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de>
    {
        if deserializer.is_human_readable() {
            struct IpAddrVisitor;

            impl<'de> Visitor<'de> for IpAddrVisitor {
                type Value = Ipv4Net;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("IPv4 network address")
                }

                fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
                    where E: Error
                {
                    s.parse().map_err(Error::custom)
                }
            }

            deserializer.deserialize_str(IpAddrVisitor)
        } else {
            let b = <[u8; 5]>::deserialize(deserializer)?;
            Ipv4Net::new(Ipv4Addr::new(b[0], b[1], b[2], b[3]), b[4]).map_err(serde::de::Error::custom)
        }
    }
}

impl Serialize for Ipv6Net {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        if serializer.is_human_readable() {
            #[cfg(feature = "ser_as_str")]
            {
                let mut buf = heapless::String::<43>::new();
                fmt::write(&mut buf, format_args!("{self}")).unwrap();
                serializer.serialize_str(&buf)
            }
            #[cfg(not(feature = "ser_as_str"))]
            serializer.collect_str(self)
        } else {
            let mut seq = serializer.serialize_tuple(17)?;
            for octet in &self.addr().octets() {
                seq.serialize_element(octet)?;
            }
            seq.serialize_element(&self.prefix_len())?;
            seq.end()
        }
    }
}

impl<'de> Deserialize<'de> for Ipv6Net {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de>
    {
        if deserializer.is_human_readable() {
            struct IpAddrVisitor;

            impl<'de> Visitor<'de> for IpAddrVisitor {
                type Value = Ipv6Net;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    formatter.write_str("IPv6 network address")
                }

                fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
                    where E: Error
                {
                    s.parse().map_err(Error::custom)
                }
            }

            deserializer.deserialize_str(IpAddrVisitor)
        } else {
            let b = <[u8; 17]>::deserialize(deserializer)?;
            Ipv6Net::new(Ipv6Addr::new(
                ((b[0] as u16) << 8) | b[1] as u16, ((b[2] as u16) << 8) | b[3] as u16,
                ((b[4] as u16) << 8) | b[5] as u16, ((b[6] as u16) << 8) | b[7] as u16,
                ((b[8] as u16) << 8) | b[9] as u16, ((b[10] as u16) << 8) | b[11] as u16,
                ((b[12] as u16) << 8) | b[13] as u16, ((b[14] as u16) << 8) | b[15] as u16
            ), b[16]).map_err(Error::custom)
        }
    }
}
