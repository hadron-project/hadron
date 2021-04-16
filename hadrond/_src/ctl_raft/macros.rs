/// Unpack a result, emitting errors over a specific channel, and returning the Ok value on success.
#[macro_export]
macro_rules! ok_or_else_tx_err {
    ($matcher:expr, $holder:ident) => {
        match $matcher {
            Ok(val) => val,
            Err(err) => {
                tracing::error!(error = ?err);
                let _ = $holder.tx.send(utils::map_result_to_status(Err(err.into())));
                return;
            }
        }
    };
}

/// Write a client request to Raft, then unpack the response and handle it as needed.
///
/// This macro exists simply as a way to cut down on boilerplate. Every request which needs to
/// write data to Raft will use roughly the same pattern, with only a few types being different.
/// This macro handles the boilerplate of pattern matching, error handling, and forwarding.
#[macro_export]
macro_rules! raft_client_write {
    (
        raft: $raft:expr, forward_tx: $forward_tx:expr, req: $req:expr, client_request: $client_request:expr,
        success: $success:pat, success_expr: $success_expr:expr,
        forward: $forward:pat, forward_expr: $forward_expr:expr $(,)*
    ) => {
        #[allow(unreachable_patterns)] // TODO: remove
        match $raft.client_write($client_request).await {
            Ok(res) => match res.data {
                $success => $success_expr,
                _ => {
                    let _ = $req.tx.send(Err(utils::status_from_err(anyhow!(ERR_UNEXPECTED_RAFT_RESPONSE_VARIANT))));
                }
            },
            Err(err) => match err {
                ClientWriteError::RaftError(err) => {
                    tracing::error!(error = ?err, "error while writing to Raft");
                    let _ = $req.tx.send(Err(utils::status_from_err(err.into())));
                }
                ClientWriteError::ForwardToLeader(orig_req, node_opt) => match orig_req {
                    $forward => {
                        let _ = $forward_tx.send(($forward_expr, node_opt));
                    }
                    _ => {
                        let _ = $req.tx.send(Err(utils::status_from_err(anyhow!(ERR_UNEXPECTED_RAFT_RESPONSE_VARIANT))));
                    }
                },
            },
        }
    };
}
