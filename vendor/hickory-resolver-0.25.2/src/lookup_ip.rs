// Copyright 2015-2017 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// https://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! LookupIp result from a resolution of ipv4 and ipv6 records with a Resolver.
//!
//! At it's heart LookupIp uses Lookup for performing all lookups. It is unlike other standard lookups in that there are customizations around A and AAAA resolutions.

use alloc::{boxed::Box, vec::Vec};
use core::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use core::pin::Pin;
use std::sync::Arc;
use core::task::{Context, Poll};
use crate::time::Instant;

use futures_util::{FutureExt, future, future::Either};
use tracing::debug;

use crate::proto::op::Query;
use crate::proto::rr::{Name, RData, Record, RecordType};
use crate::proto::xfer::{DnsHandle, DnsRequestOptions};

use crate::caching_client::CachingClient;
use crate::config::LookupIpStrategy;
use crate::dns_lru::MAX_TTL;
use crate::hickory_error::*;
use crate::hosts::Hosts;
use crate::lookup::{Lookup, LookupIntoIter, LookupIter};

/// Result of a DNS query when querying for A or AAAA records.
///
/// When resolving IP records, there can be many IPs that match a given name. A consumer of this should expect that there are more than a single address potentially returned. Generally there are multiple IPs stored for a given service in DNS so that there is a form of high availability offered for a given name. The service implementation is responsible for the semantics around which IP should be used and when, but in general if a connection fails to one, the next in the list should be attempted.
#[derive(Debug, Clone)]
pub struct LookupIp(Lookup);

impl LookupIp {
    /// Returns an iterator over the response records.
    ///
    /// Only IP records will be returned, either A or AAAA record types.
    pub fn iter(&self) -> LookupIpIter<'_> {
        LookupIpIter(self.0.iter())
    }

    /// Returns a reference to the `Query` that was used to produce this result.
    pub fn query(&self) -> &Query {
        self.0.query()
    }

    /// Returns the `Instant` at which this lookup is no longer valid.
    pub fn valid_until(&self) -> Instant {
        self.0.valid_until()
    }

    /// Return a reference to the inner lookup
    ///
    /// This can be useful for getting all records from the request
    pub fn as_lookup(&self) -> &Lookup {
        &self.0
    }
}

impl From<Lookup> for LookupIp {
    fn from(lookup: Lookup) -> Self {
        Self(lookup)
    }
}

impl From<LookupIp> for Lookup {
    fn from(lookup: LookupIp) -> Self {
        lookup.0
    }
}

/// Borrowed view of set of IPs returned from a LookupIp
pub struct LookupIpIter<'i>(pub(crate) LookupIter<'i>);

impl Iterator for LookupIpIter<'_> {
    type Item = IpAddr;

    fn next(&mut self) -> Option<Self::Item> {
        let iter: &mut _ = &mut self.0;
        iter.find_map(|rdata| match rdata {
            RData::A(ip) => Some(IpAddr::from(Ipv4Addr::from(*ip))),
            RData::AAAA(ip) => Some(IpAddr::from(Ipv6Addr::from(*ip))),
            _ => None,
        })
    }
}

impl IntoIterator for LookupIp {
    type Item = IpAddr;
    type IntoIter = LookupIpIntoIter;

    /// This is not a free conversion, because the `RData`s are cloned.
    fn into_iter(self) -> Self::IntoIter {
        LookupIpIntoIter(self.0.into_iter())
    }
}

/// Borrowed view of set of RDatas returned from a Lookup
pub struct LookupIpIntoIter(LookupIntoIter);

impl Iterator for LookupIpIntoIter {
    type Item = IpAddr;

    fn next(&mut self) -> Option<Self::Item> {
        let iter: &mut _ = &mut self.0;
        iter.find_map(|rdata| match rdata {
            RData::A(ip) => Some(IpAddr::from(Ipv4Addr::from(ip))),
            RData::AAAA(ip) => Some(IpAddr::from(Ipv6Addr::from(ip))),
            _ => None,
        })
    }
}

