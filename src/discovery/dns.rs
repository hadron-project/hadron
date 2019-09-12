use std::{
    cell::RefCell,
    net::SocketAddr,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use actix::prelude::*;
use log::{debug, error};
use tokio_timer::Interval;
use trust_dns_resolver::{
    AsyncResolver,
    error::ResolveError,
    lookup_ip::LookupIp,
    system_conf::read_system_conf,
};

use crate::{
    config::Config,
    discovery::Discovery,
};

/// An actor used for DNS based peer discovery.
///
/// This discovery system will check for new peers on a regular interval, emitting discovered
/// peers on a regular interval.
pub struct DnsDiscovery {
    config: Arc<Config>,
    discovery_dns_name: String,
    resolver: Rc<AsyncResolver>,
    query_stream_handle: Option<SpawnHandle>,
    has_active_query: Rc<RefCell<bool>>,
    discovery: Addr<Discovery>,
}

impl DnsDiscovery {
    /// Create a new DNS peer discovery backend instance.
    pub fn new(discovery: Addr<Discovery>, discovery_dns_name: String, config: Arc<Config>) -> Self {
        // Build the async resolver.
        let (resolvercfg, resolveropts) = read_system_conf()
            .unwrap_or_else(|err| panic!("Failed to read system DNS config. On *nix systems, ensure your resolv.conf is present and properly formed. {}", err));
        let (resolver, f) = AsyncResolver::new(resolvercfg, resolveropts);
        Arbiter::spawn(f);

        Self{
            config,
            discovery_dns_name,
            resolver: Rc::new(resolver),
            query_stream_handle: None,
            has_active_query: Rc::new(RefCell::new(false)),
            discovery,
        }
    }
}

impl Actor for DnsDiscovery {
    type Context = Context<Self>;

    /// Initialize this actor's internal behaviors on startup.
    ///
    /// This will begin the process of submitting DNS SRV queries to discover cluster peers on the
    /// configured DNS name.
    fn started(&mut self, ctx: &mut Context<Self>) {
        debug!("Booting the DNS discovery backend.");

        // Clone a few containers for move into closures.
        let discovery_name = self.discovery_dns_name.clone();
        let has_active_query = self.has_active_query.clone();
        let resolver = self.resolver.clone();

        // TODO: update this to use ctx.run_interval.
        // TODO: update this to use https://docs.rs/actix/0.8.1/actix/actors/resolver/struct.Resolver.html
        self.query_stream_handle = Some(Self::add_stream(
            Interval::new_interval(Duration::from_secs(10))
                // Simply map timer errors over to resolver errors.
                .map_err(|tokio_err| ResolveError::from(tokio_err.to_string()))

                // Perform a new DNS lookup on each interval.
                .and_then(move |_| -> Box<dyn Future<Item=Option<LookupIp>, Error=ResolveError>> {
                    let is_active = *has_active_query.borrow();
                    if is_active {
                        debug!("Starting new DNS peer discovery request.");
                        *has_active_query.borrow_mut() = true;
                        let (inner_rc0, inner_rc1) = (has_active_query.clone(), has_active_query.clone());
                        Box::new(resolver.lookup_ip(discovery_name.as_str())
                            .map(move |lookup_res| {
                                *inner_rc0.borrow_mut() = false;
                                Some(lookup_res)
                            })
                            .map_err(move |err| {
                                *inner_rc1.borrow_mut() = false;
                                err
                            }))
                    } else {
                        debug!("DNS discovery interval hit, but request is already in flight. Skipping.");
                        Box::new(futures::future::ok(None))
                    }
                }),
            ctx,
        ));
    }
}

impl StreamHandler<Option<LookupIp>, ResolveError> for DnsDiscovery {
    /// Handle updates coming from the DNS polling stream.
    fn handle(&mut self, item: Option<LookupIp>, _: &mut Self::Context) {
        let records = match item {
            None => return,
            Some(srv) => srv,
        };

        // Unconditionally send the payload over to the discovery actor.
        self.discovery.do_send(DnsAddrs(
            records.iter().map(|rcd| SocketAddr::from((rcd, self.config.port))).collect()
        ));
    }

    /// Handle errors coming from the DNS polling stream.
    fn error(&mut self, error: ResolveError, _: &mut Self::Context) -> Running {
        error!("Error from DNS peer discovery routine: {:?}", error);
        Running::Continue
    }
}

/// A type representing a payload of IP addresses from a DNS discovery probe.
pub struct DnsAddrs(pub Vec<SocketAddr>);

impl Message for DnsAddrs {
    type Result = ();
}
