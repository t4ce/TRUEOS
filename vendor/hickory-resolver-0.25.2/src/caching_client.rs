// Copyright 2015-2023 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// https://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Caching related functionality for the Resolver.

use alloc::{boxed::Box, vec, vec::Vec};
use crate::time::Instant;
use std::{borrow::Cow, future::Future, pin::Pin, sync::Arc};

use futures_util::future::TryFutureExt;
use once_cell::sync::Lazy;

use crate::{
    dns_lru::{self, DnsLru, TtlConfig},
    hickory_error::ResolveError,
    lookup::Lookup,
    proto::{
        op::{Query, ResponseCode},
        rr::{
            DNSClass, Name, RData, Record, RecordType,
            domain::usage::{
                DEFAULT, IN_ADDR_ARPA_127, INVALID, IP6_ARPA_1, LOCAL,
                LOCALHOST as LOCALHOST_usage, ONION, ResolverUsage,
            },
            rdata::{A, AAAA, CNAME, PTR, SOA},
            resource::RecordRef,
        },
        xfer::{DnsHandle, DnsRequestOptions, DnsResponse, FirstAnswer},
        {ForwardNSData, ProtoError, ProtoErrorKind},
    },
};

static LOCALHOST: Lazy<RData> =
    Lazy::new(|| RData::PTR(PTR(Name::from_ascii("localhost.").unwrap())));
static LOCALHOST_V4: Lazy<RData> = Lazy::new(|| RData::A(A::new(127, 0, 0, 1)));
static LOCALHOST_V6: Lazy<RData> = Lazy::new(|| RData::AAAA(AAAA::new(0, 0, 0, 0, 0, 0, 0, 1)));

/// Counts the depth of CNAME query resolutions.
#[derive(Default, Clone, Copy)]
struct DepthTracker {
    query_depth: u8,
}

impl DepthTracker {
    fn nest(self) -> Self {
        Self {
            query_depth: self.query_depth + 1,
        }
    }

    fn is_exhausted(self) -> bool {
        self.query_depth + 1 >= Self::MAX_QUERY_DEPTH
    }

    const MAX_QUERY_DEPTH: u8 = 8; // arbitrarily chosen number...
}

// TODO: need to consider this storage type as it compares to Authority in server...
//       should it just be an variation on Authority?
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct CachingClient<C>
where
    C: DnsHandle,
{
    lru: DnsLru,
    client: C,
    preserve_intermediates: bool,
}

