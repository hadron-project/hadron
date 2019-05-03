use std::{
    cell::RefCell,
    rc::Rc,
    time::Duration,
};

use actix::prelude::*;
use log::{debug, error};
use tokio_timer::Interval;
use trust_dns_resolver::{
    AsyncResolver,
    config::{ResolverConfig, ResolverOpts},
    error::ResolveError,
    lookup::SrvLookup,
};

/// An actor used for DNS based peer discovery.
///
/// This discovery system will check for new peers on a regular interval, emitting discovered
/// peers on a regular interval.
pub struct DnsDiscovery {
    resolver: Rc<AsyncResolver>,
    dns_arbiter: Arbiter,
    subscribers: Vec<()>,
    query_stream_handle: Option<SpawnHandle>,
    has_active_query: Rc<RefCell<bool>>,
}

impl DnsDiscovery {
    /// Create a new DNS peer discovery backend instance.
    pub fn new() -> Self {
        // Build the async resolver.
        let (resolver, f) = AsyncResolver::new(ResolverConfig::google(), ResolverOpts::default());

        // Spawn the resolver on its own arbiter. Keep a reference to it so that we can spawn
        // other DNS related futures there.
        let dns_arbiter = Arbiter::new();
        dns_arbiter.send(f);

        Self{
            resolver: Rc::new(resolver),
            dns_arbiter,
            subscribers: Vec::with_capacity(0),
            query_stream_handle: None,
            has_active_query: Rc::new(RefCell::new(false)),
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
        let resolver = self.resolver.clone();
        let has_active_query = self.has_active_query.clone();
        self.query_stream_handle = Some(Self::add_stream(
            Interval::new_interval(Duration::from_secs(10))
                // Simply map timer errors over to resolver errors.
                .map_err(|tokio_err| ResolveError::from(tokio_err.to_string()))

                // Perform a new DNS lookup on each interval.
                .and_then(move |_| -> Box<dyn Future<Item=Option<SrvLookup>, Error=ResolveError>> {
                    let is_active = *has_active_query.borrow();
                    match is_active {
                        false => {
                            debug!("Starting new DNS peer discovery request.");
                            *has_active_query.borrow_mut() = true;
                            let (inner_rc0, inner_rc1) = (has_active_query.clone(), has_active_query.clone());
                            Box::new(resolver.lookup_srv("rg-cluster.default.svc.cluster.local.") // TODO: update this to use provided config.
                                .map(move |srv| {
                                    *inner_rc0.borrow_mut() = false;
                                    Some(srv)
                                })
                                .map_err(move |err| {
                                    *inner_rc1.borrow_mut() = false;
                                    err
                                }))
                        }
                        true => {
                            debug!("DNS discovery interval hit, but request is already in flight. Skipping.");
                            Box::new(futures::future::ok(None))
                        }
                    }
                }),
            ctx,
        ));
    }
}

impl StreamHandler<Option<SrvLookup>, ResolveError> for DnsDiscovery {
    fn handle(&mut self, item: Option<SrvLookup>, _: &mut Self::Context) {
        let srv = match item {
            None => return,
            Some(srv) => srv,
        };

        for addr in srv.ip_iter() {
            debug!("Resolved peer at: {:?}", addr);
        }
    }

    fn error(&mut self, error: ResolveError, _: &mut Self::Context) -> Running {
        error!("Error from DNS peer discovery routine: {:?}", error);
        Running::Continue
    }
}