/// The Future returned from [crate::Resolver] when performing an A or AAAA lookup.
///
/// This type isn't necessarily something that should be used by users, see the default TypeParameters are generally correct
pub struct LookupIpFuture<C: DnsHandle + 'static> {
    client_cache: CachingClient<C>,
    names: Vec<Name>,
    strategy: LookupIpStrategy,
    options: DnsRequestOptions,
    query: Pin<Box<dyn Future<Output = Result<Lookup, ResolveError>> + Send>>,
    hosts: Arc<Hosts>,
    finally_ip_addr: Option<RData>,
}

impl<C: DnsHandle + 'static> LookupIpFuture<C> {
    /// Perform a lookup from a hostname to a set of IPs
    ///
    /// # Arguments
    ///
    /// * `names` - a set of DNS names to attempt to resolve, they will be attempted in queue order, i.e. the first is `names.pop()`. Upon each failure, the next will be attempted.
    /// * `strategy` - the lookup IP strategy to use
    /// * `client_cache` - cache with a connection to use for performing all lookups
    pub fn lookup(
        names: Vec<Name>,
        strategy: LookupIpStrategy,
        client_cache: CachingClient<C>,
        options: DnsRequestOptions,
        hosts: Arc<Hosts>,
        finally_ip_addr: Option<RData>,
    ) -> Self {
        let empty =
            ResolveError::from(ResolveErrorKind::Message("can not lookup IPs for no names"));
        Self {
            names,
            strategy,
            client_cache,
            // If there are no names remaining, this will be returned immediately,
            // otherwise, it will be retried.
            query: future::err(empty).boxed(),
            options,
            hosts,
            finally_ip_addr,
        }
    }
}

impl<C: DnsHandle + 'static> Future for LookupIpFuture<C> {
    type Output = Result<LookupIp, ResolveError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            // Try polling the underlying DNS query.
            let query = self.query.as_mut().poll(cx);

            // Determine whether or not we will attempt to retry the query.
            let should_retry = match &query {
                // If the query is NotReady, yield immediately.
                Poll::Pending => return Poll::Pending,
                // If the query returned a successful lookup, we will attempt
                // to retry if the lookup is empty. Otherwise, we will return
                // that lookup.
                Poll::Ready(Ok(lookup)) => lookup.is_empty(),
                // If the query failed, we will attempt to retry.
                Poll::Ready(Err(_)) => true,
            };

            if !should_retry {
                // If we didn't have to retry the query, or we weren't able to
                // retry because we've exhausted the names to search and have no
                // fallback IP address, return the current query.
                return query.map(|f| f.map(LookupIp::from));
            }

            if let Some(name) = self.names.pop() {
                // If there's another name left to try, build a new query
                // for that next name and continue looping.
                self.query = LookupContext {
                    client: self.client_cache.clone(),
                    options: self.options,
                    hosts: self.hosts.clone(),
                }
                .strategic_lookup(name, self.strategy)
                .boxed();
                // Continue looping with the new query. It will be polled
                // on the next iteration of the loop.
                continue;
            } else if let Some(ip_addr) = self.finally_ip_addr.take() {
                // Otherwise, if there's an IP address to fall back to,
                // we'll return it.
                let record = Record::from_rdata(Name::new(), MAX_TTL, ip_addr);
                let lookup = Lookup::new_with_max_ttl(Query::new(), Arc::from([record]));
                return Poll::Ready(Ok(lookup.into()));
            }

            // If we skipped retrying the  query, this will return the
            // successful lookup, otherwise, if the retry failed, this will
            // return the last  query result --- either an empty lookup or the
            // last error we saw.
            return query.map(|f| f.map(LookupIp::from));
        }
    }
}

#[derive(Clone)]
struct LookupContext<C: DnsHandle> {
    client: CachingClient<C>,
    options: DnsRequestOptions,
    hosts: Arc<Hosts>,
}