impl<C> CachingClient<C>
where
    C: DnsHandle + Send + 'static,
{
    #[doc(hidden)]
    pub fn new(max_size: usize, client: C, preserve_intermediates: bool) -> Self {
        Self::with_cache(
            DnsLru::new(max_size, TtlConfig::default()),
            client,
            preserve_intermediates,
        )
    }

    pub(crate) fn with_cache(lru: DnsLru, client: C, preserve_intermediates: bool) -> Self {
        Self {
            lru,
            client,
            preserve_intermediates,
        }
    }

    /// Perform a lookup against this caching client, looking first in the cache for a result
    pub fn lookup(
        &self,
        query: Query,
        options: DnsRequestOptions,
    ) -> Pin<Box<dyn Future<Output = Result<Lookup, ResolveError>> + Send>> {
        Box::pin(
            Self::inner_lookup(
                query,
                options,
                self.clone(),
                vec![],
                DepthTracker::default(),
            )
            .map_err(ResolveError::from),
        )
    }

    async fn inner_lookup(
        query: Query,
        options: DnsRequestOptions,
        mut client: Self,
        preserved_records: Vec<(Record, u32)>,
        depth: DepthTracker,
    ) -> Result<Lookup, ProtoError> {
        // see https://tools.ietf.org/html/rfc6761
        //
        // ```text
        // Name resolution APIs and libraries SHOULD recognize localhost
        // names as special and SHOULD always return the IP loopback address
        // for address queries and negative responses for all other query
        // types.  Name resolution APIs SHOULD NOT send queries for
        // localhost names to their configured caching DNS server(s).
        // ```
        // special use rules only apply to the IN Class
        if query.query_class() == DNSClass::IN {
            let usage = match query.name() {
                n if LOCALHOST_usage.zone_of(n) => &*LOCALHOST_usage,
                n if IN_ADDR_ARPA_127.zone_of(n) => &*LOCALHOST_usage,
                n if IP6_ARPA_1.zone_of(n) => &*LOCALHOST_usage,
                n if INVALID.zone_of(n) => &*INVALID,
                n if LOCAL.zone_of(n) => &*LOCAL,
                n if ONION.zone_of(n) => &*ONION,
                _ => &*DEFAULT,
            };

            match usage.resolver() {
                ResolverUsage::Loopback => match query.query_type() {
                    // TODO: look in hosts for these ips/names first...
                    RecordType::A => return Ok(Lookup::from_rdata(query, LOCALHOST_V4.clone())),
                    RecordType::AAAA => return Ok(Lookup::from_rdata(query, LOCALHOST_V6.clone())),
                    RecordType::PTR => return Ok(Lookup::from_rdata(query, LOCALHOST.clone())),
                    _ => {
                        return Err(ProtoError::nx_error(
                            Box::new(query),
                            None,
                            None,
                            None,
                            ResponseCode::NoError,
                            false,
                            None,
                        ));
                    } // Are there any other types we can use?
                },
                // TODO: this requires additional config, as Kubernetes and other systems misuse the .local. zone.
                // when mdns is not enabled we will return errors on LinkLocal ("*.local.") names
                ResolverUsage::LinkLocal => (),
                ResolverUsage::NxDomain => {
                    return Err(ProtoError::nx_error(
                        Box::new(query),
                        None,
                        None,
                        None,
                        ResponseCode::NXDomain,
                        false,
                        None,
                    ));
                }
                ResolverUsage::Normal => (),
            }
        }

        let is_dnssec = client.client.is_verifying_dnssec();

        // first transition any polling that is needed (mutable refs...)
        if let Some(cached_lookup) = client.lookup_from_cache(&query) {
            return cached_lookup;
        };

        let response_message = client
            .client
            .lookup(query.clone(), options)
            .first_answer()
            .await;

        // TODO: technically this might be duplicating work, as name_server already performs this evaluation.
        //  we may want to create a new type, if evaluated... but this is most generic to support any impl in LookupState...
        let response_message = if let Ok(response) = response_message {
            ProtoError::from_response(response, false)
        } else {
            response_message
        };

        // TODO: take all records and cache them?
        //  if it's DNSSEC they must be signed, otherwise?
        let records: Result<Records, ProtoError> = match response_message {
            // this is the only cacheable form
            Err(e) => {
                match e.kind() {
                    ProtoErrorKind::NoRecordsFound {
                        query,
                        soa,
                        negative_ttl,
                        response_code,
                        trusted,
                        ns,
                        ..
                    } => {
                        Err(Self::handle_nxdomain(
                            is_dnssec,
                            false, /*tbd*/
                            query.as_ref().clone(),
                            soa.as_ref().map(Box::as_ref).cloned(),
                            ns.clone(),
                            *negative_ttl,
                            *response_code,
                            *trusted,
                        ))
                    }
                    _ => return Err(e),
                }
            }
            Ok(response_message) => {
                // allow the handle_noerror function to deal with any error codes
                let records = Self::handle_noerror(
                    &mut client,
                    options,
                    is_dnssec,
                    &query,
                    response_message,
                    preserved_records,
                    depth,
                )?;

                Ok(records)
            }
        };

        // after the request, evaluate if we have additional queries to perform
        match records {
            Ok(Records::CnameChain {
                next: future,
                min_ttl: ttl,
            }) => match future.await {
                Ok(lookup) => client.cname(lookup, query, ttl),
                Err(e) => client.cache(query, Err(e)),
            },
            Ok(Records::Exists(rdata)) => client.cache(query, Ok(rdata)),
            Err(e) => client.cache(query, Err(e)),
        }
    }

    /// Check if this query is already cached
    fn lookup_from_cache(&self, query: &Query) -> Option<Result<Lookup, ProtoError>> {
        self.lru.get(query, Instant::now())
    }

    /// See https://tools.ietf.org/html/rfc2308
    ///
    /// For now we will regard NXDomain to strictly mean the query failed
    ///  and a record for the name, regardless of CNAME presence, what have you
    ///  ultimately does not exist.
    ///
    /// This also handles empty responses in the same way. When performing DNSSEC enabled queries, we should
    ///  never enter here, and should never cache unless verified requests.
    ///
    /// TODO: should this should be expanded to do a forward lookup? Today, this will fail even if there are
    ///   forwarding options.
    ///
    /// # Arguments
    ///
    /// * `message` - message to extract SOA, etc, from for caching failed requests
    /// * `valid_nsec` - species that in DNSSEC mode, this request is safe to cache
    /// * `negative_ttl` - this should be the SOA minimum for negative ttl
    #[allow(clippy::too_many_arguments)]
    fn handle_nxdomain(
        is_dnssec: bool,
        valid_nsec: bool,
        query: Query,
        soa: Option<Record<SOA>>,
        ns: Option<Arc<[ForwardNSData]>>,
        negative_ttl: Option<u32>,
        response_code: ResponseCode,
        trusted: bool,
    ) -> ProtoError {
        if valid_nsec || !is_dnssec {
            // only trust if there were validated NSEC records
            ProtoErrorKind::NoRecordsFound {
                query: Box::new(query),
                soa: soa.map(Box::new),
                ns,
                negative_ttl,
                response_code,
                trusted: true,
                authorities: None,
            }
            .into()
        } else {
            // not cacheable, no ttl...
            ProtoErrorKind::NoRecordsFound {
                query: Box::new(query),
                soa: soa.map(Box::new),
                ns,
                negative_ttl: None,
                response_code,
                trusted,
                authorities: None,
            }
            .into()
        }
    }

    /// Handle the case where there is no error returned
    fn handle_noerror(
        client: &mut Self,
        options: DnsRequestOptions,
        is_dnssec: bool,
        query: &Query,
        response: DnsResponse,
        mut preserved_records: Vec<(Record, u32)>,
        depth: DepthTracker,
    ) -> Result<Records, ProtoError> {
        // initial ttl is what CNAMES for min usage
        const INITIAL_TTL: u32 = dns_lru::MAX_TTL;

        // need to capture these before the subsequent and destructive record processing
        let soa = response.soa().as_ref().map(RecordRef::to_owned);
        let negative_ttl = response.negative_ttl();
        let response_code = response.response_code();

        // seek out CNAMES, this is only performed if the query is not a CNAME, ANY, or SRV
        // FIXME: for SRV this evaluation is inadequate. CNAME is a single chain to a single record
        //   for SRV, there could be many different targets. The search_name needs to be enhanced to
        //   be a list of names found for SRV records.
        let (search_name, cname_ttl, was_cname, preserved_records) = {
            // this will only search for CNAMEs if the request was not meant to be for one of the triggers for recursion
            let (search_name, cname_ttl, was_cname) =
                if query.query_type().is_any() || query.query_type().is_cname() {
                    (Cow::Borrowed(query.name()), INITIAL_TTL, false)
                } else {
                    // Folds any cnames from the answers section, into the final cname in the answers section
                    //   this works by folding the last CNAME found into the final folded result.
                    //   it assumes that the CNAMEs are in chained order in the DnsResponse Message...
                    // For SRV, the name added for the search becomes the target name.
                    //
                    // TODO: should this include the additionals?
                    response.answers().iter().fold(
                        (Cow::Borrowed(query.name()), INITIAL_TTL, false),
                        |(search_name, cname_ttl, was_cname), r| {
                            match r.data() {
                                RData::CNAME(CNAME(cname)) => {
                                    // take the minimum TTL of the cname_ttl and the next record in the chain
                                    let ttl = cname_ttl.min(r.ttl());
                                    debug_assert_eq!(r.record_type(), RecordType::CNAME);
                                    if search_name.as_ref() == r.name() {
                                        return (Cow::Owned(cname.clone()), ttl, true);
                                    }
                                }
                                RData::SRV(srv) => {
                                    // take the minimum TTL of the cname_ttl and the next record in the chain
                                    let ttl = cname_ttl.min(r.ttl());
                                    debug_assert_eq!(r.record_type(), RecordType::SRV);

                                    // the search name becomes the srv.target
                                    return (Cow::Owned(srv.target().clone()), ttl, true);
                                }
                                _ => (),
                            }

                            (search_name, cname_ttl, was_cname)
                        },
                    )
                };

            // take all answers. // TODO: following CNAMES?
            let mut response = response.into_message();
            let answers = response.take_answers();
            let additionals = response.take_additionals();
            let name_servers = response.take_name_servers();

            // set of names that still require resolution
            // TODO: this needs to be enhanced for SRV
            let mut found_name = false;

            // After following all the CNAMES to the last one, try and lookup the final name
            let records = answers
                .into_iter()
                // Chained records will generally exist in the additionals section
                .chain(additionals)
                .chain(name_servers)
                .filter_map(|r| {
                    // because this resolved potentially recursively, we want the min TTL from the chain
                    let ttl = cname_ttl.min(r.ttl());
                    // TODO: disable name validation with ResolverOpts? glibc feature...
                    // restrict to the RData type requested
                    if query.query_class() == r.dns_class() {
                        // standard evaluation, it's an any type or it's the requested type and the search_name matches
                        #[allow(clippy::suspicious_operation_groupings)]
                        if (query.query_type().is_any() || query.query_type() == r.record_type())
                            && (search_name.as_ref() == r.name() || query.name() == r.name())
                        {
                            found_name = true;
                            return Some((r, ttl));
                        }
                        // CNAME evaluation, the record is from the CNAME lookup chain.
                        if client.preserve_intermediates && r.record_type() == RecordType::CNAME {
                            return Some((r, ttl));
                        }
                        // srv evaluation, it's an srv lookup and the srv_search_name/target matches this name
                        //    and it's an IP
                        if query.query_type().is_srv()
                            && r.record_type().is_ip_addr()
                            && search_name.as_ref() == r.name()
                        {
                            found_name = true;
                            Some((r, ttl))
                        } else if query.query_type().is_ns() && r.record_type().is_ip_addr() {
                            Some((r, ttl))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            // adding the newly collected records to the preserved records
            preserved_records.extend(records);
            if !preserved_records.is_empty() && found_name {
                return Ok(Records::Exists(preserved_records));
            }

            (
                search_name.into_owned(),
                cname_ttl,
                was_cname,
                preserved_records,
            )
        };

        // TODO: for SRV records we *could* do an implicit lookup, but, this requires knowing the type of IP desired
        //    for now, we'll make the API require the user to perform a follow up to the lookups.
        // It was a CNAME, but not included in the request...
        if was_cname && !depth.is_exhausted() {
            let next_query = Query::query(search_name, query.query_type());
            Ok(Records::CnameChain {
                next: Box::pin(Self::inner_lookup(
                    next_query,
                    options,
                    client.clone(),
                    preserved_records,
                    depth.nest(),
                )),
                min_ttl: cname_ttl,
            })
        } else {
            // TODO: review See https://tools.ietf.org/html/rfc2308 for NoData section
            // Note on DNSSEC, in secure_client_handle, if verify_nsec fails then the request fails.
            //   this will mean that no unverified negative caches will make it to this point and be stored
            Err(Self::handle_nxdomain(
                is_dnssec,
                true,
                query.clone(),
                soa,
                None,
                negative_ttl,
                response_code,
                false,
            ))
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn cname(&self, lookup: Lookup, query: Query, cname_ttl: u32) -> Result<Lookup, ProtoError> {
        // this duplicates the cache entry under the original query
        Ok(self.lru.duplicate(query, lookup, cname_ttl, Instant::now()))
    }

    fn cache(
        &self,
        query: Query,
        records: Result<Vec<(Record, u32)>, ProtoError>,
    ) -> Result<Lookup, ProtoError> {
        // this will put this object into an inconsistent state, but no one should call poll again...
        match records {
            Ok(rdata) => Ok(self.lru.insert(query, rdata, Instant::now())),
            Err(err) => Err(self.lru.negative(query, err, Instant::now())),
        }
    }

    /// Flushes/Removes all entries from the cache
    pub fn clear_cache(&self) {
        self.lru.clear();
    }
}

enum Records {
    /// The records exists, a vec of rdata with ttl
    Exists(Vec<(Record, u32)>),
    /// Future lookup for recursive cname records
    CnameChain {
        next: Pin<Box<dyn Future<Output = Result<Lookup, ProtoError>> + Send>>,
        min_ttl: u32,
    },
}

// see also the lookup_tests.rs in integration-tests crate