impl<C: DnsHandle> LookupContext<C> {
    /// returns a new future for lookup
    async fn strategic_lookup(
        self,
        name: Name,
        strategy: LookupIpStrategy,
    ) -> Result<Lookup, ResolveError> {
        match strategy {
            LookupIpStrategy::Ipv4Only => self.ipv4_only(name).await,
            LookupIpStrategy::Ipv6Only => self.ipv6_only(name).await,
            LookupIpStrategy::Ipv4AndIpv6 => self.ipv4_and_ipv6(name).await,
            LookupIpStrategy::Ipv6thenIpv4 => self.ipv6_then_ipv4(name).await,
            LookupIpStrategy::Ipv4thenIpv6 => self.ipv4_then_ipv6(name).await,
        }
    }

    /// queries only for A records
    async fn ipv4_only(&self, name: Name) -> Result<Lookup, ResolveError> {
        self.hosts_lookup(Query::query(name, RecordType::A)).await
    }

    /// queries only for AAAA records
    async fn ipv6_only(&self, name: Name) -> Result<Lookup, ResolveError> {
        self.hosts_lookup(Query::query(name, RecordType::AAAA))
            .await
    }

    // TODO: this really needs to have a stream interface
    /// queries only for A and AAAA in parallel
    async fn ipv4_and_ipv6(&self, name: Name) -> Result<Lookup, ResolveError> {
        let sel_res = future::select(
            self.hosts_lookup(Query::query(name.clone(), RecordType::A))
                .boxed(),
            self.hosts_lookup(Query::query(name, RecordType::AAAA))
                .boxed(),
        )
        .await;

        let (ips, remaining_query) = match sel_res {
            Either::Left(ips_and_remaining) => ips_and_remaining,
            Either::Right(ips_and_remaining) => ips_and_remaining,
        };

        let next_ips = remaining_query.await;

        match (ips, next_ips) {
            (Ok(ips), Ok(next_ips)) => {
                // TODO: create a LookupIp enum with the ability to chain these together
                let ips = ips.append(next_ips);
                Ok(ips)
            }
            (Ok(ips), Err(e)) | (Err(e), Ok(ips)) => {
                debug!(
                    "one of ipv4 or ipv6 lookup failed in ipv4_and_ipv6 strategy: {}",
                    e
                );
                Ok(ips)
            }
            (Err(e1), Err(e2)) => {
                debug!(
                    "both of ipv4 or ipv6 lookup failed in ipv4_and_ipv6 strategy e1: {}, e2: {}",
                    e1, e2
                );
                Err(e1)
            }
        }
    }

    /// queries only for AAAA and on no results queries for A
    async fn ipv6_then_ipv4(&self, name: Name) -> Result<Lookup, ResolveError> {
        self.rt_then_swap(name, RecordType::AAAA, RecordType::A)
            .await
    }

    /// queries only for A and on no results queries for AAAA
    async fn ipv4_then_ipv6(&self, name: Name) -> Result<Lookup, ResolveError> {
        self.rt_then_swap(name, RecordType::A, RecordType::AAAA)
            .await
    }

    /// queries only for first_type and on no results queries for second_type
    async fn rt_then_swap(
        &self,
        name: Name,
        first_type: RecordType,
        second_type: RecordType,
    ) -> Result<Lookup, ResolveError> {
        let res = self
            .hosts_lookup(Query::query(name.clone(), first_type))
            .await;

        match res {
            Ok(ips) => {
                if ips.is_empty() {
                    // no ips returns, NXDomain or Otherwise, doesn't matter
                    self.hosts_lookup(Query::query(name.clone(), second_type))
                        .await
                } else {
                    Ok(ips)
                }
            }
            Err(_) => {
                self.hosts_lookup(Query::query(name.clone(), second_type))
                    .await
            }
        }
    }

    /// first lookups in hosts, then performs the query
    async fn hosts_lookup(&self, query: Query) -> Result<Lookup, ResolveError> {
        match self.hosts.lookup_static_host(&query) {
            Some(lookup) => Ok(lookup),
            None => self.client.lookup(query, self.options).await,
        }
    }
}
